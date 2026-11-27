//! Replication frames come from the network: decoding must never panic. Valid frames round-trip
//! canonically; structurally decoded Hello can still fail bounded semantic validation.
//! The harness is `flaminio_fuzz::repl_frame`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::repl_frame(data));
