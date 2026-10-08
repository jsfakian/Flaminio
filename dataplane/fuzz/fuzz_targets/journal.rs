//! Journal replay over a fuzzer-controlled journal region. The first block is repaired into a
//! valid batch header for sequence 1, so mutations reach payload and record parsing.
#![no_main]

use flaminio_format::checksum::crc32c;
use flaminio_format::consts::{BATCH_MAGIC, GIB, MIB};
use flaminio_format::journal::{JournalPosition, JournalScanner};
use flaminio_format::layout::{Layout, LayoutParams};
use libfuzzer_sys::fuzz_target;

const NONCE: u64 = 1;

fuzz_target!(|data: &[u8]| {
    let layout = Layout::compute(GIB, LayoutParams { journal_size: Some(4 * MIB), ..Default::default() }).unwrap();
    let mut dev = vec![0u8; (layout.journal_offset + layout.journal_size) as usize];
    let j = layout.journal_offset as usize;
    let n = data.len().min(layout.journal_size as usize);
    dev[j..j + n].copy_from_slice(&data[..n]);
    if n >= 64 {
        let h = &mut dev[j..j + 64];
        h[0..4].copy_from_slice(&BATCH_MAGIC);
        h[4..6].copy_from_slice(&1u16.to_le_bytes());
        h[8..16].copy_from_slice(&NONCE.to_le_bytes());
        h[16..24].copy_from_slice(&1u64.to_le_bytes());
        h[24..32].copy_from_slice(&0u64.to_le_bytes());
        let total = u32::from_le_bytes(h[32..36].try_into().unwrap());
        let payload_len = u32::from_le_bytes(h[36..40].try_into().unwrap()) as usize;
        if (total as usize) <= n && payload_len + 64 <= total as usize {
            let crc = crc32c(&dev[j + 64..j + 64 + payload_len]);
            dev[j + 44..j + 48].copy_from_slice(&crc.to_le_bytes());
        }
        let crc = crc32c(&dev[j..j + 60]);
        dev[j + 60..j + 64].copy_from_slice(&crc.to_le_bytes());
    }
    let scan = JournalScanner::new(&dev, &layout, NONCE, JournalPosition { offset: 0, seq: 1 });
    for batch in scan {
        if let Ok(batch) = batch {
            for record in batch.records() {
                let _ = record;
            }
        }
    }
});
