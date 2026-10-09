//! Capability tokens come from the network: verification must never panic.
#![no_main]

use flaminio_net::token::TokenKey;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let key = TokenKey::from_raw(&[7; 32]).expect("key");
    let _ = key.verify(data);
});
