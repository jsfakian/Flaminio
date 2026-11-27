//! Linearizability checking of single-block histories (protocol spec §9).
//!
//! Every block is a register with unique written values, checked on its own: linearizability
//! is compositional, so a history is linearizable exactly when each block's sub-history is.
//! The search follows Wing & Gong, with memoization on (linearized set, current value).

use std::collections::{BTreeMap, HashSet};

use crate::extent::Extent;
use crate::msg::{Request, Response, ToReplica};
use crate::replica::{Action, ReplicaCore, UNCERTAINTY_BUDGET};
use crate::target::FencedAuthority;
use crate::types::{Block, ChunkIdx, Epoch, Value, Version, VolumeId};

/// Independent reference state for bounded simulator inputs. Every admitted attempt keeps
/// its exact extent/version, including duplicate and unanswered writes. There is deliberately
/// no arena, coarse mode, maximum-version summary, or completed-operation history here.
#[derive(Debug, Default)]
pub(super) struct Uncertainty {
    fenced: Epoch,
    session: Option<(Epoch, u64)>,
    chunks: BTreeMap<ChunkIdx, ShadowChunk>,
    pub stats: UncertaintyEvents,
}

#[derive(Debug, Default)]
struct ShadowChunk {
    blocks: Block,
    records: Vec<(Extent, Version)>,
    restarted_at: Option<Epoch>,
}

#[derive(Debug, Default)]
pub(super) struct UncertaintyEvents {
    pub writes: u64,
    pub repairs: u64,
    pub prunes: u64,
    pub settles: u64,
}

pub(super) struct CheckedUncertainty {
    pub coarsened: bool,
    pub restarted_chunks: usize,
}

impl Uncertainty {
    pub fn add_chunk(&mut self, chunk: ChunkIdx, blocks: Block) {
        // Existing chunks retain every reason for uncertainty.
        self.chunks.entry(chunk).or_insert_with(|| ShadowChunk {
            blocks,
            ..ShadowChunk::default()
        });
    }

    pub fn remove_chunk(&mut self, chunk: ChunkIdx) {
        self.chunks.remove(&chunk);
    }

    pub fn restart(&mut self) {
        self.session = None;
        for c in self.chunks.values_mut() {
            c.records.clear();
            c.restarted_at = Some(self.fenced);
        }
    }

    fn prune(&mut self, chunk: ChunkIdx, below: Version) {
        if let Some(c) = self.chunks.get_mut(&chunk) {
            c.records.retain(|(_, version)| *version >= below);
            if c.restarted_at.is_some_and(|epoch| epoch < below.epoch) {
                c.restarted_at = None;
            }
        }
    }

    /// Admission is observed before I/O, not inferred from a successful response. Session
    /// and fence proof are tracked independently; a foreign or stale bound proves nothing.
    pub fn observe<P>(&mut self, m: &ToReplica<P>, action: &Action<P>, blocks: Block) {
        if let (Request::Hello { chunks }, Action::Fence { .. }) = (&m.body, action) {
            if m.epoch >= self.fenced {
                self.fenced = m.epoch;
                if self.session.is_none_or(|old| old < (m.epoch, m.session)) {
                    self.session = Some((m.epoch, m.session));
                }
                for &chunk in chunks {
                    self.add_chunk(chunk, blocks);
                }
            }
            return;
        }
        let current = self.session == Some((m.epoch, m.session)) && m.epoch == self.fenced;
        match (&m.body, action) {
            (
                Request::Write {
                    chunk,
                    ext,
                    version,
                    prune,
                    ..
                },
                Action::Write { .. },
            ) => {
                self.stats.writes += 1;
                self.stats.repairs += u64::from(prune.is_none());
                let proof = prune.filter(|bound| current && bound.epoch == m.epoch);
                // Original exact-record semantics: retain the accepted attempt first,
                // then apply only independently valid strict completion proof.
                if let Some(c) = self.chunks.get_mut(chunk) {
                    c.records.push((*ext, *version));
                }
                if let Some(below) = proof {
                    self.stats.prunes += 1;
                    self.prune(*chunk, below);
                }
            }
            (Request::Settle { chunk, below }, Action::Reply(Response::Ok))
                if current && below.epoch == m.epoch =>
            {
                self.stats.settles += 1;
                self.prune(*chunk, *below);
            }
            (Request::BeginResync { chunk }, Action::Reply(Response::Ok)) => {
                self.add_chunk(*chunk, blocks);
            }
            _ => {}
        }
    }

    /// Check the live demand and retained allocation before constructing another report.
    pub fn check_core(
        &self,
        core: &ReplicaCore,
        vol: VolumeId,
    ) -> Result<CheckedUncertainty, String> {
        let stats = core.uncertainty_stats(vol);
        let budget = core.budget() as usize;
        if stats.chunks > budget
            || stats.records > budget.saturating_sub(stats.chunks)
            || stats.allocated_slots > budget
            || stats.records > stats.allocated_slots
        {
            return Err(format!(
                "uncertainty budget exceeded: chunks {} records {} allocated {} budget {budget}",
                stats.chunks, stats.records, stats.allocated_slots
            ));
        }
        let report = core.report(vol);
        let extents = report
            .iter()
            .map(|(_, extents)| extents.len())
            .sum::<usize>();
        if report.len() > budget || extents > budget {
            return Err(format!(
                "uncertainty budget exceeded: report rows {} extents {extents} budget {budget}",
                report.len()
            ));
        }
        self.check_report(&report)?;
        Ok(CheckedUncertainty {
            coarsened: self.chunks.values().map(|c| c.records.len()).sum::<usize>() > stats.records,
            restarted_chunks: self
                .chunks
                .values()
                .filter(|c| c.restarted_at.is_some())
                .count(),
        })
    }

    pub fn check_report(&self, report: &[(ChunkIdx, Vec<Extent>)]) -> Result<(), String> {
        if report.len() != self.chunks.len() {
            return Err("uncertainty not covered: held chunk rows differ".to_string());
        }
        for ((&chunk, expected), (reported_chunk, extents)) in self.chunks.iter().zip(report) {
            if chunk != *reported_chunk {
                return Err(format!("uncertainty not covered: chunk {chunk} has no row"));
            }
            let mut previous_end = 0;
            for extent in extents {
                let Some(end) = extent.start.checked_add(extent.len) else {
                    return Err(format!(
                        "uncertainty not covered: malformed chunk {chunk} row"
                    ));
                };
                if extent.len == 0 || extent.start < previous_end || end > expected.blocks {
                    return Err(format!(
                        "uncertainty not covered: malformed chunk {chunk} row"
                    ));
                }
                previous_end = end;
            }
            let missing = if expected.restarted_at.is_some() {
                (!covers(extents, Extent::new(0, expected.blocks))).then_some("restart")
            } else {
                expected
                    .records
                    .iter()
                    .find_map(|(extent, _)| (!covers(extents, *extent)).then_some("accepted write"))
            };
            if let Some(reason) = missing {
                return Err(format!("uncertainty not covered: chunk {chunk} {reason}"));
            }
        }
        Ok(())
    }
}

/// Interval containment, independent of the producer's ExtentSet and coarsening algorithms.
fn covers(report: &[Extent], expected: Extent) -> bool {
    let mut next = expected.start;
    for extent in report {
        if extent.start > next {
            break;
        }
        next = next.max(extent.end());
        if next >= expected.end() {
            return true;
        }
    }
    false
}

/// Validate an entire Hello answer independently of Target::valid_fenced. The required set
/// is the one captured when the pending Hello was sent, even after placement changes.
pub(super) fn valid_fenced<P>(
    body: &Response<P>,
    required: &[ChunkIdx],
    chunks: u32,
    blocks: Block,
) -> bool {
    let Response::Fenced { uncertain } = body else {
        return false;
    };
    let mut required = required.iter().copied().peekable();
    let mut previous_chunk = None;
    let mut total = 0usize;
    if uncertain.len() > UNCERTAINTY_BUDGET as usize {
        return false;
    }
    for (chunk, extents) in uncertain {
        if *chunk >= chunks || previous_chunk.is_some_and(|previous| previous >= *chunk) {
            return false;
        }
        previous_chunk = Some(*chunk);
        if required.peek().is_some_and(|needed| *needed < *chunk) {
            return false;
        }
        if required.peek() == Some(chunk) {
            required.next();
        }
        let Some(count) = total.checked_add(extents.len()) else {
            return false;
        };
        total = count;
        if total > UNCERTAINTY_BUDGET as usize {
            return false;
        }
        let mut previous_end = 0;
        for extent in extents {
            let Some(end) = extent.start.checked_add(extent.len) else {
                return false;
            };
            if extent.len == 0 || extent.start < previous_end || end > blocks {
                return false;
            }
            previous_end = end;
        }
    }
    required.next().is_none()
        && uncertain
            .len()
            .checked_add(total)
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(13))
            .is_some_and(|body| body <= 1_048_704)
}

pub(super) fn check_fenced_authority(
    before: &Option<FencedAuthority>,
    after: &Option<FencedAuthority>,
) -> Result<(), String> {
    if before != after {
        return Err(
            "invalid Fenced accepted: authority changed after invalid Hello answer".to_string(),
        );
    }
    Ok(())
}

/// One operation on a block, with real (simulator) times.
#[derive(Debug, Clone, Copy)]
pub struct COp {
    pub invoke: u64,
    /// Completion time. For an optional operation, the latest time it may take effect.
    pub ret: u64,
    pub kind: CKind,
    /// The operation may also never take effect (a write without an answer).
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CKind {
    Read(Value),
    Write(Value),
}

/// Most operations per block the checker accepts.
pub const MAX_OPS: usize = 128;

/// Checks one block's history, starting from value 0. Returns an explanation on failure.
pub fn check_register(ops: &[COp]) -> Result<(), String> {
    assert!(ops.len() <= MAX_OPS, "too many operations for one block");
    let mut ops = ops.to_vec();
    ops.sort_by_key(|o| (o.invoke, o.ret));
    let all: u128 = if ops.len() == 128 {
        u128::MAX
    } else {
        (1u128 << ops.len()) - 1
    };
    let mut seen = HashSet::new();
    let mut deepest = 0u128;
    if search(&ops, 0, 0, all, &mut seen, &mut deepest) {
        return Ok(());
    }
    // Explain from the longest prefix found: the first operation that could not be placed.
    let stuck: Vec<String> = (0..ops.len())
        .filter(|i| deepest & (1 << i) == 0)
        .take(6)
        .map(|i| format!("{:?}", ops[i]))
        .collect();
    Err(format!(
        "not linearizable; placed {} of {} operations; next unplaced: {}",
        deepest.count_ones(),
        ops.len(),
        stuck.join(", ")
    ))
}

fn search(
    ops: &[COp],
    done: u128,
    value: Value,
    all: u128,
    seen: &mut HashSet<(u128, Value)>,
    deepest: &mut u128,
) -> bool {
    if done == all {
        return true;
    }
    if !seen.insert((done, value)) {
        return false;
    }
    if done.count_ones() > deepest.count_ones() {
        *deepest = done;
    }
    // An operation can go next only if it was invoked before every other open operation
    // returned.
    let open = |i: usize| done & (1 << i) == 0;
    let mut first_ret = u64::MAX;
    let mut second_ret = u64::MAX;
    let mut first_idx = usize::MAX;
    for (i, o) in ops.iter().enumerate().filter(|(i, _)| open(*i)) {
        if o.ret < first_ret {
            second_ret = first_ret;
            first_ret = o.ret;
            first_idx = i;
        } else if o.ret < second_ret {
            second_ret = o.ret;
        }
    }
    for (i, o) in ops.iter().enumerate() {
        if !open(i) {
            continue;
        }
        let others_ret = if i == first_idx {
            second_ret
        } else {
            first_ret
        };
        if o.invoke > others_ret {
            // Sorted by invocation: no later operation can go first either.
            break;
        }
        let next = done | (1 << i);
        match o.kind {
            CKind::Read(v) if v == value => {
                if search(ops, next, value, all, seen, deepest) {
                    return true;
                }
            }
            CKind::Read(_) => {}
            CKind::Write(v) => {
                if search(ops, next, v, all, seen, deepest) {
                    return true;
                }
            }
        }
        if o.optional && search(ops, next, value, all, seen, deepest) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(invoke: u64, ret: u64, kind: CKind) -> COp {
        COp {
            invoke,
            ret,
            kind,
            optional: false,
        }
    }

    fn opt(invoke: u64, deadline: u64, v: Value) -> COp {
        COp {
            invoke,
            ret: deadline,
            kind: CKind::Write(v),
            optional: true,
        }
    }

    use CKind::{Read as R, Write as W};

    #[test]
    fn sequential_histories() {
        assert!(check_register(&[op(0, 1, W(1)), op(2, 3, R(1))]).is_ok());
        assert!(check_register(&[op(0, 1, R(0)), op(2, 3, W(1)), op(4, 5, R(1))]).is_ok());
        // A stale read after a completed write.
        assert!(check_register(&[op(0, 1, W(1)), op(2, 3, R(0))]).is_err());
        // Reading a value nobody wrote.
        assert!(check_register(&[op(0, 1, R(7))]).is_err());
    }

    #[test]
    fn concurrent_operations_may_go_either_way() {
        // A read overlapping a write may see either value.
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(0))]).is_ok());
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(1))]).is_ok());
        // But once a read saw the new value, a later read cannot see the old one.
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(1)), op(4, 5, R(0))]).is_err());
    }

    #[test]
    fn optional_writes() {
        // An unanswered write may take effect...
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(1))]).is_ok());
        // ...or not...
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(0))]).is_ok());
        // ...but not both, in that order.
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(1)), op(7, 8, R(0))]).is_err());
        // And not after its deadline: here, after a newer write completed and was read.
        assert!(
            check_register(&[opt(0, 4, 1), op(2, 4, W(2)), op(5, 6, R(2)), op(7, 8, R(1))])
                .is_err()
        );
        assert!(check_register(&[opt(0, 4, 1), op(2, 4, W(2)), op(5, 6, R(2))]).is_ok());
    }
}
