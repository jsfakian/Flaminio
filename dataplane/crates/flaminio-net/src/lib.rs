//! The replication transport of Flaminio's data plane.
//!
//! - [`frame`]: the wire format of the replication protocol (`flaminio_repl::msg`), with
//!   payloads as bytes. Every decoder treats its input as untrusted.
//! - [`identity`]: SPIFFE-style workload identities, read from peer certificates.
//! - [`tls`]: TLS 1.3 configurations that require and verify client certificates.
//! - [`token`]: capability tokens issued by the metadata service (Ed25519).
//! - [`session`]: one connection carries one session: the first frame is a `Hello` whose token
//!   names the session's volume and epoch, and the node of the peer's certificate.
//!
//! The identity and token formats are those of the Go control plane (`pkg/pki`,
//! `pkg/captoken`), and the tests check the Go vectors.

pub mod frame;
pub mod identity;
pub mod session;
pub mod tls;
pub mod token;

/// Bytes per block: extents count 4 KiB blocks.
pub const BLOCK: usize = 4096;
