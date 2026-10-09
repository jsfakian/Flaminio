//! The node engine of Flaminio's data plane.
//!
//! - [`replica::ReplicaService`] serves replication sessions from volume targets: the replica
//!   rules of `flaminio_repl` (fencing, sessions, uncertain sets, resync filters) in front of
//!   the replica store.
//! - [`net`] is the network thread: replication sessions in both directions, over TLS.
//!
//! - [`engine`] is the thread that owns both: the replica service and the volume targets,
//!   which run the verified state machine of `flaminio_repl::target`.
//!
//! The engine is single-threaded by design, like the core it runs: one thread owns the store
//! and every protocol state machine, and network threads hand it decoded frames.

pub mod engine;
pub mod net;
pub mod replica;

use flaminio_format::Uuid;

/// The store's identifier for a volume: the first 16 bytes of SHA-256 over the volume's name.
pub fn volume_uuid(name: &str) -> Uuid {
    let d = ring::digest::digest(
        &ring::digest::SHA256,
        format!("flaminio-volume:{name}").as_bytes(),
    );
    let mut b = [0u8; 16];
    b.copy_from_slice(&d.as_ref()[..16]);
    Uuid(b)
}
