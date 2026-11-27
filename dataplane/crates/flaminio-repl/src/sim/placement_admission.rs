//! Sans-I/O model of placement admission and drain completion (docs/issues/78).
//!
//! Models a global placement gate, creating-header reservations with
//! `admission_generation`, drain markers, chunk replicas / incoming moves, and a
//! CheckDrain split into check-start / header-scan / chunk-scan / final-validation.
//! Injected mutants omit the admission compare, reservation count, final marker
//! compare, or publication fence; each must produce a bounded counterexample.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const GATE: &str = "/flaminio/v1/placement-gate";

fn drain_key(node: &str) -> String {
    format!("/flaminio/v1/drain/{node}")
}
fn volume_key(id: &str) -> String {
    format!("/flaminio/v1/volumes/{id}")
}
fn chunk_key(vol: &str, i: u32) -> String {
    format!("/flaminio/v1/chunks/{vol}/{i:08}")
}
fn node_key(id: &str) -> String {
    format!("/flaminio/v1/nodes/{id}")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutant {
    #[default]
    None,
    /// Create/move claim omits the placement-gate compare.
    OmittedAdmissionCompare,
    /// CheckDrain header-scan skips creating reservations with admission_generation < D.
    OmittedReservationCount,
    /// CheckDrain final-validation skips the drain-marker compare.
    OmittedFinalMarkerCompare,
    /// A delayed copy batch omits its original creating-header comparison.
    OmittedPublicationFence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub id: String,
    pub creating: bool,
    #[serde(default)]
    pub deleting: bool,
    #[serde(default)]
    pub admission_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub replicas: Vec<String>,
    #[serde(default)]
    pub moving_to: String,
}

// A private sealed plan survives a service restart. Its public header is the reservation;
// no chunk is public until a separate copy batch compares the root and original claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreatePlan {
    gate: i64,
    chunks: Vec<Chunk>,
    sealed: bool,
    claim: i64,
    cleaned: bool,
    next: usize,
}

#[derive(Debug, Clone)]
struct PlannedMove {
    gate: i64,
    header: i64,
    chunk: i64,
    to: String,
}

#[derive(Debug, Clone, Default)]
pub struct Store {
    rev: i64,
    data: BTreeMap<String, (i64, String)>,
    planned_moves: BTreeMap<(String, u32), PlannedMove>,
    // Independent history of successful admission/publication events, never read by scans.
    // It survives model restarts and is retired only by publication or a committed fence.
    admitted: BTreeMap<String, (i64, i64)>, // volume -> (claim, admission generation)
    published: BTreeMap<String, Chunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    /// Admission ordered after mark D targeted the drained node.
    PostMarkAdmission { node: String, drain_gen: i64 },
    /// CheckDrain reported complete while work remained for D, or validated a stale D.
    PrematureOrStaleCompletion { node: String, drain_gen: i64 },
    /// A successful public copy had no live admission for its original claim.
    FencedPublication { volume: String },
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<(i64, &str)> {
        self.data.get(key).map(|(r, v)| (*r, v.as_str()))
    }

    pub fn rev(&self) -> i64 {
        self.rev
    }

    fn cmp_ok(&self, key: &str, expect: i64) -> bool {
        let cur = self.data.get(key).map(|(r, _)| *r).unwrap_or(0);
        cur == expect
    }

    // One transaction gives every changed key the same revision, as etcd and Mem do.
    fn transaction(
        &mut self,
        cmps: &[(String, i64)],
        ops: Vec<(String, Option<String>)>,
    ) -> Option<i64> {
        if !cmps.iter().all(|(key, rev)| self.cmp_ok(key, *rev)) {
            return None;
        }
        if !ops.is_empty() {
            self.rev += 1;
            for (key, value) in ops {
                if let Some(value) = value {
                    self.data.insert(key, (self.rev, value));
                } else {
                    self.data.remove(&key);
                }
            }
        }
        Some(self.rev)
    }

    fn put(&mut self, key: String, value: String) {
        self.transaction(&[], vec![(key, Some(value))]);
    }

    fn root_key(volume: &str) -> String {
        format!("/flaminio/v1/create-roots/{volume}")
    }

    fn tombstone_key(volume: &str) -> String {
        format!("/flaminio/v1/tombstones/{volume}")
    }

    fn plan(&self, volume: &str) -> Option<(i64, CreatePlan)> {
        let (rev, raw) = self.get(&Self::root_key(volume))?;
        serde_json::from_str(raw).ok().map(|plan| (rev, plan))
    }

    /// Ensure the gate exists; return its modification revision.
    pub fn ensure_gate(&mut self) -> i64 {
        if let Some((r, _)) = self.get(GATE) {
            return r;
        }
        self.put(GATE.into(), "1".into());
        self.rev
    }

    pub fn register_node(&mut self, id: &str) {
        let g = self.ensure_gate();
        if self.get(&node_key(id)).is_some() {
            return; // Unchanged registration initializes, but does not advance, the gate.
        }
        self.transaction(
            &[(node_key(id), 0), (GATE.into(), g)],
            vec![
                (node_key(id), Some("1".into())),
                (GATE.into(), Some("1".into())),
            ],
        );
    }

    pub fn mark_drain(&mut self, node: &str) -> Option<i64> {
        let g = self.ensure_gate();
        let (nr, _) = self.get(&node_key(node))?;
        if let Some((d, _)) = self.get(&drain_key(node)) {
            return Some(d);
        }
        self.transaction(
            &[(node_key(node), nr), (GATE.into(), g), (drain_key(node), 0)],
            vec![
                (drain_key(node), Some("1".into())),
                (GATE.into(), Some("1".into())),
            ],
        )
    }

    pub fn unmark_drain(&mut self, node: &str) -> bool {
        let g = self.ensure_gate();
        let Some((d, _)) = self.get(&drain_key(node)) else {
            return true;
        };
        self.transaction(
            &[(drain_key(node), d), (GATE.into(), g)],
            vec![(drain_key(node), None), (GATE.into(), Some("1".into()))],
        )
        .is_some()
    }

    /// Planning reads the gate before eligibility. The private plan admits no placement.
    pub fn plan_create(&mut self, volume: &str, nodes: &[String]) -> bool {
        self.plan_create_chunks(volume, nodes, 1)
    }

    pub fn plan_create_chunks(&mut self, volume: &str, nodes: &[String], count: usize) -> bool {
        let gate = self.ensure_gate();
        let eligible: Vec<String> = nodes
            .iter()
            .filter(|node| {
                self.get(&node_key(node)).is_some() && self.get(&drain_key(node)).is_none()
            })
            .cloned()
            .collect();
        if eligible.is_empty()
            || eligible.len() != nodes.len()
            || count == 0
            || count > 65_536
            || self.get(&volume_key(volume)).is_some()
        {
            return false;
        }
        let plan = CreatePlan {
            gate,
            chunks: vec![
                Chunk {
                    replicas: eligible,
                    moving_to: String::new(),
                };
                count
            ],
            sealed: false,
            claim: 0,
            cleaned: false,
            next: 0,
        };
        self.put(
            Self::root_key(volume),
            serde_json::to_string(&plan).unwrap_or_default(),
        );
        true
    }

    /// Bounded private staging/sealing precedes the claim; it creates no public chunks.
    pub fn stage_create(&mut self, volume: &str) -> bool {
        let Some((rev, mut plan)) = self.plan(volume) else {
            return false;
        };
        if plan.claim != 0 {
            return false;
        }
        plan.sealed = true;
        self.transaction(
            &[(Self::root_key(volume), rev)],
            vec![(
                Self::root_key(volume),
                Some(serde_json::to_string(&plan).unwrap_or_default()),
            )],
        )
        .is_some()
    }

    pub fn claim_create(
        &mut self,
        volume: &str,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let Some((root_rev, mut plan)) = self.plan(volume) else {
            return Ok(false);
        };
        if !plan.sealed || plan.claim != 0 {
            return Ok(false);
        }
        let mut cmps = vec![
            (Self::root_key(volume), root_rev),
            (volume_key(volume), 0),
            (Self::tombstone_key(volume), 0),
        ];
        if mutant != Mutant::OmittedAdmissionCompare {
            cmps.push((GATE.into(), plan.gate));
        }
        let h = Header {
            id: volume.into(),
            creating: true,
            deleting: false,
            admission_generation: plan.gate,
        };
        plan.claim = self.rev + 1;
        if self
            .transaction(
                &cmps,
                vec![
                    (
                        volume_key(volume),
                        Some(serde_json::to_string(&h).unwrap_or_default()),
                    ),
                    (
                        Self::root_key(volume),
                        Some(serde_json::to_string(&plan).unwrap_or_default()),
                    ),
                ],
            )
            .is_none()
        {
            return Ok(false);
        }
        self.admitted.insert(volume.into(), (plan.claim, plan.gate));
        for node in plan.chunks.iter().flat_map(|chunk| &chunk.replicas) {
            if let Some((d, _)) = self.get(&drain_key(node)) {
                return Err(InvariantViolation::PostMarkAdmission {
                    node: node.clone(),
                    drain_gen: d,
                });
            }
        }
        Ok(true)
    }

    pub fn admit_create(
        &mut self,
        volume: &str,
        nodes: &[String],
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        if !self.plan_create(volume, nodes) || !self.stage_create(volume) {
            return Ok(false);
        }
        self.claim_create(volume, mutant)
    }

    /// A cleaning batch removes old public keys under the original claim and root guards.
    pub fn clean_batch(&mut self, volume: &str) -> bool {
        let Some((rev, mut plan)) = self.plan(volume) else {
            return false;
        };
        if plan.claim == 0 || plan.cleaned {
            return false;
        }
        let prefix = format!("/flaminio/v1/chunks/{volume}/");
        let mut ops: Vec<_> = self
            .data
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .take(32)
            .map(|key| (key.clone(), None))
            .collect();
        // The real cleaner changes to copying only after a separate empty-prefix observation.
        plan.cleaned = ops.is_empty();
        let removed: Vec<_> = ops.iter().map(|(key, _)| key.clone()).collect();
        ops.push((
            Self::root_key(volume),
            Some(serde_json::to_string(&plan).unwrap_or_default()),
        ));
        let applied = self
            .transaction(
                &[
                    (volume_key(volume), plan.claim),
                    (Self::root_key(volume), rev),
                ],
                ops,
            )
            .is_some();
        if applied {
            for key in removed {
                self.published.remove(&key);
            }
        }
        applied
    }

    /// One bounded public-copy batch compares both claim and root, preserving admission.
    pub fn copy_batch(&mut self, volume: &str, mutant: Mutant) -> Result<bool, InvariantViolation> {
        let Some((rev, mut plan)) = self.plan(volume) else {
            return Ok(false);
        };
        if plan.claim == 0 || !plan.cleaned {
            return Ok(false);
        }
        let start = plan.next;
        let end = (plan.next + 32).min(plan.chunks.len());
        let mut ops: Vec<_> = (plan.next..end)
            .map(|i| {
                (
                    chunk_key(volume, i as u32),
                    Some(serde_json::to_string(&plan.chunks[i]).unwrap_or_default()),
                )
            })
            .collect();
        plan.next = end;
        ops.push((
            Self::root_key(volume),
            Some(serde_json::to_string(&plan).unwrap_or_default()),
        ));
        let mut cmps = vec![(Self::root_key(volume), rev)];
        if mutant != Mutant::OmittedPublicationFence {
            cmps.push((volume_key(volume), plan.claim));
        }
        if self.transaction(&cmps, ops).is_none() {
            return Ok(false);
        }
        if !matches!(self.admitted.get(volume), Some((claim, _)) if *claim == plan.claim) {
            return Err(InvariantViolation::FencedPublication {
                volume: volume.into(),
            });
        }
        for i in start..end {
            self.published
                .insert(chunk_key(volume, i as u32), plan.chunks[i].clone());
        }
        Ok(true)
    }

    pub fn publish(&mut self, volume: &str) -> bool {
        let Some((rev, plan)) = self.plan(volume) else {
            return false;
        };
        if plan.claim == 0 || plan.next != plan.chunks.len() {
            return false;
        }
        let Some((_, raw)) = self.get(&volume_key(volume)) else {
            return false;
        };
        let Ok(mut h) = serde_json::from_str::<Header>(raw) else {
            return false;
        };
        if !h.creating || h.deleting {
            return false;
        }
        h.creating = false;
        let applied = self
            .transaction(
                &[
                    (volume_key(volume), plan.claim),
                    (Self::root_key(volume), rev),
                ],
                vec![
                    (
                        volume_key(volume),
                        Some(serde_json::to_string(&h).unwrap_or_default()),
                    ),
                    (
                        Self::root_key(volume),
                        Some(serde_json::to_string(&plan).unwrap_or_default()),
                    ),
                ],
            )
            .is_some();
        if applied {
            self.admitted.remove(volume);
        }
        applied
    }

    /// #76 rolls an unpublished claim back with a delete-only header CAS. Visible deletion
    /// first marks its header. Either fence invalidates every delayed claim/root helper.
    pub fn delete_fence(&mut self, volume: &str) -> bool {
        let Some((rev, raw)) = self.get(&volume_key(volume)) else {
            return false;
        };
        let Ok(mut h) = serde_json::from_str::<Header>(raw) else {
            return false;
        };
        if h.creating {
            let applied = self
                .transaction(
                    &[(volume_key(volume), rev), (Self::tombstone_key(volume), 0)],
                    vec![(volume_key(volume), None)],
                )
                .is_some();
            if applied {
                self.admitted.remove(volume);
            }
            return applied;
        }
        h.deleting = true;
        self.transaction(
            &[(volume_key(volume), rev)],
            vec![(
                volume_key(volume),
                Some(serde_json::to_string(&h).unwrap_or_default()),
            )],
        )
        .is_some()
    }

    pub fn delete_header(&mut self, volume: &str) -> bool {
        let Some((rev, raw)) = self.get(&volume_key(volume)) else {
            return false;
        };
        let Ok(h) = serde_json::from_str::<Header>(raw) else {
            return false;
        };
        if !h.deleting {
            return false;
        }
        let mut ops = vec![(volume_key(volume), None)];
        if !h.creating {
            ops.push((Self::tombstone_key(volume), Some("1".into())));
        }
        self.transaction(&[(volume_key(volume), rev)], ops)
            .is_some()
    }

    pub fn delete_chunks(&mut self, volume: &str) {
        let prefix = format!("/flaminio/v1/chunks/{volume}/");
        let ops: Vec<_> = self
            .data
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .take(32)
            .map(|key| (key.clone(), None))
            .collect();
        let removed: Vec<_> = ops.iter().map(|(key, _)| key.clone()).collect();
        if self.transaction(&[(volume_key(volume), 0)], ops).is_some() {
            for key in removed {
                self.published.remove(&key);
            }
        }
    }

    pub fn delete_volume(&mut self, volume: &str) {
        self.delete_fence(volume);
        self.delete_header(volume);
        self.delete_chunks(volume);
    }

    pub fn plan_move(&mut self, volume: &str, chunk: u32, to: &str) -> bool {
        let gate = self.ensure_gate();
        if self.get(&node_key(to)).is_none() || self.get(&drain_key(to)).is_some() {
            return false;
        }
        let Some((header, raw)) = self.get(&volume_key(volume)) else {
            return false;
        };
        let Ok(h) = serde_json::from_str::<Header>(raw) else {
            return false;
        };
        if h.creating || h.deleting {
            return false;
        }
        let Some((chunk_rev, raw)) = self.get(&chunk_key(volume, chunk)) else {
            return false;
        };
        let Ok(c) = serde_json::from_str::<Chunk>(raw) else {
            return false;
        };
        if c.replicas.contains(&to.to_string()) || !c.moving_to.is_empty() {
            return false;
        }
        self.planned_moves.insert(
            (volume.into(), chunk),
            PlannedMove {
                gate,
                header,
                chunk: chunk_rev,
                to: to.into(),
            },
        );
        true
    }

    pub fn commit_move(
        &mut self,
        volume: &str,
        chunk: u32,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let Some(plan) = self.planned_moves.remove(&(volume.into(), chunk)) else {
            return Ok(false);
        };
        let key = chunk_key(volume, chunk);
        let Some((_, raw)) = self.get(&key) else {
            return Ok(false);
        };
        let Ok(mut c) = serde_json::from_str::<Chunk>(raw) else {
            return Ok(false);
        };
        let mut cmps = vec![(volume_key(volume), plan.header), (key.clone(), plan.chunk)];
        if mutant != Mutant::OmittedAdmissionCompare {
            cmps.push((GATE.into(), plan.gate));
        }
        c.moving_to = plan.to.clone();
        if self
            .transaction(
                &cmps,
                vec![(
                    key.clone(),
                    Some(serde_json::to_string(&c).unwrap_or_default()),
                )],
            )
            .is_none()
        {
            return Ok(false);
        }
        if let Some(published) = self.published.get_mut(&key) {
            published.moving_to = plan.to.clone();
        }
        if let Some((d, _)) = self.get(&drain_key(&plan.to)) {
            return Err(InvariantViolation::PostMarkAdmission {
                node: plan.to,
                drain_gen: d,
            });
        }
        Ok(true)
    }

    pub fn move_replica(
        &mut self,
        volume: &str,
        chunk: u32,
        to: &str,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        if !self.plan_move(volume, chunk, to) {
            return Ok(false);
        }
        self.commit_move(volume, chunk, mutant)
    }

    /// Split CheckDrain phases. `snapshot_d` is captured at check-start.
    pub fn check_start(&self, node: &str, generation: i64) -> Option<i64> {
        let (rev, _) = self.get(&drain_key(node))?;
        if rev != generation {
            return None;
        }
        Some(rev)
    }

    pub fn header_scan(&self, d: i64, mutant: Mutant) -> u64 {
        if mutant == Mutant::OmittedReservationCount {
            return 0;
        }
        let mut n = 0u64;
        for (k, (_, raw)) in &self.data {
            if !k.starts_with("/flaminio/v1/volumes/") {
                continue;
            }
            let Ok(h) = serde_json::from_str::<Header>(raw) else {
                continue;
            };
            if h.creating && h.admission_generation < d {
                n += 1;
            }
        }
        n
    }

    pub fn chunk_scan(&self, node: &str) -> (u64, u64) {
        let mut placement = 0u64;
        let mut incoming = 0u64;
        for (k, (_, raw)) in &self.data {
            if !k.starts_with("/flaminio/v1/chunks/") {
                continue;
            }
            let Ok(c) = serde_json::from_str::<Chunk>(raw) else {
                continue;
            };
            if c.replicas.iter().any(|r| r == node) {
                placement += 1;
            }
            if c.moving_to == node {
                incoming += 1;
            }
        }
        (placement, incoming)
    }

    /// Final marker compare. Returns Ok(complete) or Err when a mutant reports success on a
    /// stale or non-empty inventory.
    pub fn final_validation(
        &self,
        node: &str,
        d: i64,
        pending: u64,
        placement: u64,
        incoming: u64,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let marker_ok = matches!(self.get(&drain_key(node)), Some((rev, _)) if rev == d);
        let compare = mutant != Mutant::OmittedFinalMarkerCompare;
        if compare && !marker_ok {
            return Ok(false);
        }
        let complete = pending == 0 && placement == 0 && incoming == 0;
        if complete && !marker_ok {
            // Mutant succeeded without a current marker (unmark / re-mark).
            return Err(InvariantViolation::PrematureOrStaleCompletion {
                node: node.to_string(),
                drain_gen: d,
            });
        }
        if complete && marker_ok {
            // Replay successful admission/publication events, independently of the inventory
            // representation and scanners. A counting omission must not erase its own oracle.
            let reservation = self
                .admitted
                .values()
                .any(|(_, generation)| *generation < d);
            let occupied = self.published.values().any(|chunk| {
                chunk.replicas.iter().any(|replica| replica == node) || chunk.moving_to == node
            });
            if reservation || occupied {
                return Err(InvariantViolation::PrematureOrStaleCompletion {
                    node: node.to_string(),
                    drain_gen: d,
                });
            }
        }
        Ok(complete && marker_ok)
    }
}

/// One independently schedulable event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    Register {
        node: String,
    },
    Mark {
        node: String,
    },
    Unmark {
        node: String,
    },
    Admit {
        volume: String,
        nodes: Vec<String>,
    },
    Plan {
        volume: String,
        nodes: Vec<String>,
        #[serde(default = "one_chunk")]
        chunk_count: usize,
    },
    Stage {
        volume: String,
    },
    Claim {
        volume: String,
    },
    Batch {
        volume: String,
    },
    Clean {
        volume: String,
    },
    /// Simulate a concurrent mark between plan and claim for the omitted-compare mutant.
    MarkThenAdmit {
        mark_node: String,
        volume: String,
        nodes: Vec<String>,
    },
    Publish {
        volume: String,
    },
    Delete {
        volume: String,
    },
    DeleteFence {
        volume: String,
    },
    DeleteHeader {
        volume: String,
    },
    DeleteChunks {
        volume: String,
    },
    Move {
        volume: String,
        chunk: u32,
        to: String,
    },
    PlanMove {
        volume: String,
        chunk: u32,
        to: String,
    },
    CommitMove {
        volume: String,
        chunk: u32,
    },
    Restart,
    CheckStart {
        node: String,
        generation: i64,
    },
    HeaderScan,
    ChunkScan {
        node: String,
    },
    FinalValidation {
        node: String,
        generation: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_pending: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_placement: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_incoming: Option<u64>,
    },
}

fn one_chunk() -> usize {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    #[serde(default)]
    pub mutant: Mutant,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub expect_violation: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct CheckCursor {
    node: String,
    d: i64,
    pending: u64,
    placement: u64,
    incoming: u64,
    started: bool,
    headers_scanned: bool,
    chunks_scanned: bool,
}

/// Run a schedule; Ok(None) clean, Ok(Some) expected violation, Err unexpected.
pub fn run_schedule(sched: &Schedule) -> Result<Option<InvariantViolation>, String> {
    let mut store = Store::new();
    let mut check = CheckCursor::default();
    let mut seen: Option<InvariantViolation> = None;
    let mut last_drain_gen: BTreeMap<String, i64> = BTreeMap::new();

    for step in &sched.steps {
        let r: Result<(), InvariantViolation> = match step {
            Step::Register { node } => {
                store.register_node(node);
                Ok(())
            }
            Step::Mark { node } => {
                if let Some(d) = store.mark_drain(node) {
                    last_drain_gen.insert(node.clone(), d);
                }
                Ok(())
            }
            Step::Unmark { node } => {
                let _ = store.unmark_drain(node);
                Ok(())
            }
            Step::Admit { volume, nodes } => {
                store.admit_create(volume, nodes, sched.mutant).map(|_| ())
            }
            Step::Plan {
                volume,
                nodes,
                chunk_count,
            } => {
                store.plan_create_chunks(volume, nodes, *chunk_count);
                Ok(())
            }
            Step::Stage { volume } => {
                store.stage_create(volume);
                Ok(())
            }
            Step::Claim { volume } => store.claim_create(volume, sched.mutant).map(|_| ()),
            Step::Batch { volume } => store.copy_batch(volume, sched.mutant).map(|_| ()),
            Step::Clean { volume } => {
                store.clean_batch(volume);
                Ok(())
            }
            Step::MarkThenAdmit {
                mark_node,
                volume,
                nodes,
            } => {
                store.plan_create(volume, nodes);
                store.stage_create(volume);
                if let Some(d) = store.mark_drain(mark_node) {
                    last_drain_gen.insert(mark_node.clone(), d);
                }
                store.claim_create(volume, sched.mutant).map(|_| ())
            }
            Step::Publish { volume } => {
                let _ = store.publish(volume);
                Ok(())
            }
            Step::Delete { volume } => {
                store.delete_volume(volume);
                Ok(())
            }
            Step::DeleteFence { volume } => {
                store.delete_fence(volume);
                Ok(())
            }
            Step::DeleteHeader { volume } => {
                store.delete_header(volume);
                Ok(())
            }
            Step::DeleteChunks { volume } => {
                store.delete_chunks(volume);
                Ok(())
            }
            Step::Move { volume, chunk, to } => store
                .move_replica(volume, *chunk, to, sched.mutant)
                .map(|_| ()),
            Step::PlanMove { volume, chunk, to } => {
                store.plan_move(volume, *chunk, to);
                Ok(())
            }
            Step::CommitMove { volume, chunk } => {
                store.commit_move(volume, *chunk, sched.mutant).map(|_| ())
            }
            Step::Restart => {
                // Durable state only; cursor reset models a restarted controller.
                check = CheckCursor::default();
                store.planned_moves.clear();
                Ok(())
            }
            Step::CheckStart { node, generation } => {
                check = CheckCursor::default();
                let drain_gen = if *generation == 0 {
                    last_drain_gen.get(node).copied().unwrap_or(0)
                } else {
                    *generation
                };
                if let Some(d) = store.check_start(node, drain_gen) {
                    check.node = node.clone();
                    check.d = d;
                    check.started = true;
                }
                Ok(())
            }
            Step::HeaderScan => {
                if check.started && !check.headers_scanned {
                    check.pending = store.header_scan(check.d, sched.mutant);
                    check.headers_scanned = true;
                }
                Ok(())
            }
            Step::ChunkScan { node } => {
                if check.started
                    && check.headers_scanned
                    && check.node == *node
                    && !check.chunks_scanned
                {
                    let (p, i) = store.chunk_scan(node);
                    check.placement = p;
                    check.incoming = i;
                    check.chunks_scanned = true;
                }
                Ok(())
            }
            Step::FinalValidation {
                node,
                generation,
                expect_pending,
                expect_placement,
                expect_incoming,
            } => {
                // generation 0 means the drain generation captured at check-start (or the last
                // mark if the check never started). Re-mark must not silently validate D2 when
                // the schedule is certifying the original D.
                let drain_gen = if *generation == 0 {
                    if check.started && check.d != 0 {
                        check.d
                    } else {
                        last_drain_gen.get(node).copied().unwrap_or(0)
                    }
                } else {
                    *generation
                };
                if !check.started
                    || !check.headers_scanned
                    || !check.chunks_scanned
                    || check.node != *node
                    || drain_gen != check.d
                {
                    Ok(())
                } else {
                    match store.final_validation(
                        node,
                        drain_gen,
                        check.pending,
                        check.placement,
                        check.incoming,
                        sched.mutant,
                    ) {
                        Ok(complete) => {
                            let current = matches!(store.get(&drain_key(node)), Some((d, _)) if d == drain_gen);
                            if current {
                                for (name, expected, actual) in [
                                    ("pending", expect_pending, check.pending),
                                    ("placement", expect_placement, check.placement),
                                    ("incoming", expect_incoming, check.incoming),
                                ] {
                                    if expected.is_some_and(|expected| expected != actual) {
                                        return Err(format!(
                                            "{name}: expected {expected:?}, got {actual}"
                                        ));
                                    }
                                }
                                if let (Some(p), Some(c), Some(i)) =
                                    (expect_pending, expect_placement, expect_incoming)
                                    && complete != (*p == 0 && *c == 0 && *i == 0)
                                {
                                    return Err("completion disagrees with fixture counters".into());
                                }
                            }
                            Ok(())
                        }
                        Err(v) => Err(v),
                    }
                }
            }
        };
        match r {
            Ok(()) => {}
            Err(v) => {
                seen = Some(v);
                break;
            }
        }
    }
    match (&sched.expect_violation, &seen) {
        (None, None) => Ok(None),
        (Some(name), Some(v)) if violation_name(v) == name.as_str() => Ok(seen),
        (Some(name), None) => Err(format!("expected violation {name}, got none")),
        (Some(name), Some(v)) => Err(format!(
            "expected violation {name}, got {}",
            violation_name(v)
        )),
        (None, Some(v)) => Err(format!("unexpected {}", violation_name(v))),
    }
}

fn violation_name(v: &InvariantViolation) -> &'static str {
    match v {
        InvariantViolation::PostMarkAdmission { .. } => "PostMarkAdmission",
        InvariantViolation::PrematureOrStaleCompletion { .. } => "PrematureOrStaleCompletion",
        InvariantViolation::FencedPublication { .. } => "FencedPublication",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleFile {
    pub schedules: Vec<Schedule>,
}

/// Load schedules from the shared JSON fixture.
pub fn load_schedules(path: &std::path::Path) -> Result<ScheduleFile, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// Built-in short schedules (no external fixture required for the model itself).
pub fn builtin_schedules() -> Vec<Schedule> {
    vec![
        Schedule {
            name: "clean_mark_admit_elsewhere".into(),
            mutant: Mutant::None,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::Register { node: "b".into() },
                Step::Mark { node: "a".into() },
                Step::Admit {
                    volume: "v1".into(),
                    nodes: vec!["b".into()],
                },
                Step::Clean {
                    volume: "v1".into(),
                },
                Step::Batch {
                    volume: "v1".into(),
                },
                Step::Publish {
                    volume: "v1".into(),
                },
            ],
            expect_violation: None,
        },
        Schedule {
            name: "omitted_admission_compare_counterexample".into(),
            mutant: Mutant::OmittedAdmissionCompare,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::MarkThenAdmit {
                    mark_node: "a".into(),
                    volume: "v".into(),
                    nodes: vec!["a".into()],
                },
            ],
            expect_violation: Some("PostMarkAdmission".into()),
        },
        Schedule {
            name: "omitted_reservation_count_counterexample".into(),
            mutant: Mutant::OmittedReservationCount,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::Register { node: "b".into() },
                Step::Admit {
                    volume: "old".into(),
                    nodes: vec!["b".into()],
                },
                Step::Mark { node: "a".into() },
                // Drain a has no replicas; older creating on b must still block completion.
                Step::CheckStart {
                    node: "a".into(),
                    generation: 0,
                },
                Step::HeaderScan,
                Step::ChunkScan { node: "a".into() },
                Step::FinalValidation {
                    node: "a".into(),
                    generation: 0,
                    expect_pending: None,
                    expect_placement: None,
                    expect_incoming: None,
                },
            ],
            expect_violation: Some("PrematureOrStaleCompletion".into()),
        },
        Schedule {
            name: "omitted_final_marker_compare_counterexample".into(),
            mutant: Mutant::OmittedFinalMarkerCompare,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::Mark { node: "a".into() },
                Step::CheckStart {
                    node: "a".into(),
                    generation: 0,
                },
                Step::HeaderScan,
                Step::ChunkScan { node: "a".into() },
                Step::Unmark { node: "a".into() },
                Step::FinalValidation {
                    node: "a".into(),
                    generation: 0,
                    expect_pending: None,
                    expect_placement: None,
                    expect_incoming: None,
                },
            ],
            expect_violation: Some("PrematureOrStaleCompletion".into()),
        },
        // Clean model: empty scan then unmark before validation must not succeed for D.
        Schedule {
            name: "empty_scan_unmark_before_validation".into(),
            mutant: Mutant::None,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::Mark { node: "a".into() },
                Step::CheckStart {
                    node: "a".into(),
                    generation: 0,
                },
                Step::HeaderScan,
                Step::ChunkScan { node: "a".into() },
                Step::Unmark { node: "a".into() },
                Step::FinalValidation {
                    node: "a".into(),
                    generation: 0,
                    expect_pending: None,
                    expect_placement: None,
                    expect_incoming: None,
                },
            ],
            expect_violation: None,
        },
        // Clean model: unmark then re-mark D2; validating original D must fail closed.
        Schedule {
            name: "empty_scan_unmark_remark_before_validation".into(),
            mutant: Mutant::None,
            steps: vec![
                Step::Register { node: "a".into() },
                Step::Mark { node: "a".into() },
                Step::CheckStart {
                    node: "a".into(),
                    generation: 0,
                },
                Step::HeaderScan,
                Step::ChunkScan { node: "a".into() },
                Step::Unmark { node: "a".into() },
                Step::Mark { node: "a".into() },
                Step::FinalValidation {
                    node: "a".into(),
                    generation: 0,
                    expect_pending: None,
                    expect_placement: None,
                    expect_incoming: None,
                },
            ],
            expect_violation: None,
        },
    ]
}

/// Run builtin schedules (generation 0 in check steps means "use the last mark for that node").
pub fn run_builtin_schedules() -> Result<(), String> {
    for sched in builtin_schedules() {
        run_schedule(&sched).map_err(|e| format!("{}: {e}", sched.name))?;
    }
    explore_short_schedules()?;
    Ok(())
}

/// Exhaust every order-preserving interleaving of a 40-chunk creator and a drain check,
/// plus every scan/lifecycle interleaving across unmark, fresh admission and re-mark.
/// The fixed budgets are 792 + 35 schedules; the independent oracle runs at each validation.
pub fn explore_short_schedules() -> Result<usize, String> {
    fn explore(
        left: &[Step],
        right: &[Step],
        prefix: &mut Vec<Step>,
        count: &mut usize,
    ) -> Result<(), String> {
        if left.is_empty() && right.is_empty() {
            let schedule = Schedule {
                name: format!("bounded_interleaving_{}", *count),
                mutant: Mutant::None,
                steps: prefix.clone(),
                expect_violation: None,
            };
            run_schedule(&schedule).map_err(|e| format!("{}: {e}", schedule.name))?;
            *count += 1;
            return Ok(());
        }
        if let Some((step, rest)) = left.split_first() {
            prefix.push(step.clone());
            explore(rest, right, prefix, count)?;
            prefix.pop();
        }
        if let Some((step, rest)) = right.split_first() {
            prefix.push(step.clone());
            explore(left, rest, prefix, count)?;
            prefix.pop();
        }
        Ok(())
    }

    let creator = vec![
        Step::Plan {
            volume: "v".into(),
            nodes: vec!["a".into()],
            chunk_count: 40,
        },
        Step::Stage { volume: "v".into() },
        Step::Claim { volume: "v".into() },
        Step::Clean { volume: "v".into() },
        Step::Batch { volume: "v".into() },
        Step::Batch { volume: "v".into() },
        Step::Publish { volume: "v".into() },
    ];
    let scan = vec![
        Step::CheckStart {
            node: "a".into(),
            generation: 0,
        },
        Step::HeaderScan,
        Step::ChunkScan { node: "a".into() },
        Step::FinalValidation {
            node: "a".into(),
            generation: 0,
            expect_pending: None,
            expect_placement: None,
            expect_incoming: None,
        },
    ];
    let mut drain = vec![Step::Mark { node: "a".into() }];
    drain.extend(scan.clone());
    let mut prefix = vec![
        Step::Register { node: "a".into() },
        Step::Register { node: "b".into() },
    ];
    let mut count = 0;
    explore(&creator, &drain, &mut prefix, &mut count)?;
    prefix.push(Step::Mark { node: "a".into() });
    let lifecycle = vec![
        Step::Unmark { node: "a".into() },
        Step::Admit {
            volume: "new".into(),
            nodes: vec!["a".into()],
        },
        Step::Mark { node: "a".into() },
    ];
    explore(&lifecycle, &scan, &mut prefix, &mut count)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../meta/testdata/placement-admission.json")
    }

    #[test]
    fn builtin_schedules_catch_injected_bugs() {
        run_builtin_schedules().unwrap();
    }

    #[test]
    fn fixture_schedules_match_model() {
        let file = load_schedules(&fixture_path()).expect("load placement-admission.json");
        assert!(
            file.schedules.len() >= 6,
            "expected clean + mutant + unmark schedules, got {}",
            file.schedules.len()
        );
        for sched in &file.schedules {
            run_schedule(sched).unwrap_or_else(|e| panic!("schedule {}: {e}", sched.name));
        }
    }

    #[test]
    fn fixture_counter_expectations_are_enforced() {
        let raw = std::fs::read_to_string(fixture_path()).unwrap();
        let original: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for field in ["expect_pending", "expect_placement", "expect_incoming"] {
            let mut value = original.clone();
            let schedule = value["schedules"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|schedule| schedule["name"] == "publication_between_header_and_chunk_scan")
                .unwrap();
            let validation = schedule["steps"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            let expected = validation[field].as_u64().unwrap();
            validation[field] = (expected + 1).into();
            let schedule: Schedule = serde_json::from_value(schedule.clone()).unwrap();
            assert!(run_schedule(&schedule).is_err(), "ignored {field}");
        }
    }

    #[test]
    fn delayed_copy_cannot_publish_after_claim_fence() {
        let mut store = Store::new();
        store.register_node("a");
        assert!(store.plan_create_chunks("v", &["a".into()], 40));
        assert!(store.stage_create("v"));
        assert!(store.claim_create("v", Mutant::None).unwrap());
        assert!(store.clean_batch("v"));
        assert!(store.copy_batch("v", Mutant::None).unwrap());
        assert!(store.delete_fence("v"));
        assert!(!store.copy_batch("v", Mutant::None).unwrap());
        assert_eq!(store.chunk_scan("a"), (32, 0));
    }

    #[test]
    fn completion_oracle_accounts_for_admission_publication_and_incoming_history() {
        let mut store = Store::new();
        store.register_node("a");
        store.register_node("b");
        assert!(
            store
                .admit_create("v", &["b".into()], Mutant::None)
                .unwrap()
        );
        let d = store.mark_drain("a").unwrap();
        // Even an unrelated earlier reservation blocks a claimed zero completion.
        assert!(matches!(
            store.final_validation("a", d, 0, 0, 0, Mutant::None),
            Err(InvariantViolation::PrematureOrStaleCompletion { .. })
        ));
        assert!(store.clean_batch("v"));
        assert!(store.copy_batch("v", Mutant::None).unwrap());
        assert!(store.publish("v"));
        let d = store.mark_drain("b").unwrap();
        assert!(matches!(
            store.final_validation("b", d, 0, 0, 0, Mutant::None),
            Err(InvariantViolation::PrematureOrStaleCompletion { .. })
        ));
        assert!(store.unmark_drain("a"));
        assert!(store.move_replica("v", 0, "a", Mutant::None).unwrap());
        let d = store.mark_drain("a").unwrap();
        assert!(matches!(
            store.final_validation("a", d, 0, 0, 0, Mutant::None),
            Err(InvariantViolation::PrematureOrStaleCompletion { .. })
        ));
    }

    #[test]
    fn clean_model_refuses_stale_generation() {
        let mut store = Store::new();
        store.register_node("a");
        let d = store.mark_drain("a").unwrap();
        store.unmark_drain("a");
        assert!(store.check_start("a", d).is_none());
        let complete = store
            .final_validation("a", d, 0, 0, 0, Mutant::None)
            .unwrap();
        assert!(!complete);
    }

    #[test]
    fn each_injected_bug_yields_counterexample() {
        for name in [
            "omitted_admission_compare_counterexample",
            "omitted_reservation_count_counterexample",
            "omitted_final_marker_compare_counterexample",
        ] {
            let sched = builtin_schedules()
                .into_iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert!(sched.expect_violation.is_some());
            run_schedule(&sched).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn bounded_exploration_and_clean_mutation_controls() {
        assert_eq!(explore_short_schedules().unwrap(), 827);
        let file = load_schedules(&fixture_path()).unwrap();
        for schedule in file.schedules {
            if schedule.mutant == Mutant::None {
                continue;
            }
            let mut clean = schedule.clone();
            clean.mutant = Mutant::None;
            clean.expect_violation = None;
            assert!(run_schedule(&clean).unwrap().is_none(), "{}", clean.name);
            assert!(
                run_schedule(&schedule).unwrap().is_some(),
                "mutation was not caught on the identical bounded schedule: {}",
                schedule.name
            );
        }
    }
}
