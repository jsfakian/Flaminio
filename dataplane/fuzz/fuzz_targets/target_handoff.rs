#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| flaminio_fuzz::target_handoff(data));
