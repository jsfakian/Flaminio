//! `flaminio-io`: the Flaminio data-plane daemon.
//!
//! - `digest`: offline per-chunk checksums on quiesced storage.
//! - `clock-check`: read-only inspection of the explicit physical clock contract.
//! - `node`: qualified, authenticated replicas and targets started through a signed
//!   current-process agent/admin Attach handoff. Legacy `serve`/`replica` starts refuse.

mod cli;
use flaminio_io::clock;
#[cfg(target_os = "linux")]
mod control;
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
        Some("replica") => Err("legacy replica is unsupported; use node --clock-contract PATH for qualified authenticated storage".into()),
        Some("clock-check") => clock::main(&args[1..]),
        Some("digest") => cli::main("digest", &args[1..]),
        Some("node") => node::main(&args[1..]),
        #[cfg(target_os = "linux")]
        _ => ublk::main(args),
        #[cfg(not(target_os = "linux"))]
        _ => Err("node requires a qualified Linux host; del needs Linux (ublk); digest is offline".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flaminio-io: {e}");
            ExitCode::FAILURE
        }
    }
}
