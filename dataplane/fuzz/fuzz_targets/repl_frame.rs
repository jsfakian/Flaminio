//! Replication frames come from the network: decoding must never panic, and whatever decodes
//! must encode back to the same bytes (except a refusal's reason, which is decoded lossily).
#![no_main]

use flaminio_net::frame::Frame;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(f) = Frame::decode(data)
        && !matches!(f, Frame::Refused { .. })
    {
        let b = f.encode();
        assert_eq!(&b[4..], data);
    }
});
