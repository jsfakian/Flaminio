//! Replication frames come from the network: decoding must never panic, and whatever decodes
//! must encode back to the same bytes (except a refusal's reason, which is decoded lossily).
//! The harness is `flaminio_fuzz::repl_frame`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::repl_frame(data));
