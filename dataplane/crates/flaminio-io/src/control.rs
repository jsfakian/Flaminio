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
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use flaminio_node::engine::Input;
use libublk::ctrl::UblkCtrl;

use crate::node::base64_decode;
use crate::ublk::{Attached, Control};

type Wake = Arc<dyn Fn() + Send + Sync>;

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
                (Some(token), None) => attach(ctl, wake, volume, token),
                (None, None) => "error: the token is not base64".into(),
            }
        }
        ["detach", volume] => detach(engine, wake, attached, volume),
        ["status"] => match attached.lock().expect("attached").clone() {
            Some((v, id)) => format!("ok {v} /dev/ublkb{id}"),
            None => "ok none".into(),
        },
        _ => "error: usage: attach VOLUME TOKEN-BASE64 | detach VOLUME | status".into(),
    };
    let mut conn = conn;
    let _ = writeln!(conn, "{answer}");
}

fn attach(ctl: &mpsc::Sender<Control>, wake: &Wake, volume: &str, token: Vec<u8>) -> String {
    let (reply, answer) = mpsc::channel();
    if ctl
        .send(Control::Attach {
            volume: volume.to_string(),
            token,
            reply,
        })
        .is_err()
    {
        return "error: the node is shutting down".into();
    }
    wake();
    answer
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or_else(|_| "error: no answer within 120 s".into())
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
