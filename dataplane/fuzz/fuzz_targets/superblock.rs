//! Superblock decoding. The fuzzer input is used as a superblock with a repaired magic and CRC,
//! so mutations reach the field validation instead of dying at the checksum.
//! The harness is `flaminio_fuzz::superblock`, which the regression test replays too.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| flaminio_fuzz::superblock(data));
