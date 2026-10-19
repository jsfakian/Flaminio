//! Power loss at a random point of a random workload, optionally a second power loss during
//! recovery, then recovery. Checks the store's durability contract:
//!
//! - every acknowledged operation survives;
//! - the operation in flight at the crash is all-or-nothing;
//! - metadata is consistent (no leaked or doubly used space) and every block passes its checksum.

mod common;

use common::*;
use std::collections::BTreeMap;

use flaminio_store::{Allocation, MemDevice, Store, StoreConfig, StoreError, WriteReq};
use proptest::prelude::*;

const CHUNKS: u64 = 2;
const BLOCKS: u64 = CHUNK / BLOCK as u64;
const CLUSTER_BLOCKS: u64 = 16;
const CLUSTERS: u64 = BLOCKS / CLUSTER_BLOCKS;

#[derive(Debug, Clone)]
enum Op {
    Write {
        chunk: u64,
        block: u64,
        len: u64,
    },
    /// Writes to distinct clusters, committed as one group, so all-or-nothing together.
    /// Items: cluster -> (first block within the cluster, block count).
    WriteBatch {
        chunk: u64,
        items: BTreeMap<u64, (u64, u64)>,
    },
    Discard {
        chunk: u64,
        cluster: u64,
        count: u64,
    },
    Snapshot {
        chunk: u64,
    },
    Checkpoint,
    Delete {
        chunk: u64,
    },
    Create {
        chunk: u64,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        10 => (0..CHUNKS, 0..BLOCKS, 1u64..48)
            .prop_map(|(chunk, block, len)| Op::Write { chunk, block, len: len.min(BLOCKS - block) }),
        3 => (0..CHUNKS, prop::collection::btree_map(0..CLUSTERS, (0..CLUSTER_BLOCKS, 1..=CLUSTER_BLOCKS), 1..6))
            .prop_map(|(chunk, items)| Op::WriteBatch {
                chunk,
                items: items.into_iter().map(|(c, (off, len))| (c, (off, len.min(CLUSTER_BLOCKS - off)))).collect(),
            }),
        1 => (0..CHUNKS, 0..CLUSTERS, 1u64..4)
            .prop_map(|(chunk, cluster, count)| Op::Discard { chunk, cluster, count: count.min(CLUSTERS - cluster) }),
        1 => (0..CHUNKS).prop_map(|chunk| Op::Snapshot { chunk }),
        1 => Just(Op::Checkpoint),
        1 => (0..CHUNKS).prop_map(|chunk| Op::Delete { chunk }),
        1 => (0..CHUNKS).prop_map(|chunk| Op::Create { chunk }),
    ]
}

/// Per chunk: `None` if it does not exist, otherwise the write id visible in each block
/// (0 = zeros).
type Model = Vec<Option<Vec<u32>>>;

fn content(id: u32, chunk: u64, block: u64) -> Vec<u8> {
    if id == 0 {
        vec![0; BLOCK]
    } else {
        common::block(id, chunk * BLOCKS + block)
    }
}

/// Runs `op` against the store. Returns whether it was performed (ops on missing chunks are
/// skipped).
fn run(store: &mut Store<&MemDevice>, model: &Model, op: &Op, id: u32) -> Result<bool, StoreError> {
    let exists = |c: u64| model[c as usize].is_some();
    match *op {
        Op::Write { chunk, block, len } if exists(chunk) => {
            let data: Vec<u8> = (block..block + len)
                .flat_map(|b| content(id, chunk, b))
                .collect();
            store.write(key(chunk), block * BLOCK as u64, &data, u64::from(id), 1)?;
        }
        Op::WriteBatch { chunk, ref items } if exists(chunk) => {
            let data: Vec<Vec<u8>> = batch_blocks(items)
                .map(|(first, len)| {
                    (first..first + len)
                        .flat_map(|b| content(id, chunk, b))
                        .collect()
                })
                .collect();
            let reqs: Vec<WriteReq<'_>> = batch_blocks(items)
                .zip(&data)
                .map(|((first, _), d)| WriteReq {
                    key: key(chunk),
                    offset: first * BLOCK as u64,
                    data: d.into(),
                    write_seq: u64::from(id),
                    epoch: 1,
                })
                .collect();
            for result in store.write_batch(&reqs)? {
                result?;
            }
        }
        Op::Discard {
            chunk,
            cluster,
            count,
        } if exists(chunk) => {
            let cs = CLUSTER_BLOCKS * BLOCK as u64;
            store.discard(key(chunk), cluster * cs, count * cs, 1)?;
        }
        Op::Snapshot { chunk } if exists(chunk) => {
            let info = store.chunk_info(key(chunk)).expect("model says it exists");
            if info.layers >= 60 {
                return Ok(false);
            }
            store.snapshot(key(chunk), info.top_snapshot_gen + 1, 1)?;
        }
        Op::Checkpoint => store.checkpoint()?,
        Op::Delete { chunk } if exists(chunk) => store.delete_chunk(key(chunk), 1)?,
        Op::Create { chunk } if !exists(chunk) => {
            store.create_chunk(key(chunk), CHUNK, CHUNK, 1)?
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// (first block in the chunk, block count) of each write in a batch.
fn batch_blocks(items: &BTreeMap<u64, (u64, u64)>) -> impl Iterator<Item = (u64, u64)> + '_ {
    items
        .iter()
        .map(|(c, (off, len))| (c * CLUSTER_BLOCKS + off, *len))
}

fn update(model: &mut Model, op: &Op, id: u32) {
    match *op {
        Op::WriteBatch { chunk, ref items } => {
            let blocks = model[chunk as usize].as_mut().expect("exists");
            for (first, len) in batch_blocks(items) {
                blocks[first as usize..(first + len) as usize].fill(id);
            }
        }
        Op::Write { chunk, block, len } => {
            let blocks = model[chunk as usize].as_mut().expect("exists");
            blocks[block as usize..(block + len) as usize].fill(id);
        }
        Op::Discard {
            chunk,
            cluster,
            count,
        } => {
            let blocks = model[chunk as usize].as_mut().expect("exists");
            blocks[(cluster * CLUSTER_BLOCKS) as usize
                ..((cluster + count) * CLUSTER_BLOCKS) as usize]
                .fill(0);
        }
        Op::Delete { chunk } => model[chunk as usize] = None,
        Op::Create { chunk } => model[chunk as usize] = Some(vec![0; BLOCKS as usize]),
        Op::Snapshot { .. } | Op::Checkpoint => {}
    }
}

/// What the in-flight operation would have made block `b` of `chunk`, if it changes it.
fn in_flight_value(inflight: Option<&(Op, u32)>, chunk: u64, b: u64) -> Option<u32> {
    match inflight? {
        (
            Op::Write {
                chunk: c,
                block,
                len,
            },
            id,
        ) if *c == chunk && (*block..block + len).contains(&b) => Some(*id),
        (Op::WriteBatch { chunk: c, items }, id)
            if *c == chunk
                && batch_blocks(items).any(|(first, len)| (first..first + len).contains(&b)) =>
        {
            Some(*id)
        }
        (
            Op::Discard {
                chunk: c,
                cluster,
                count,
            },
            _,
        ) if *c == chunk
            && (cluster * CLUSTER_BLOCKS..(cluster + count) * CLUSTER_BLOCKS).contains(&b) =>
        {
            Some(0)
        }
        _ => None,
    }
}

fn verify(
    store: &Store<&MemDevice>,
    model: &Model,
    inflight: Option<&(Op, u32)>,
) -> Result<(), TestCaseError> {
    for c in 0..CHUNKS {
        let exists = store.chunk_info(key(c)).is_some();
        let expected: Vec<u32> = match (&model[c as usize], exists) {
            (None, false) => continue,
            (Some(_), false) => {
                prop_assert!(
                    matches!(inflight, Some((Op::Delete { chunk }, _)) if *chunk == c),
                    "acknowledged chunk {c} lost"
                );
                continue;
            }
            (None, true) => {
                prop_assert!(
                    matches!(inflight, Some((Op::Create { chunk }, _)) if *chunk == c),
                    "chunk {c} appeared from nowhere"
                );
                vec![0; BLOCKS as usize]
            }
            (Some(blocks), true) => blocks.clone(),
        };
        let mut buf = vec![0u8; CHUNK as usize];
        store
            .read(key(c), 0, &mut buf, 1)
            .map_err(|e| TestCaseError::fail(format!("read chunk {c}: {e}")))?;
        let mut applied = None::<bool>;
        for b in 0..BLOCKS {
            let actual = &buf[b as usize * BLOCK..(b as usize + 1) * BLOCK];
            let old = expected[b as usize];
            let new = in_flight_value(inflight, c, b);
            let is_old = actual == content(old, c, b);
            let is_new = new.is_some_and(|n| actual == content(n, c, b));
            prop_assert!(
                is_old || is_new,
                "chunk {c} block {b}: unexpected contents (expected write {old}, in flight {new:?})"
            );
            if new.is_some() && is_old != is_new {
                // A block the in-flight operation changes, and whose old and new contents
                // differ: all such blocks must agree on whether it was applied.
                let this = is_new;
                prop_assert!(
                    applied.is_none_or(|a| a == this),
                    "chunk {c} block {b}: in-flight operation applied partially"
                );
                applied = Some(this);
            }
        }
    }
    Ok(())
}

fn open(
    dev: &MemDevice,
    (allocation, pipelined): (Allocation, bool),
) -> Result<Store<&MemDevice>, StoreError> {
    Store::open(
        dev,
        StoreConfig {
            allocation,
            pipelined_commits: pipelined,
            ..StoreConfig::default()
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn power_loss_anywhere(
        ops in prop::collection::vec(op(), 1..60),
        fail_point in 0u64..600,
        seed in any::<u64>(),
        second_crash in prop::option::of((0u64..80, any::<u64>())),
        first_fit in any::<bool>(),
        pipelined in any::<bool>(),
    ) {
        // First-fit reuses freed clusters immediately, which next-fit on a large device rarely
        // does. Both must hold up.
        let allocation = if first_fit { Allocation::FirstFit } else { Allocation::NextFit };
        // Pipelined commits change recovery (spec §6.2): the last batches' data is checked, and
        // sequence numbers skip ahead.
        let allocation = (allocation, pipelined);
        // A 4 MiB journal makes wrap-around and automatic checkpoints frequent.
        let dev = MemDevice::new(DEVICE);
        Store::format(&dev, &options(4)).unwrap();
        let mut model: Model = vec![None; CHUNKS as usize];
        let mut inflight = None;
        {
            let mut store = open(&dev, allocation).unwrap();
            for c in 0..CHUNKS {
                store.create_chunk(key(c), CHUNK, CHUNK, 1).unwrap();
                model[c as usize] = Some(vec![0; BLOCKS as usize]);
            }
            dev.fail_after(Some(fail_point));
            for (i, op) in ops.iter().enumerate() {
                let id = i as u32 + 1;
                match run(&mut store, &model, op, id) {
                    Ok(true) => update(&mut model, op, id),
                    Ok(false) => {}
                    Err(StoreError::Io(_)) => {
                        inflight = Some((op.clone(), id));
                        break;
                    }
                    Err(e) => return Err(TestCaseError::fail(format!("op {op:?} failed: {e}"))),
                }
            }
        }

        let mut image = dev.crash(seed);
        if let Some((n, seed2)) = second_crash {
            // Lose power again, this time during recovery.
            image.fail_after(Some(n));
            let _ = open(&image, allocation);
            image = image.crash(seed2);
        }

        let mut store = open(&image, allocation).map_err(|e| TestCaseError::fail(format!("recovery failed: {e}")))?;
        verify(&store, &model, inflight.as_ref())?;
        store.fsck(true).map_err(|e| TestCaseError::fail(format!("fsck: {e}")))?;

        // The recovered store must be fully usable.
        let survivor = (0..CHUNKS).find(|&c| store.chunk_info(key(c)).is_some());
        if let Some(c) = survivor {
            let data = content(9999, c, 5);
            store.write(key(c), 5 * BLOCK as u64, &data, 1_000_000, 1).unwrap();
            drop(store);
            let store = open(&image, allocation).unwrap();
            let mut buf = vec![0u8; BLOCK];
            store.read(key(c), 5 * BLOCK as u64, &mut buf, 1).unwrap();
            prop_assert_eq!(buf, data);
            store.fsck(true).map_err(|e| TestCaseError::fail(format!("fsck after reuse: {e}")))?;
        }
    }
}
