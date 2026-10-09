//! The replication protocol core of Flaminio, without I/O.
//!
//! Three state machines implement the rules in `docs/replication-protocol.md`:
//!
//! - [`meta::Metadata`]: the metadata service's transitions (attach, conditional ISR changes);
//! - [`replica::Replica`]: a node that holds chunk replicas (fencing, sessions, uncertain sets,
//!   resync filters);
//! - [`target::Target`]: the per-volume I/O coordinator (write fan-out, ISR shrink before
//!   acknowledging, leases, fencing and reconciliation after failover, resync and rejoin).
//!
//! None of them does I/O or reads a clock: they take the local time as an argument and return
//! what to send. [`sim`] drives them through crashes, partitions, delays and clock drift, and
//! checks every run for linearizability, convergence and liveness.
//!
//! Blocks hold a [`types::Value`] here, standing in for 4 KiB of data. The data plane maps the
//! same decisions onto real I/O.

pub mod meta;
pub mod msg;
pub mod replica;
pub mod sim;
pub mod target;
pub mod types;
