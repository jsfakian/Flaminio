//! Sans-I/O model of interrupted volume creation recovery (docs/issues/76).
//!
//! Models create roots, birth fences, plan keys, headers and delete-only GC. Schedules in
//! `meta/testdata/create-recovery.json` drive transitions. Invariants:
//! - `NoRootResurrection`: a GC'd operation id never exists again.
//! - `NoUnprovedTombstoneBypass`: skipping a tombstone requires the unpublished proof.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Create-root progress states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootState {
    Preparing,
    Sealed,
    Cleaning,
    Copying,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSpec {
    pub size_bytes: u64,
    pub chunk_bytes: u64,
    pub replicas: u32,
    pub min_isr: u32,
    #[serde(default)]
    pub prefer_node: String,
    #[serde(default)]
    pub failure_domain: String,
    #[serde(default)]
    pub failure_domain_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub id: String,
    pub creating: bool,
    pub deleting: bool,
    pub epoch: u64,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub create_operation: String,
    #[serde(default)]
    pub create_spec: Option<CreateSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub volume: String,
    pub spec: CreateSpec,
    pub state: RootState,
    pub chunk_count: u32,
    pub staged: u32,
    pub next: u32,
}

/// One compare in a model transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cmp {
    pub key: String,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Txn {
    pub cmps: Vec<Cmp>,
    pub ops: Vec<Op>,
}

/// Injected protocol mutants the schedule runner can enable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutant {
    #[default]
    None,
    /// Drop the birth-revision compare on initial root creation.
    MissingBirthCompare,
    /// Treat Creating alone as unpublished (ignores epoch/target proof).
    WeakenedUnpublishedClassifier,
}

#[derive(Debug, Clone, Default)]
pub struct Store {
    rev: i64,
    data: BTreeMap<String, (i64, String)>,
    /// Operation ids whose roots were deleted; must never reappear.
    pub retired_ops: BTreeSet<String>,
    /// Per-volume publication / Attach / token history for NoUnprovedTombstoneBypass.
    pub incarnation: BTreeMap<String, IncarnationHistory>,
}

#[derive(Debug, Clone, Default)]
pub struct IncarnationHistory {
    pub published: bool,
    pub attach_authorized: bool,
    pub token_issued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    RootResurrection { op: String },
    UnprovedTombstoneBypass { volume: String },
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<(i64, &str)> {
        self.data.get(key).map(|(r, v)| (*r, v.as_str()))
    }

    pub fn apply(&mut self, txn: &Txn, mutant: Mutant) -> Result<bool, InvariantViolation> {
        let mut cmps = txn.cmps.clone();
        if mutant == Mutant::MissingBirthCompare {
            cmps.retain(|c| !c.key.contains("/create-birth/"));
        }
        for c in &cmps {
            let cur = self.data.get(&c.key).map(|(r, _)| *r).unwrap_or(0);
            if cur != c.rev {
                return Ok(false);
            }
        }
        self.rev += 1;
        let rev = self.rev;
        for op in &txn.ops {
            if op.delete {
                if let Some(op_id) = op.key.strip_prefix("/flaminio/v1/create-roots/") {
                    self.retired_ops.insert(op_id.to_string());
                }
                self.data.remove(&op.key);
            } else {
                if let Some(op_id) = op.key.strip_prefix("/flaminio/v1/create-roots/")
                    && self.retired_ops.contains(op_id)
                {
                    return Err(InvariantViolation::RootResurrection {
                        op: op_id.to_string(),
                    });
                }
                self.data.insert(op.key.clone(), (rev, op.value.clone()));
            }
        }
        Ok(true)
    }

    /// Recognized never-published shape, or the weakened mutant.
    pub fn unpublished_proof(h: &Header, mutant: Mutant) -> bool {
        match mutant {
            Mutant::WeakenedUnpublishedClassifier => h.creating,
            _ => h.creating && h.epoch == 0 && h.target.is_empty(),
        }
    }

    /// CAS-delete header without a tombstone; checks NoUnprovedTombstoneBypass.
    pub fn unpublished_rollback(
        &mut self,
        volume: &str,
        header_rev: i64,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let key = format!("/flaminio/v1/volumes/{volume}");
        let Some((rev, raw)) = self.get(&key) else {
            return Ok(false);
        };
        if rev != header_rev {
            return Ok(false);
        }
        let h: Header = serde_json::from_str(raw).unwrap_or(Header {
            id: volume.to_string(),
            creating: false,
            deleting: false,
            epoch: 0,
            target: String::new(),
            create_operation: String::new(),
            create_spec: None,
        });
        if !Self::unpublished_proof(&h, mutant) {
            return Ok(false);
        }
        let hist = self.incarnation.entry(volume.to_string()).or_default();
        if hist.published || hist.attach_authorized || hist.token_issued {
            return Err(InvariantViolation::UnprovedTombstoneBypass {
                volume: volume.to_string(),
            });
        }
        let txn = Txn {
            cmps: vec![
                Cmp {
                    key: key.clone(),
                    rev: header_rev,
                },
                Cmp {
                    key: format!("/flaminio/v1/tombstones/{volume}"),
                    rev: 0,
                },
            ],
            ops: vec![Op {
                key,
                value: String::new(),
                delete: true,
            }],
        };
        self.apply(&txn, Mutant::None)
    }

    /// Initial root creation with birth fence (unless mutant strips it).
    pub fn create_root(
        &mut self,
        op: &str,
        volume: &str,
        birth_rev: i64,
        root_json: &str,
        mutant: Mutant,
    ) -> Result<bool, InvariantViolation> {
        let shard = birth_shard(volume);
        let txn = Txn {
            cmps: vec![
                Cmp {
                    key: format!("/flaminio/v1/create-roots/{op}"),
                    rev: 0,
                },
                Cmp {
                    key: format!("/flaminio/v1/create-birth/{shard}"),
                    rev: birth_rev,
                },
            ],
            ops: vec![
                Op {
                    key: format!("/flaminio/v1/create-roots/{op}"),
                    value: root_json.to_string(),
                    delete: false,
                },
                Op {
                    key: format!("/flaminio/v1/create-birth/{shard}"),
                    value: "1".into(),
                    delete: false,
                },
            ],
        };
        self.apply(&txn, mutant)
    }
}

fn birth_shard(volume: &str) -> String {
    use std::hash::{Hash, Hasher};
    // Stable nibble from a simple hash of the volume id (test model; Go uses SHA-256).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    volume.hash(&mut h);
    format!("{:x}", (h.finish() >> 60) & 0xf)
}

/// One step in a shared schedule fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    CreateRoot {
        op: String,
        volume: String,
        root: String,
    },
    GcRoot {
        op: String,
    },
    ReplayCreateRoot {
        op: String,
        volume: String,
        root: String,
        birth_rev: i64,
    },
    MarkPublished {
        volume: String,
    },
    MarkAttach {
        volume: String,
    },
    MarkToken {
        volume: String,
    },
    UnpublishedRollback {
        volume: String,
        header: Header,
        header_rev: i64,
    },
    PutHeader {
        header: Header,
        rev_hint: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    #[serde(default)]
    pub mutant: Mutant,
    pub steps: Vec<Step>,
    /// When set, the schedule must produce this invariant violation.
    #[serde(default)]
    pub expect_violation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleFile {
    pub schedules: Vec<Schedule>,
}

/// Run one schedule; returns Ok(None) on success, Ok(Some(v)) when an expected violation is
/// observed, or Err when an unexpected violation or missing expected violation occurs.
pub fn run_schedule(sched: &Schedule) -> Result<Option<InvariantViolation>, String> {
    let mut store = Store::new();
    let mut seen: Option<InvariantViolation> = None;
    for step in &sched.steps {
        let r = match step {
            Step::CreateRoot { op, volume, root } => {
                let birth_rev = store
                    .get(&format!(
                        "/flaminio/v1/create-birth/{}",
                        birth_shard(volume)
                    ))
                    .map(|(r, _)| r)
                    .unwrap_or(0);
                store.create_root(op, volume, birth_rev, root, sched.mutant)
            }
            Step::GcRoot { op } => {
                let key = format!("/flaminio/v1/create-roots/{op}");
                let Some((rev, _)) = store.get(&key) else {
                    continue;
                };
                store.apply(
                    &Txn {
                        cmps: vec![Cmp {
                            key: key.clone(),
                            rev,
                        }],
                        ops: vec![Op {
                            key,
                            value: String::new(),
                            delete: true,
                        }],
                    },
                    Mutant::None,
                )
            }
            Step::ReplayCreateRoot {
                op,
                volume,
                root,
                birth_rev,
            } => store.create_root(op, volume, *birth_rev, root, sched.mutant),
            Step::MarkPublished { volume } => {
                store
                    .incarnation
                    .entry(volume.clone())
                    .or_default()
                    .published = true;
                Ok(true)
            }
            Step::MarkAttach { volume } => {
                store
                    .incarnation
                    .entry(volume.clone())
                    .or_default()
                    .attach_authorized = true;
                Ok(true)
            }
            Step::MarkToken { volume } => {
                store
                    .incarnation
                    .entry(volume.clone())
                    .or_default()
                    .token_issued = true;
                Ok(true)
            }
            Step::PutHeader {
                header,
                rev_hint: _,
            } => {
                let key = format!("/flaminio/v1/volumes/{}", header.id);
                let raw = serde_json::to_string(header).map_err(|e| e.to_string())?;
                store.apply(
                    &Txn {
                        cmps: vec![],
                        ops: vec![Op {
                            key,
                            value: raw,
                            delete: false,
                        }],
                    },
                    Mutant::None,
                )
            }
            Step::UnpublishedRollback {
                volume,
                header,
                header_rev,
            } => {
                let key = format!("/flaminio/v1/volumes/{volume}");
                if store.get(&key).is_none() {
                    let raw = serde_json::to_string(header).map_err(|e| e.to_string())?;
                    let _ = store.apply(
                        &Txn {
                            cmps: vec![],
                            ops: vec![Op {
                                key: key.clone(),
                                value: raw,
                                delete: false,
                            }],
                        },
                        Mutant::None,
                    );
                }
                let rev = store.get(&key).map(|(r, _)| r).unwrap_or(*header_rev);
                store.unpublished_rollback(volume, rev, sched.mutant)
            }
        };
        match r {
            Ok(_) => {}
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
        InvariantViolation::RootResurrection { .. } => "NoRootResurrection",
        InvariantViolation::UnprovedTombstoneBypass { .. } => "NoUnprovedTombstoneBypass",
    }
}

/// Load schedules from the shared JSON fixture (repo-relative or absolute).
pub fn load_schedules(path: &std::path::Path) -> Result<ScheduleFile, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../meta/testdata/create-recovery.json")
    }

    #[test]
    fn create_recovery_schedules() {
        let file = load_schedules(&fixture_path()).expect("load create-recovery.json");
        assert!(
            file.schedules.len() >= 4,
            "expected birth/unpublished schedules, got {}",
            file.schedules.len()
        );
        for sched in &file.schedules {
            run_schedule(sched).unwrap_or_else(|e| panic!("schedule {}: {e}", sched.name));
        }
    }

    #[test]
    fn missing_birth_compare_is_required_counterexample() {
        let file = load_schedules(&fixture_path()).expect("load");
        let sched = file
            .schedules
            .iter()
            .find(|s| s.name == "missing_birth_compare_counterexample")
            .expect("missing_birth_compare_counterexample");
        assert_eq!(sched.mutant, Mutant::MissingBirthCompare);
        assert_eq!(
            sched.expect_violation.as_deref(),
            Some("NoRootResurrection")
        );
        run_schedule(sched).unwrap();
    }

    #[test]
    fn weakened_unpublished_classifier_is_required_counterexample() {
        let file = load_schedules(&fixture_path()).expect("load");
        let sched = file
            .schedules
            .iter()
            .find(|s| s.name == "weakened_unpublished_classifier_counterexample")
            .expect("weakened_unpublished_classifier_counterexample");
        assert_eq!(sched.mutant, Mutant::WeakenedUnpublishedClassifier);
        assert_eq!(
            sched.expect_violation.as_deref(),
            Some("NoUnprovedTombstoneBypass")
        );
        run_schedule(sched).unwrap();
    }
}
