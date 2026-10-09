//! The node engine of Flaminio's data plane.
//!
//! - [`replica::ReplicaService`] serves replication sessions from volume targets: the replica
//!   rules of `flaminio_repl` (fencing, sessions, uncertain sets, resync filters) in front of
//!   the replica store.
//! - [`server`] puts it on the network: a network thread for TLS sessions, and an engine loop.
//!
//! Volume targets come next (phase 1 step 4d).
//!
//! The engine is single-threaded by design, like the core it runs: one thread owns the store
//! and every protocol state machine, and network threads hand it decoded frames.

pub mod replica;
pub mod server;

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
