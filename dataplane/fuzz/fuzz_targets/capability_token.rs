//! Capability tokens come from the network: verification must never panic.
//! The harness is `flaminio_fuzz::capability_token`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::capability_token(data));
