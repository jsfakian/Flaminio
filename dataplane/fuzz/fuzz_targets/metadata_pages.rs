//! Every metadata page decoder, with fuzzer-controlled payloads behind a valid header and CRC.
#![no_main]

use flaminio_format::consts::GIB;
use flaminio_format::layout::{Layout, LayoutParams};
use flaminio_format::meta::{BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, MapPage, PageTable};
use flaminio_format::page::{PAGE_PAYLOAD_LEN, PageType, new_page, payload_mut, seal};
use libfuzzer_sys::fuzz_target;

const NONCE: u64 = 1;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
    let layout = Layout::compute(4 * GIB, LayoutParams::default()).unwrap();
    let offset = layout.pool_pair_offset(0);
    let mut page = new_page();
    let n = rest.len().min(PAGE_PAYLOAD_LEN);
    payload_mut(&mut page)[..n].copy_from_slice(&rest[..n]);
    let page_type = match selector % 6 {
        0 => PageType::Checkpoint,
        1 => PageType::DataBitmap,
        2 => PageType::ChunkHeader,
        3 => PageType::PageTable,
        _ => PageType::Map,
    };
    seal(&mut page, page_type, NONCE, 1, offset);
    let b = &page[..];
    match page_type {
        PageType::Checkpoint => drop(CheckpointHeader::decode(b, NONCE, offset, &layout)),
        PageType::DataBitmap => drop(BitmapPage::decode(b, BitmapKind::Data, NONCE, offset, 0)),
        PageType::ChunkHeader => drop(ChunkHeader::decode(b, NONCE, offset, &layout)),
        PageType::PageTable => drop(PageTable::decode(b, NONCE, offset, &layout, 0, 0)),
        _ => drop(MapPage::decode(b, NONCE, offset, &layout, 0, 0)),
    }
});
