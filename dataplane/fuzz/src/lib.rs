//! The fuzz harnesses, one function per target (docs/4/design.md). Each `fuzz_targets/<target>.rs`
//! only calls its function, and the regression test below replays the committed inputs of
//! `regressions/<target>/` through the same function, on the stable toolchain.
//!
//! Nothing here may refer to `libfuzzer_sys`: that keeps libFuzzer out of the test binary.

use flaminio_format::checksum::crc32c;
use flaminio_format::consts::{BATCH_MAGIC, BLOCK_SIZE, GIB, MIB, SUPERBLOCK_MAGIC};
use flaminio_format::journal::{JournalPosition, JournalScanner};
use flaminio_format::layout::{Layout, LayoutParams};
use flaminio_format::meta::{
    BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, MapPage, PageTable,
};
use flaminio_format::page::{PAGE_PAYLOAD_LEN, PageType, new_page, payload_mut, seal};
use flaminio_format::superblock::Superblock;
use flaminio_net::frame::Frame;
use flaminio_net::token::TokenKey;

const NONCE: u64 = 1;

/// Superblock decoding. The input is used as a superblock with a repaired magic and CRC, so
/// mutations reach the field validation instead of dying at the checksum.
pub fn superblock(data: &[u8]) {
    let mut b = vec![0u8; BLOCK_SIZE];
    let n = data.len().min(BLOCK_SIZE);
    b[..n].copy_from_slice(&data[..n]);
    let _ = Superblock::decode(&b);
    b[0..8].copy_from_slice(&SUPERBLOCK_MAGIC);
    let crc = crc32c(&b[..BLOCK_SIZE - 4]);
    b[BLOCK_SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
    let _ = Superblock::decode(&b);
}

/// Every metadata page decoder, with fuzzer-controlled payloads behind a valid header and CRC.
pub fn metadata_pages(data: &[u8]) {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
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
}

/// Journal replay over a fuzzer-controlled journal region. The first block is repaired into a
/// valid batch header for sequence 1, so mutations reach payload and record parsing.
pub fn journal(data: &[u8]) {
    let layout = Layout::compute(
        GIB,
        LayoutParams {
            journal_size: Some(4 * MIB),
            ..Default::default()
        },
    )
    .unwrap();
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
    for batch in scan.flatten() {
        for record in batch.records() {
            let _ = record;
        }
    }
}

/// Replication frames come from the network: decoding must never panic, and whatever decodes
/// must encode back to the same bytes (except a refusal's reason, which is decoded lossily).
pub fn repl_frame(data: &[u8]) {
    if let Ok(f) = Frame::decode(data)
        && !matches!(f, Frame::Refused { .. })
    {
        let b = f.encode();
        assert_eq!(&b[4..], data);
    }
}

/// Capability tokens come from the network: verification must never panic.
pub fn capability_token(data: &[u8]) {
    let key = TokenKey::from_raw(&[7; 32]).expect("key");
    let _ = key.verify(data);
}

/// Replays every committed regression input (docs/4/spec.md, S4 and S6).
#[cfg(test)]
mod regressions {
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    type Harness = fn(&[u8]);

    /// Every harness, by target name.
    const TARGETS: &[(&str, Harness)] = &[
        ("superblock", super::superblock),
        ("metadata_pages", super::metadata_pages),
        ("journal", super::journal),
        ("repl_frame", super::repl_frame),
        ("capability_token", super::capability_token),
    ];
    /// The fuzzer's timeout for one input (`-timeout=10` in scripts/dev/fuzz.sh).
    const LIMIT: Duration = Duration::from_secs(10);
    /// The stack of libFuzzer's main thread, where the fuzzer runs a harness.
    const STACK: usize = 8 << 20;

    fn path(sub: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(sub)
    }

    /// The names in `dir` that `keep` maps to a target name.
    fn names(dir: &Path, keep: impl Fn(&Path) -> Option<String>) -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.expect("directory entry").path())
            .filter_map(|p| keep(&p))
            .collect()
    }

    #[test]
    fn every_target_has_a_harness_and_a_regression_directory() {
        let harnesses: BTreeSet<String> = TARGETS.iter().map(|(n, _)| n.to_string()).collect();
        let targets = names(&path("fuzz_targets"), |p| {
            (p.extension()? == "rs").then(|| p.file_stem()?.to_str().map(String::from))?
        });
        let regressions = names(&path("regressions"), |p| {
            p.is_dir()
                .then(|| p.file_name()?.to_str().map(String::from))?
        });
        assert_eq!(harnesses, targets, "TARGETS against fuzz_targets/*.rs");
        assert_eq!(harnesses, regressions, "TARGETS against regressions/*/");
    }

    #[test]
    fn regression_inputs_pass() {
        let inputs: Vec<(Harness, PathBuf)> = TARGETS
            .iter()
            .flat_map(|&(name, harness)| {
                let mut files: Vec<PathBuf> = names(&path("regressions").join(name), |p| {
                    p.is_file().then(|| p.to_str().map(String::from))?
                })
                .into_iter()
                .map(PathBuf::from)
                .collect();
                files.sort();
                files.into_iter().map(move |f| (harness, f))
            })
            .collect();
        let mut failed = Vec::new();
        for (i, (harness, file)) in inputs.iter().enumerate() {
            // Not captured by the test harness, so the log names the input even if it aborts.
            let _ =
                std::io::stderr().write_all(format!("replaying {}\n", file.display()).as_bytes());
            let data = std::fs::read(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let (done, result) = mpsc::channel();
            let harness = *harness;
            std::thread::Builder::new()
                .stack_size(STACK)
                .spawn(move || {
                    let ok = std::panic::catch_unwind(|| harness(&data)).is_ok();
                    let _ = done.send(ok);
                })
                .expect("spawn the replay thread");
            match result.recv_timeout(LIMIT) {
                Ok(true) => {}
                Ok(false) => failed.push(format!("{}: panicked", file.display())),
                Err(RecvTimeoutError::Disconnected) => {
                    failed.push(format!("{}: ended without a result", file.display()))
                }
                Err(RecvTimeoutError::Timeout) => {
                    // The thread cannot be stopped: stop here, so at most one keeps running.
                    failed.push(format!("{}: still running after {LIMIT:?}", file.display()));
                    for (_, rest) in &inputs[i + 1..] {
                        failed.push(format!("{}: not run", rest.display()));
                    }
                    break;
                }
            }
        }
        assert!(
            failed.is_empty(),
            "regression inputs failed:\n{}",
            failed.join("\n")
        );
    }
}
