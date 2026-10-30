//! The node's control socket: attach and detach volumes while the node runs. One command per
//! connection, one line each way:
//!
//! ```text
//! attach VOLUME TOKEN-BASE64 [MS] ->  ok /dev/ublkbN   (or: error ...)
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
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

pub fn spawn(
    path: &Path,
    ctl: mpsc::Sender<Control>,
    engine: mpsc::Sender<Input>,
    wake: Wake,
    attached: Attached,
) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    thread::Builder::new()
        .name("flaminio-control".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let (ctl, engine, wake, attached) =
                    (ctl.clone(), engine.clone(), wake.clone(), attached.clone());
                // Attaches take a while; one thread per command keeps a detach possible.
                thread::spawn(move || serve(conn, &ctl, &engine, &wake, &attached));
            }
        })
        .map_err(|e| format!("control thread: {e}"))?;
    Ok(())
}

fn serve(
    conn: UnixStream,
    ctl: &mpsc::Sender<Control>,
    engine: &mpsc::Sender<Input>,
    wake: &Wake,
    attached: &Attached,
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
    let answer = match words.as_slice() {
        ["attach", volume, token, within @ ..] if within.len() <= 1 => {
            // Not in the match below: a guard there would be held through the attach, and the
            // device's ready callback takes this lock.
            let current = attached.lock().expect("attached").clone();
            match (
                base64_decode(token),
                attach_limit(within.first().copied()),
                current,
            ) {
                // The main thread takes commands only while no volume is attached.
                (_, _, Some((v, _))) => {
                    format!("error: {v} is attached here; one volume at a time")
                }
                (None, _, None) => "error: the token is not base64".into(),
                (_, Err(e), None) => format!("error: {e}"),
                (Some(token), Ok(limit), None) => attach(ctl, wake, volume, token, limit),
            }
        }
        ["detach", volume] => detach(engine, wake, attached, volume),
        ["delete", volume] => delete(engine, wake, volume),
        ["status"] => match attached.lock().expect("attached").clone() {
            Some((v, id)) => format!("ok {v} /dev/ublkb{id}"),
            None => "ok none".into(),
        },
        _ => {
            "error: usage: attach VOLUME TOKEN-BASE64 [MS] | detach VOLUME | delete VOLUME | status"
                .into()
        }
    };
    let mut conn = conn;
    let _ = writeln!(conn, "{answer}");
}

fn attach(
    ctl: &mpsc::Sender<Control>,
    wake: &Wake,
    volume: &str,
    token: Vec<u8>,
    limit: Duration,
) -> String {
    let (reply, answer) = mpsc::channel();
    if ctl
        .send(Control::Attach {
            volume: volume.to_string(),
            token,
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
        Some((v, id)) if v == volume => id,
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
    fn an_attach_without_an_answer_times_out_at_its_deadline() {
        let (ctl, requests) = mpsc::channel();
        let wake: Wake = Arc::new(|| {});
        let sent = Instant::now();
        let answer = attach(&ctl, &wake, "v", vec![1], Duration::from_millis(100));
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
        let answer = attach(&ctl, &wake, "v", vec![1], Duration::from_secs(5));
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
