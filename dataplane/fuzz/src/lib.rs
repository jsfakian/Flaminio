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
use std::sync::LazyLock;

const NONCE: u64 = 1;

// What every input shares is built once, not per input.
static PAGES_LAYOUT: LazyLock<Layout> =
    LazyLock::new(|| Layout::compute(4 * GIB, LayoutParams::default()).unwrap());
static JOURNAL_LAYOUT: LazyLock<Layout> = LazyLock::new(|| {
    Layout::compute(
        GIB,
        LayoutParams {
            journal_size: Some(4 * MIB),
            ..Default::default()
        },
    )
    .unwrap()
});
static TOKEN_KEY: LazyLock<TokenKey> = LazyLock::new(|| TokenKey::from_raw(&[7; 32]).expect("key"));

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
/// The first byte picks the decoder from eight fixed slots (`selector % 8`): a new decoder takes a
/// free slot, so a committed regression input keeps reaching the decoder it was saved for. The free
/// slots, 6 and 7, do nothing.
pub fn metadata_pages(data: &[u8]) {
    type Decode = fn(&[u8], u64, &Layout);
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    // One table from slot to page type and decoder, so the two cannot get out of step.
    let (page_type, decode): (PageType, Decode) = match selector % 8 {
        0 => (PageType::Checkpoint, |b, offset, layout| {
            drop(CheckpointHeader::decode(b, NONCE, offset, layout))
        }),
        1 => (PageType::DataBitmap, |b, offset, _| {
            drop(BitmapPage::decode(b, BitmapKind::Data, NONCE, offset, 0))
        }),
        2 => (PageType::ChunkHeader, |b, offset, layout| {
            drop(ChunkHeader::decode(b, NONCE, offset, layout))
        }),
        3 => (PageType::PageTable, |b, offset, layout| {
            drop(PageTable::decode(b, NONCE, offset, layout, 0, 0))
        }),
        4 => (PageType::Map, |b, offset, layout| {
            drop(MapPage::decode(b, NONCE, offset, layout, 0, 0))
        }),
        5 => (PageType::PoolBitmap, |b, offset, _| {
            drop(BitmapPage::decode(b, BitmapKind::Pool, NONCE, offset, 0))
        }),
        _ => return,
    };
    let layout = &*PAGES_LAYOUT;
    let offset = layout.pool_pair_offset(0);
    let mut page = new_page();
    let n = rest.len().min(PAGE_PAYLOAD_LEN);
    payload_mut(&mut page)[..n].copy_from_slice(&rest[..n]);
    seal(&mut page, page_type, NONCE, 1, offset);
    decode(&page[..], offset, layout);
}

/// Journal replay over a fuzzer-controlled journal region. The first block is repaired into a
/// valid batch header for sequence 1, so mutations reach payload and record parsing.
pub fn journal(data: &[u8]) {
    let layout = &*JOURNAL_LAYOUT;
    // A fresh, zeroed image for every input: no input can leak into the next one.
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
    let scan = JournalScanner::new(&dev, layout, NONCE, JournalPosition { offset: 0, seq: 1 });
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
    let _ = TOKEN_KEY.verify(data);
}

/// Replays every committed regression input (docs/4/spec.md, S4 and S6).
#[cfg(test)]
mod regressions {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    type Harness = fn(&[u8]);

    /// Every harness, by target name: the name of its function, so the two cannot be mismatched.
    macro_rules! targets {
        ($($f:ident),* $(,)?) => { &[$((stringify!($f), super::$f as Harness)),*] };
    }
    const TARGETS: &[(&str, Harness)] = targets![
        superblock,
        metadata_pages,
        journal,
        repl_frame,
        capability_token,
    ];
    /// Time and stack for one input: stricter than the fuzzer's (10 s, `-timeout=10` in
    /// scripts/dev/fuzz.sh, and an 8 MiB main thread) on purpose. This build runs faster and with
    /// smaller frames than the fuzzer's instrumented one, so equal limits would let an input that
    /// times out or overflows there pass here. 5 s keeps a margin for a busy shared runner.
    const LIMIT: Duration = Duration::from_secs(5);
    const STACK: usize = 2 << 20;

    fn path(sub: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(sub)
    }

    /// The entries of `dir`, sorted.
    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.expect("directory entry").path())
            .collect();
        paths.sort();
        paths
    }

    /// The target names that `keep` finds among the entries of `dir`. A name that is not UTF-8 is
    /// kept, lossily, so that it shows up as a mismatch instead of vanishing.
    fn names(dir: &Path, keep: impl Fn(&Path) -> Option<&OsStr>) -> BTreeSet<String> {
        entries(dir)
            .iter()
            .filter_map(|p| keep(p).map(|n| n.to_string_lossy().into_owned()))
            .collect()
    }

    /// The `name` of every `[[bin]]` in the fuzz crate's Cargo.toml: what cargo-fuzz builds. Each
    /// must require the `fuzzing` feature, without which it cannot build (libfuzzer-sys is
    /// optional).
    fn bins() -> BTreeSet<String> {
        let manifest: toml::Table = std::fs::read_to_string(path("Cargo.toml"))
            .expect("Cargo.toml")
            .parse()
            .expect("Cargo.toml is TOML");
        manifest
            .get("bin")
            .and_then(|b| b.as_array())
            .into_iter()
            .flatten()
            .map(|b| {
                let name = b.get("name").and_then(|n| n.as_str());
                let name = name.expect("every [[bin]] has a name");
                let features = b.get("required-features").and_then(|f| f.as_array());
                assert!(
                    features.is_some_and(|f| f.iter().any(|v| v.as_str() == Some("fuzzing"))),
                    "[[bin]] {name}: needs required-features = [\"fuzzing\"]"
                );
                name.to_string()
            })
            .collect()
    }

    /// Whether `name` is the name of a regression input (docs/4/spec.md, S3): `empty`,
    /// `seed-<what>`, or libFuzzer's `<kind>-<sha1>` (`minimized-from-` from `cargo fuzz tmin`).
    /// libFuzzer loads every file of the directory, so a file named otherwise (a stray
    /// `.DS_Store`, an editor backup) would be replayed by the fuzzer and must not be ignored here.
    fn is_input_name(name: &str) -> bool {
        let sha1 = |h: &str| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit());
        name == "empty"
            || name.strip_prefix("seed-").is_some_and(|w| !w.is_empty())
            || ["crash-", "timeout-", "oom-", "leak-", "minimized-from-"]
                .iter()
                .any(|kind| name.strip_prefix(kind).is_some_and(sha1))
    }

    #[test]
    fn every_target_has_a_harness_and_a_regression_directory() {
        let harnesses: BTreeSet<String> = TARGETS.iter().map(|(n, _)| n.to_string()).collect();
        let sources = names(&path("fuzz_targets"), |p| {
            (p.extension()? == "rs").then(|| p.file_stem())?
        });
        let regressions = names(&path("regressions"), |p| {
            p.is_dir().then(|| p.file_name())?
        });
        assert_eq!(
            harnesses,
            bins(),
            "TARGETS against the [[bin]] entries of Cargo.toml"
        );
        assert_eq!(harnesses, sources, "TARGETS against fuzz_targets/*.rs");
        assert_eq!(harnesses, regressions, "TARGETS against regressions/*/");
        // Nothing but target directories in regressions/: an input left there directly would be
        // read by neither the replay nor the fuzzer. Hidden files (a Finder .DS_Store) are read
        // by nothing at this level and are let be.
        for entry in entries(&path("regressions")) {
            if entry
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            assert!(
                entry.is_dir(),
                "{}: not in a target's directory, so never replayed",
                entry.display()
            );
        }
        // And each binary runs its own harness, so the fuzzer and the replay agree (S2): its one
        // `fuzz_target!` line calls `flaminio_fuzz::<name>` and nothing else.
        for name in &harnesses {
            let source = path("fuzz_targets").join(format!("{name}.rs"));
            let text = std::fs::read_to_string(&source).expect("fuzz target source");
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| l.starts_with("fuzz_target!"))
                .collect();
            let call = format!("flaminio_fuzz::{name}(");
            assert!(
                lines.len() == 1
                    && lines[0].contains(&call)
                    && lines[0].matches("flaminio_fuzz::").count() == 1,
                "{}: its fuzz_target! must call {call}..) and nothing else",
                source.display()
            );
        }
    }

    #[test]
    fn regression_inputs_pass() {
        let inputs: Vec<(Harness, PathBuf)> = TARGETS
            .iter()
            .flat_map(|&(name, harness)| {
                entries(&path("regressions").join(name))
                    .into_iter()
                    .map(move |f| (harness, f))
            })
            .collect();
        let mut failed = Vec::new();
        let (inputs, strays): (Vec<_>, Vec<_>) = inputs.into_iter().partition(|(_, f)| {
            f.is_file() && is_input_name(&f.file_name().unwrap_or_default().to_string_lossy())
        });
        for (_, stray) in &strays {
            failed.push(format!(
                "{}: not a regression input's name (docs/ci.md, Fuzzing): rename or remove it",
                stray.display()
            ));
        }
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
