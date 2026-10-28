//! Journal replay over a fuzzer-controlled journal region. The first block is repaired into a
//! valid batch header for sequence 1, so mutations reach payload and record parsing.
//! The harness is `flaminio_fuzz::journal`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::journal(data));
