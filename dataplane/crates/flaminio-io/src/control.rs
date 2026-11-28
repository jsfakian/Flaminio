//! The node's control socket: attach and detach volumes while the node runs. One command per
//! connection, one line each way:
//!
//! ```text
//! attach VOLUME TOKEN-BASE64 HANDOFF-BASE64URL [MS] ->  ok /dev/ublkbN   (or: error ...)
//! detach VOLUME                ->  ok
//! status                       ->  ok VOLUME /dev/ublkbN | ok none
//! ```
//!
//! The socket is root-only (mode 0600): an attach hands out the volume. This is what the CSI
//! node plugin will drive.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use flaminio_net::handoff::{Purpose, VerifiedHandoff};
use flaminio_net::token::TokenKey;
use flaminio_node::clock::ClockDomain;
use flaminio_node::engine::Input;
use libublk::ctrl::UblkCtrl;

use crate::node::base64_decode;
use crate::ublk::{ADD_DEVICE_LIMIT, Attached, Control, METADATA_LIMIT, START_LIMIT};

type Wake = Arc<dyn Fn() + Send + Sync>;

/// How long an attach may take before the socket answers without it: the sum of its steps
/// (docs/issues/6). The CSI node plugin waits longer (`DefaultControlTimeout` in
/// control/csi/dataplane.go), so this answer reaches it.
pub(crate) const ATTACH_LIMIT: Duration = METADATA_LIMIT
    .saturating_add(ADD_DEVICE_LIMIT)
    .saturating_add(START_LIMIT);
/// How long a delete may take: below the CSI node plugin's limit, like an attach.
const DELETE_LIMIT: Duration = Duration::from_secs(50);

pub struct ClockControl {
    pub domain: Arc<ClockDomain>,
    pub nonce: [u8; 32],
    pub key: TokenKey,
}

pub fn spawn(
    path: &Path,
    ctl: mpsc::Sender<Control>,
    engine: mpsc::Sender<Input>,
    wake: Wake,
    attached: Attached,
    clock: ClockControl,
) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let clock = Arc::new(clock);
    thread::Builder::new()
        .name("flaminio-control".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let (ctl, engine, wake, attached) =
                    (ctl.clone(), engine.clone(), wake.clone(), attached.clone());
                // Attaches take a while; one thread per command keeps a detach possible.
                let clock = clock.clone();
                thread::spawn(move || serve(conn, &ctl, &engine, &wake, &attached, &clock));
            }
        })
        .map_err(|e| format!("control thread: {e}"))?;
    Ok(())
}

pub(crate) fn serve(
    conn: UnixStream,
    ctl: &mpsc::Sender<Control>,
    engine: &mpsc::Sender<Input>,
    wake: &Wake,
    attached: &Attached,
    clock: &ClockControl,
) {
    let mut line = String::new();
    let Ok(read) = conn.try_clone() else { return };
    if BufReader::new(read.take(64 << 10))
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut observer = clock.domain.observer();
    let answer = match words.as_slice() {
        ["clock-status"] => crate::clock::status(&clock.domain),
        ["attach-challenge"] => match observer.sample().and_then(|sample| observer.success_permit(sample)) {
            Ok(permit) => { permit.consume(); format!("ok {}", clock.nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()) }
            Err(_) => "error: clock quarantined; operator restart required".into(),
        },
        ["attach", volume, token, handoff, within @ ..] if within.len() <= 1 => {
            if observer.sample().is_err() { "error: clock quarantined; operator restart required".into() }
            else if token.len() > 4096 || handoff.len() != 179 { "error: invalid bounded token or handoff".into() }
            else {
                let proof = base64_decode(token).and_then(|token| decode_handoff(handoff).and_then(|handoff| clock.key.verify_handoff(&token, &handoff, &clock.nonce, Purpose::Normal).ok()));
                let current = attached.lock().expect("attached").clone();
                match (proof, attach_limit(within.first().copied()), current) {
                    (None, _, _) => "error: invalid current-process target handoff".into(),
                    (_, Err(e), _) => format!("error: {e}"),
                    (Some(proof), Ok(_), Some(device)) => {
                        if device.volume == *volume && device.proof.capability() == proof.capability() {
                            match device.retry_permit(&proof) {
                                Some(permit) => { permit.consume(); format!("ok /dev/ublkb{}", device.id) }
                                None => if clock.domain.status().is_healthy() { "error: existing target lifetime is no longer live".into() } else { "error: clock quarantined; operator restart required".into() },
                            }
                        } else { format!("error: {} has an existing device lifetime; retry this handoff only after that device stops", device.volume) }
                    },
                    (Some(proof), Ok(limit), None) => {
                        if proof.claims().volume != *volume { "error: handoff volume mismatch".into() }
                        else { attach(ctl, wake, volume, proof, limit) }
                    }
                }
            }
        }
        ["attach", ..] => "error: attach requires VOLUME TOKEN HANDOFF [MS]; obtain an authorized agent/admin Attach bound to attach-challenge".into(),
        ["detach", volume] => detach(engine, wake, attached, volume),
        ["delete", volume] => {
            if observer.sample().is_err() { "error: clock quarantined; operator restart required".into() } else { delete(engine, wake, volume) }
        },
        ["status"] => match attached.lock().expect("attached").clone() {
            Some(device) => format!("ok {} /dev/ublkb{}", device.volume, device.id),
            None => "ok none".into(),
        },
        _ => {
            "error: usage: attach VOLUME TOKEN HANDOFF [MS] | detach VOLUME | delete VOLUME | status | clock-status | attach-challenge"
                .into()
        }
    };
    let mut conn = conn;
    let _ = writeln!(conn, "{answer}");
}

fn decode_handoff(text: &str) -> Option<Vec<u8>> {
    if text.len() != 179
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let standard = text.replace('-', "+").replace('_', "/");
    // 134 bytes leave four significant bits in the final base64 digit.
    let last = *standard.as_bytes().last()?;
    let digit = match last {
        b'A'..=b'Z' => last - b'A',
        b'a'..=b'z' => last - b'a' + 26,
        b'0'..=b'9' => last - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    if digit & 3 != 0 {
        return None;
    }
    let bytes = base64_decode(&standard)?;
    (bytes.len() == flaminio_net::handoff::LEN).then_some(bytes)
}

fn attach(
    ctl: &mpsc::Sender<Control>,
    wake: &Wake,
    volume: &str,
    proof: VerifiedHandoff,
    limit: Duration,
) -> String {
    let (reply, answer) = mpsc::channel();
    if ctl
        .send(Control::Attach {
            volume: volume.to_string(),
            proof,
            reply,
            deadline: Instant::now() + limit,
        })
        .is_err()
    {
        return "error: the node is shutting down".into();
    }
    wake();
    match answer.recv_timeout(limit) {
        Ok(a) => a,
        // The request carries this deadline. Not begun yet, it never will be; begun, its steps
        // have ended by now, but starting the device, which is not bounded, may still bring
        // the device up (docs/issues/6, S2).
        Err(mpsc::RecvTimeoutError::Timeout) => {
            format!("error: no answer within {}", seconds(limit))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            "error: the node dropped the attach without an answer (see its log)".into()
        }
    }
}

/// The limit on an attach: `ATTACH_LIMIT`, or less if the caller says it waits less (its
/// time left, in milliseconds), so that its answer reaches the caller (docs/issues/6).
fn attach_limit(within: Option<&str>) -> Result<Duration, String> {
    match within {
        None => Ok(ATTACH_LIMIT),
        Some(ms) => ms
            .parse()
            .map(|ms| Duration::from_millis(ms).min(ATTACH_LIMIT))
            .map_err(|_| format!("the time limit {ms:?} is not a number of milliseconds")),
    }
}

/// A limit as the socket's errors name it: seconds, with at most one decimal.
pub(crate) fn seconds(d: Duration) -> String {
    let tenths = (d.as_millis() + 50) / 100;
    match tenths % 10 {
        0 => format!("{} s", tenths / 10),
        t => format!("{}.{t} s", tenths / 10),
    }
}

/// Deletes this node's replicas of a volume deleted from the metadata service.
fn delete(engine: &mpsc::Sender<Input>, wake: &Wake, volume: &str) -> String {
    let (reply, answer) = mpsc::channel();
    if engine
        .send(Input::DeleteReplicas {
            volume: volume.to_string(),
            reply,
        })
        .is_err()
    {
        return "error: the node is shutting down".into();
    }
    wake();
    match answer.recv_timeout(DELETE_LIMIT) {
        Ok(Ok(n)) => format!("ok {n} replicas deleted"),
        Ok(Err(e)) => format!("error: {e}"),
        Err(_) => format!("error: no answer within {}", seconds(DELETE_LIMIT)),
    }
}

fn detach(engine: &mpsc::Sender<Input>, wake: &Wake, attached: &Attached, volume: &str) -> String {
    let id = match attached.lock().expect("attached").clone() {
        Some(device) if device.volume == volume => {
            device.live.store(false, Ordering::SeqCst);
            device.id
        }
        _ => return format!("error: {volume} is not attached here"),
    };
    // End the target first, so pending I/O fails cleanly; then stop the device.
    let _ = engine.send(Input::Detach {
        volume: volume.to_string(),
    });
    wake();
    if let Err(e) = UblkCtrl::new_simple(id).and_then(|c| c.kill_dev()) {
        return format!("error: stop ublk device {id}: {e}");
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while attached.lock().expect("attached").is_some() {
        if Instant::now() > deadline {
            return "error: the device did not stop within 30 s".into();
        }
        thread::sleep(Duration::from_millis(50));
    }
    "ok".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_handoff_socket_commands_refuse_before_dispatch() {
        use crate::test_support::{Clock, NONCE, b64, credentials, signer, token_key};
        use flaminio_node::clock::ClockQualification;
        use std::sync::atomic::AtomicUsize;
        let (token, handoff) = credentials("v", "fl-a", 5, 1, &NONCE);
        let encoded_token = b64(&token);
        let rawurl = |bytes: &[u8]| {
            b64(bytes)
                .trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_")
        };
        let encoded_handoff = rawurl(&handoff);
        let mut noncanonical = encoded_handoff.as_bytes().to_vec();
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let last = noncanonical.last_mut().unwrap();
        let digit = alphabet.iter().position(|b| b == last).unwrap();
        assert_eq!(digit & 3, 0);
        *last = alphabet[digit + 1]; // same decoded bytes, forbidden unused bits
        let noncanonical = String::from_utf8(noncanonical).unwrap();
        let mut unknown = handoff[..70].to_vec();
        unknown[4] = 2;
        unknown.extend_from_slice(signer().sign(&unknown).as_ref());
        let lines = [
            "attach".into(),
            "attach v".into(),
            format!("attach v {encoded_token}"),
            format!("attach v {} {encoded_handoff}", "A".repeat(4097)),
            format!("attach v {encoded_token} {}", "A".repeat(180)),
            format!("attach v {encoded_token} {}", "!".repeat(179)),
            format!("attach v {encoded_token} {encoded_handoff}="),
            format!("attach v {encoded_token} {noncanonical} 1"),
            format!("attach v {encoded_token} {}", rawurl(&unknown)),
            format!("attach v {encoded_token} {encoded_handoff} 1 extra"),
        ];
        let domain = Arc::new(
            ClockDomain::new(
                Arc::new(Clock::default()),
                ClockQualification::new(1, 1, 1000).unwrap(),
            )
            .unwrap(),
        );
        let clock = ClockControl {
            domain,
            nonce: NONCE,
            key: token_key(),
        };
        for (case, line) in lines.iter().enumerate() {
            let (ctl, requests) = mpsc::channel();
            let (engine, inputs) = mpsc::channel();
            let wakes = Arc::new(AtomicUsize::new(0));
            let count = wakes.clone();
            let wake: Wake = Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            });
            let attached = Arc::new(std::sync::Mutex::new(None));
            let (mut client, server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            writeln!(client, "{line}").unwrap();
            serve(server, &ctl, &engine, &wake, &attached, &clock);
            let mut answer = String::new();
            BufReader::new(client).read_line(&mut answer).unwrap();
            assert!(answer.starts_with("error:"), "case {case}: {answer}");
            assert!(
                requests.try_recv().is_err(),
                "case {case}: attach dispatched"
            );
            assert!(inputs.try_recv().is_err(), "case {case}: engine mutated");
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                0,
                "case {case}: engine awakened"
            );
            assert!(attached.lock().unwrap().is_none());
        }
    }

    fn proof() -> VerifiedHandoff {
        fn hex(s: &str) -> Vec<u8> {
            s.as_bytes()
                .chunks_exact(2)
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect()
        }
        let key = TokenKey::from_raw(&hex(
            "f80cccdce4ae1c07ae208a2adf99a310ae4207e0306fa0236110b06827bbb8d0 ".trim(),
        ))
        .unwrap();
        key.verify_handoff(&hex("464c43540200017600000000000000070004666c2d6100000000000000000000000000000001e8a6fbb12562c66dbedd45c0b6381850b830a84206caae91f956af865481a307cab6d64e665cd3f89f4f9cbed328c028fb21c97ed3e5d91b111ee60a147ed701"), &hex("464c43480101535353535353535353535353535353535353535353535353535353535353535341b30ce88401383246cdf81d88bbce4c9783641c1ccd4fad99dbc536addd2d4ff04803810d0dca256f98003af248a1d4df145b68970cd403a336ce4746acf1bb3477bba1bca30625ca7ba0ec4d336ff3b83fecda13eca8851aa05eaf6900000e"), &[83;32], Purpose::Normal).unwrap()
    }

    #[test]
    fn an_attach_without_an_answer_times_out_at_its_deadline() {
        let (ctl, requests) = mpsc::channel();
        let wake: Wake = Arc::new(|| {});
        let sent = Instant::now();
        let answer = attach(&ctl, &wake, "v", proof(), Duration::from_millis(100));
        assert_eq!(answer, "error: no answer within 0.1 s");
        // The request carries the caller's deadline, passed now: the node will skip it.
        let Ok(Control::Attach { deadline, .. }) = requests.try_recv() else {
            panic!("no request sent");
        };
        assert!(deadline >= sent + Duration::from_millis(100));
        assert!(deadline <= Instant::now());
    }

    #[test]
    fn an_attach_takes_the_callers_shorter_limit() {
        assert_eq!(attach_limit(None), Ok(ATTACH_LIMIT));
        assert_eq!(attach_limit(Some("20000")), Ok(Duration::from_secs(20)));
        assert_eq!(attach_limit(Some("999999")), Ok(ATTACH_LIMIT));
        assert!(attach_limit(Some("soon")).is_err());
    }

    #[test]
    fn an_attach_dropped_without_an_answer_says_so() {
        let (ctl, requests) = mpsc::channel();
        let wake: Wake = Arc::new(|| {});
        // The node takes the request and drops its reply channel, as a panicking attach does.
        let node = thread::spawn(move || drop(requests.recv()));
        let answer = attach(&ctl, &wake, "v", proof(), Duration::from_secs(5));
        node.join().unwrap();
        assert!(answer.contains("dropped the attach"), "{answer}");
    }

    /// The CSI node plugin's limit, read from its source: the data plane's answers must come
    /// before it gives up, or kubelet sees no cause (docs/issues/6). Takes `N * time.Second` or
    /// `N * time.Minute`, in a const declaration or block, with or without a comment.
    fn csi_control_timeout() -> Duration {
        let go = include_str!("../../../../control/csi/dataplane.go");
        let value = go
            .lines()
            .map(|l| l.split("//").next().unwrap_or_default().trim())
            .find_map(|l| {
                let rest = l
                    .trim_start_matches("const ")
                    .strip_prefix("DefaultControlTimeout")?;
                Some(rest.trim_start().strip_prefix('=')?.trim().to_string())
            })
            .expect("DefaultControlTimeout in control/csi/dataplane.go");
        let (n, unit) = value
            .split_once('*')
            .map(|(n, u)| (n.trim(), u.trim()))
            .expect("DefaultControlTimeout as N * time.Unit: update this parser");
        let n: u64 = n.parse().expect("DefaultControlTimeout's N");
        match unit {
            "time.Second" => Duration::from_secs(n),
            "time.Minute" => Duration::from_secs(60 * n),
            other => panic!("DefaultControlTimeout in {other}: update this parser"),
        }
    }

    #[test]
    fn the_limits_are_their_steps_and_below_the_csi_limit() {
        assert_eq!(
            ATTACH_LIMIT,
            METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT
        );
        let csi = csi_control_timeout();
        assert!(
            ATTACH_LIMIT < csi,
            "{ATTACH_LIMIT:?} against the CSI's {csi:?}"
        );
        assert!(
            DELETE_LIMIT < csi,
            "{DELETE_LIMIT:?} against the CSI's {csi:?}"
        );
        assert_eq!(seconds(ATTACH_LIMIT), "50 s");
    }
}
