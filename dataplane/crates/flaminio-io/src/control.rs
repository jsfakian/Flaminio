//! The node's control socket: attach and detach volumes while the node runs. One command per
//! connection, one line each way:
//!
//! ```text
//! attach VOLUME TOKEN-BASE64   ->  ok /dev/ublkbN      (or: error ...)
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use flaminio_node::engine::Input;
use libublk::ctrl::UblkCtrl;

use crate::node::base64_decode;
use crate::ublk::{ADD_DEVICE_LIMIT, Attached, Control, METADATA_LIMIT, START_LIMIT};

type Wake = Arc<dyn Fn() + Send + Sync>;

/// How long an attach may take before the socket answers without it: the sum of its steps
/// (docs/6). The CSI node plugin waits longer (`DefaultControlTimeout` in
/// control/csi/dataplane.go), so this answer reaches it.
pub(crate) const ATTACH_LIMIT: Duration = METADATA_LIMIT
    .saturating_add(ADD_DEVICE_LIMIT)
    .saturating_add(START_LIMIT);
/// How long a delete may take: as long as an attach, below the CSI node plugin's limit.
const DELETE_LIMIT: Duration = ATTACH_LIMIT;

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
        ["attach", volume, token] => {
            // Not in the match below: a guard there would be held through the attach, and the
            // device's ready callback takes this lock.
            let current = attached.lock().expect("attached").clone();
            match (base64_decode(token), current) {
                // The main thread takes commands only while no volume is attached.
                (_, Some((v, _))) => format!("error: {v} is attached here; one volume at a time"),
                (Some(token), None) => attach(ctl, wake, volume, token, ATTACH_LIMIT),
                (None, None) => "error: the token is not base64".into(),
            }
        }
        ["detach", volume] => detach(engine, wake, attached, volume),
        ["delete", volume] => delete(engine, wake, volume),
        ["status"] => match attached.lock().expect("attached").clone() {
            Some((v, id)) => format!("ok {v} /dev/ublkb{id}"),
            None => "ok none".into(),
        },
        _ => "error: usage: attach VOLUME TOKEN-BASE64 | detach VOLUME | delete VOLUME | status"
            .into(),
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
    let withdrawn = Arc::new(AtomicBool::new(false));
    if ctl
        .send(Control::Attach {
            volume: volume.to_string(),
            token,
            reply,
            withdrawn: withdrawn.clone(),
        })
        .is_err()
    {
        return "error: the node is shutting down".into();
    }
    wake();
    answer.recv_timeout(limit).unwrap_or_else(|_| {
        // Not begun yet, it never will be; under way, it may still finish (docs/6, S2).
        withdrawn.store(true, Ordering::Relaxed);
        format!("error: no answer within {}", seconds(limit))
    })
}

/// A limit as the socket's errors name it: seconds, with at most one decimal.
fn seconds(d: Duration) -> String {
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
    fn an_attach_without_an_answer_is_withdrawn() {
        let (ctl, requests) = mpsc::channel();
        let wake: Wake = Arc::new(|| {});
        let answer = attach(&ctl, &wake, "v", vec![1], Duration::from_millis(100));
        assert_eq!(answer, "error: no answer within 0.1 s");
        // The node still holds the request, and will skip it.
        let Ok(Control::Attach { withdrawn, .. }) = requests.try_recv() else {
            panic!("no request sent");
        };
        assert!(withdrawn.load(Ordering::Relaxed));
    }

    #[test]
    fn the_attach_limit_is_its_steps_and_below_the_csi_limit() {
        assert_eq!(
            ATTACH_LIMIT,
            METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT
        );
        // `DefaultControlTimeout` in control/csi/dataplane.go: this answer must come first.
        assert!(ATTACH_LIMIT < Duration::from_secs(60));
        assert!(DELETE_LIMIT < Duration::from_secs(60));
        assert_eq!(seconds(ATTACH_LIMIT), "50 s");
    }
}
