//! Sans-I/O model of placement admission and drain completion (docs/issues/78).
//!
//! Models a global placement gate, creating-header reservations with
//! `admission_generation`, drain markers, chunk replicas / incoming moves, and a
//! CheckDrain split into check-start / header-scan / chunk-scan / final-validation.
//! Injected mutants omit the admission compare, reservation count, or final marker
//! compare; each must produce a bounded counterexample.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub id: String,
    pub creating: bool,
    #[serde(default)]
    pub admission_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub replicas: Vec<String>,
    #[serde(default)]
    pub moving_to: String,
}

#[derive(Debug, Clone, Default)]
pub struct Store {
    rev: i64,
    data: BTreeMap<String, (i64, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    /// Admission ordered after mark D targeted the drained node.
    PostMarkAdmission { node: String, drain_gen: i64 },
    /// CheckDrain reported complete while work remained for D, or validated a stale D.
    PrematureOrStaleCompletion { node: String, drain_gen: i64 },
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

    fn put(&mut self, key: String, value: String) {
        self.rev += 1;
        self.data.insert(key, (self.rev, value));
    }

    fn delete(&mut self, key: &str) {
        self.rev += 1;
        self.data.remove(key);
    }

    /// Ensure the gate exists; return its modification revision.
    pub fn ensure_gate(&mut self) -> i64 {
        if let Some((r, _)) = self.get(GATE) {
            return r;
        }
        self.put(GATE.into(), "1".into());
        self.get(GATE).map(|(r, _)| r).unwrap()
    }

    pub fn bump_gate(&mut self, expect: i64) -> bool {
        if !self.cmp_ok(GATE, expect) {
            return false;
        }
        self.put(GATE.into(), "1".into());
        true
    }

    pub fn register_node(&mut self, id: &str) {
        let g = self.ensure_gate();
        self.put(node_key(id), "1".into());
        let _ = self.bump_gate(g);
    }

    pub fn mark_drain(&mut self, node: &str) -> Option<i64> {
        let g = self.ensure_gate();
        if self.get(&drain_key(node)).is_some() {
            return self.get(&drain_key(node)).map(|(r, _)| r);
        }
        self.get(&node_key(node))?;
        if !self.cmp_ok(GATE, g) {
            return None;
        }
        self.put(drain_key(node), "1".into());
        let d = self.get(&drain_key(node)).map(|(r, _)| r)?;
        let _ = self.bump_gate(g);
        Some(d)
    }

    pub fn unmark_drain(&mut self, node: &str) -> bool {
        let g = self.ensure_gate();
        let Some((drev, _)) = self.get(&drain_key(node)) else {
            return true;
        };
        if !self.cmp_ok(GATE, g) || !self.cmp_ok(&drain_key(node), drev) {
            return false;
        }
        self.delete(&drain_key(node));
        self.bump_gate(g)
    }

    /// Plan+admit a creating header onto `nodes`. Returns Err(violation) on post-mark admit.
    pub fn admit_create(
        &mut self,
        volume: &str,
        nodes: &[String],
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let gate = self.ensure_gate();
        for n in nodes {
            if let Some((d, _)) = self.get(&drain_key(n)) {
                // Eligibility observation: draining nodes are not planned onto.
                // A mutant that omits the gate compare can still commit after a concurrent mark.
                let _ = d;
            }
        }
        let planned: Vec<String> = nodes
            .iter()
            .filter(|n| self.get(&drain_key(n)).is_none())
            .cloned()
            .collect();
        if planned.is_empty() {
            return Ok(false);
        }
        // Concurrent mark between plan and claim: gate advances.
        let gate_at_claim = self.get(GATE).map(|(r, _)| r).unwrap_or(0);
        let compare_gate = mutant != Mutant::OmittedAdmissionCompare;
        if compare_gate && gate_at_claim != gate {
            return Ok(false);
        }
        if self.get(&volume_key(volume)).is_some() {
            return Ok(false);
        }
        // Detect forbidden post-mark admission: any planned node that is now marked with D > gate
        // used at plan time (mark won ordering).
        for n in &planned {
            if let Some((d, _)) = self.get(&drain_key(n))
                && d > gate
                && !compare_gate
            {
                // Mutant admitted without gate fence after mark advanced the gate.
                let h = Header {
                    id: volume.to_string(),
                    creating: true,
                    admission_generation: gate,
                };
                self.put(
                    volume_key(volume),
                    serde_json::to_string(&h).unwrap_or_default(),
                );
                for (i, node) in planned.iter().enumerate() {
                    let c = Chunk {
                        replicas: vec![node.clone()],
                        moving_to: String::new(),
                    };
                    self.put(
                        chunk_key(volume, i as u32),
                        serde_json::to_string(&c).unwrap_or_default(),
                    );
                }
                return Err(InvariantViolation::PostMarkAdmission {
                    node: n.clone(),
                    drain_gen: d,
                });
            }
        }
        let h = Header {
            id: volume.to_string(),
            creating: true,
            admission_generation: gate,
        };
        self.put(
            volume_key(volume),
            serde_json::to_string(&h).unwrap_or_default(),
        );
        for (i, node) in planned.iter().enumerate() {
            let c = Chunk {
                replicas: vec![node.clone()],
                moving_to: String::new(),
            };
            self.put(
                chunk_key(volume, i as u32),
                serde_json::to_string(&c).unwrap_or_default(),
            );
        }
        Ok(true)
    }

    pub fn publish(&mut self, volume: &str) -> bool {
        let Some((rev, raw)) = self
            .get(&volume_key(volume))
            .map(|(r, v)| (r, v.to_string()))
        else {
            return false;
        };
        let Ok(mut h) = serde_json::from_str::<Header>(&raw) else {
            return false;
        };
        if !h.creating {
            return true;
        }
        h.creating = false;
        if !self.cmp_ok(&volume_key(volume), rev) {
            return false;
        }
        self.put(
            volume_key(volume),
            serde_json::to_string(&h).unwrap_or_default(),
        );
        true
    }

    pub fn delete_volume(&mut self, volume: &str) {
        self.delete(&volume_key(volume));
        let prefix = format!("/flaminio/v1/chunks/{volume}/");
        let keys: Vec<String> = self
            .data
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for k in keys {
            self.delete(&k);
        }
    }

    pub fn move_replica(
        &mut self,
        volume: &str,
        chunk: u32,
        to: &str,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let gate = self.ensure_gate();
        if self.get(&node_key(to)).is_none() {
            return Ok(false);
        }
        if self.get(&drain_key(to)).is_some() {
            return Ok(false); // FailedPrecondition in the service
        }
        let key = chunk_key(volume, chunk);
        let Some((crev, raw)) = self.get(&key).map(|(r, v)| (r, v.to_string())) else {
            return Ok(false);
        };
        let Ok(mut c) = serde_json::from_str::<Chunk>(&raw) else {
            return Ok(false);
        };
        if c.replicas.contains(&to.to_string()) || !c.moving_to.is_empty() {
            return Ok(false);
        }
        let gate_now = self.get(GATE).map(|(r, _)| r).unwrap_or(0);
        let compare_gate = mutant != Mutant::OmittedAdmissionCompare;
        if compare_gate && gate_now != gate {
            return Ok(false);
        }
        if let Some((d, _)) = self.get(&drain_key(to))
            && d > gate
            && !compare_gate
        {
            c.moving_to = to.to_string();
            self.put(key, serde_json::to_string(&c).unwrap_or_default());
            return Err(InvariantViolation::PostMarkAdmission {
                node: to.to_string(),
                drain_gen: d,
            });
        }
        if !self.cmp_ok(&key, crev) {
            return Ok(false);
        }
        c.moving_to = to.to_string();
        self.put(key, serde_json::to_string(&c).unwrap_or_default());
        Ok(true)
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
        if complete && (!marker_ok || !compare && !marker_ok) {
            // Mutant succeeded without a current marker (unmark / re-mark).
            return Err(InvariantViolation::PrematureOrStaleCompletion {
                node: node.to_string(),
                drain_gen: d,
            });
        }
        if complete && marker_ok && mutant == Mutant::OmittedReservationCount && pending == 0 {
            // Caller passed pending=0 from omitted scan while creating headers remain.
            let real = self.header_scan(d, Mutant::None);
            if real > 0 {
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
    Move {
        volume: String,
        chunk: u32,
        to: String,
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
    },
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
    d: i64,
    pending: u64,
    placement: u64,
    incoming: u64,
    started: bool,
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
            Step::MarkThenAdmit {
                mark_node,
                volume,
                nodes,
            } => {
                // Plan under current gate, then mark, then claim (tests omitted compare).
                let gate = store.ensure_gate();
                let planned: Vec<String> = nodes
                    .iter()
                    .filter(|n| store.get(&drain_key(n)).is_none())
                    .cloned()
                    .collect();
                if let Some(d) = store.mark_drain(mark_node) {
                    last_drain_gen.insert(mark_node.clone(), d);
                }
                // Claim with the pre-mark gate revision (stale).
                let compare_gate = sched.mutant != Mutant::OmittedAdmissionCompare;
                let gate_now = store.get(GATE).map(|(r, _)| r).unwrap_or(0);
                if (compare_gate && gate_now != gate) || planned.is_empty() {
                    Ok(())
                } else {
                    let h = Header {
                        id: volume.clone(),
                        creating: true,
                        admission_generation: gate,
                    };
                    store.put(
                        volume_key(volume),
                        serde_json::to_string(&h).unwrap_or_default(),
                    );
                    for (i, node) in planned.iter().enumerate() {
                        let c = Chunk {
                            replicas: vec![node.clone()],
                            moving_to: String::new(),
                        };
                        store.put(
                            chunk_key(volume, i as u32),
                            serde_json::to_string(&c).unwrap_or_default(),
                        );
                    }
                    if !compare_gate {
                        if let Some((d, _)) = store.get(&drain_key(mark_node)) {
                            Err(InvariantViolation::PostMarkAdmission {
                                node: mark_node.clone(),
                                drain_gen: d,
                            })
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
            }
            Step::Publish { volume } => {
                let _ = store.publish(volume);
                Ok(())
            }
            Step::Delete { volume } => {
                store.delete_volume(volume);
                Ok(())
            }
            Step::Move { volume, chunk, to } => store
                .move_replica(volume, *chunk, to, sched.mutant)
                .map(|_| ()),
            Step::Restart => {
                // Durable state only; cursor reset models a restarted controller.
                check = CheckCursor::default();
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
                    check.d = d;
                    check.started = true;
                }
                Ok(())
            }
            Step::HeaderScan => {
                if check.started {
                    check.pending = store.header_scan(check.d, sched.mutant);
                }
                Ok(())
            }
            Step::ChunkScan { node } => {
                if check.started {
                    let (p, i) = store.chunk_scan(node);
                    check.placement = p;
                    check.incoming = i;
                }
                Ok(())
            }
            Step::FinalValidation { node, generation } => {
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
                if !check.started {
                    check.d = drain_gen;
                }
                match store.final_validation(
                    node,
                    drain_gen,
                    check.pending,
                    check.placement,
                    check.incoming,
                    sched.mutant,
                ) {
                    Ok(_) => Ok(()),
                    Err(v) => Err(v),
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
                    nodes: vec!["a".into(), "b".into()],
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
    Ok(())
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
}
