//! Every metadata page decoder, with fuzzer-controlled payloads behind a valid header and CRC.
//! The harness is `flaminio_fuzz::metadata_pages`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::metadata_pages(data));
