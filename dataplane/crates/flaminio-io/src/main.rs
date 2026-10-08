//! `flaminio-io`: the Flaminio data-plane daemon.
//!
//! MVP step 4: serves one volume from the local replica store (RF=1) as a ublk block device,
//! `/dev/ublkbN`. Replication (step 5) adds remote replicas behind the same volume interface.

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod volume;

#[cfg(target_os = "linux")]
mod ublk;

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        match ublk::main(std::env::args().skip(1).collect()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("flaminio-io: {e}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("flaminio-io needs Linux (ublk)");
        ExitCode::FAILURE
    }
}
