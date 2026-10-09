//! `flaminio-io`: the Flaminio data-plane daemon.
//!
//! - `serve` (Linux): a volume as a ublk block device, `/dev/ublkbN`, on the local replica
//!   store, optionally mirrored to a remote replica (MVP steps 4 and 5).
//! - `replica`: this disk as the remote replica of a volume.
//! - `digest`: per-chunk checksums of a volume, to compare replicas offline.
//! - `node`: a phase-1 node: replicas served over mutual TLS, and optionally a volume target
//!   (`--attach`), on the replication core and the metadata service.

mod cli;
mod node;
mod proto;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod remote;
mod replica;
#[cfg(target_os = "linux")]
mod ublk;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod volume;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some(cmd @ ("replica" | "digest")) => cli::main(cmd, &args[1..]),
        Some("node") => node::main(&args[1..]),
        #[cfg(target_os = "linux")]
        _ => ublk::main(args),
        #[cfg(not(target_os = "linux"))]
        _ => Err("serve and del need Linux (ublk); replica and digest work anywhere".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flaminio-io: {e}");
            ExitCode::FAILURE
        }
    }
}
