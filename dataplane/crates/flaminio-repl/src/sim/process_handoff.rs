//! Symbolic authenticated handoff model. Signatures are unforgeable symbolic identities;
//! native acceptance replays the same schedules against real CAS, FLCT/FLCH and mTLS.
//! Authority observations are recorded before proof verification or target admission.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutation {
    None,
    AcceptOldProcessHandoff,
    MintHandoffWithoutEpochAdvance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub volume: u32,
    pub node: u32,
    pub incarnation: u64,
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub capability: Capability,
    bound_capability: Capability,
    capability_id: u64,
    bound_capability_id: u64,
    pub nonce: u64,
    purpose: u8,
    authenticated: bool,
    pub issuance: usize,
}

#[derive(Debug, Clone)]
pub struct Issuance {
    pub before_epoch: u64,
    pub committed_epoch: u64,
    pub committed_target: u32,
    pub nonce: u64,
    pub capability: Capability,
    pub authorized: bool,
}

impl Issuance {
    pub fn credentials(&self, issuance: usize) -> Credentials {
        Credentials {
            capability: self.capability,
            bound_capability: self.capability,
            nonce: self.nonce,
            capability_id: issuance as u64 + 1,
            bound_capability_id: issuance as u64 + 1,
            purpose: 1,
            authenticated: true,
            issuance,
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if !self.authorized
            || self.committed_epoch <= self.before_epoch
            || self.capability.incarnation == 0
            || self.capability.incarnation >= self.capability.epoch
            || self.capability.epoch != self.committed_epoch
            || self.capability.node != self.committed_target
            || self.nonce == 0
        {
            Err("FreshProcessEpoch: handoff minted without authorized strict epoch advance".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Process {
    pub nonce: u64,
    ran_epoch: Option<u64>,
    live: Option<Capability>,
    pub healthy: bool,
}

impl Process {
    pub fn new(nonce: u64) -> Self {
        assert_ne!(nonce, 0);
        Self {
            nonce,
            ran_epoch: None,
            live: None,
            healthy: true,
        }
    }
    pub fn stop(&mut self) {
        self.live = None;
    }
    pub fn quarantine(&mut self) {
        self.healthy = false;
    }

    pub fn start(
        &mut self,
        credentials: &Credentials,
        expected: Capability,
        mutation: Mutation,
    ) -> bool {
        if !self.healthy
            || expected.incarnation == 0
            || expected.incarnation >= expected.epoch
            || !credentials.authenticated
            || credentials.purpose != 1
            || credentials.capability != expected
            || credentials.bound_capability != expected
            || credentials.capability_id != credentials.bound_capability_id
            || (credentials.nonce != self.nonce && mutation != Mutation::AcceptOldProcessHandoff)
        {
            return false;
        }
        if self.live == Some(expected) {
            return true;
        } // exact live retry, no new lifetime
        if self.live.is_some() || self.ran_epoch.is_some_and(|epoch| epoch >= expected.epoch) {
            return false;
        }
        self.ran_epoch = Some(expected.epoch);
        self.live = Some(expected);
        true
    }
}

/// Independent issuance/process history check; valid signatures alone confer no freshness.
pub fn check_start(
    issued: &[Issuance],
    credentials: &Credentials,
    current_nonce: u64,
    expected: Capability,
) -> Result<(), String> {
    let Some(record) = issued.get(credentials.issuance) else {
        return Err("FreshProcessEpoch: no authorized issuance history".into());
    };
    record.check()?;
    if record.nonce != current_nonce
        || record.capability != expected
        || credentials.capability_id != credentials.issuance as u64 + 1
    {
        return Err("FreshProcessEpoch: admitted old process or substituted capability".into());
    }
    Ok(())
}

/// Native replay expands symbolic nonces to exactly 32 bytes: four repetitions of the
/// nonzero u64's little-endian representation. This is a fixture convention, never entropy.
pub fn nonce_bytes(nonce: u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    for chunk in bytes.chunks_exact_mut(8) {
        chunk.copy_from_slice(&nonce.to_le_bytes());
    }
    bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Admin,
    OwnAgent,
    OtherAgent,
    Io,
    Node,
    Reader,
    Anonymous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Challenge {
        nonce: u64,
    },
    Restart {
        nonce: u64,
    },
    ClockFault,
    MetadataAvailable {
        available: bool,
    },
    AuthorizedAttach {
        actor: Actor,
        only_if_target: bool,
        unless_target: bool,
        nonce: u64,
        slot: String,
        expect_issued: bool,
        expect_epoch: u64,
        expect_target: u32,
        expect_mutation: bool,
    },
    TargetStart {
        slot: String,
        expect_started: bool,
    },
    StopTarget,
    TamperCapability {
        slot: String,
        field: String,
        value: u64,
    },
    TamperProof {
        slot: String,
        field: String,
        value: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub name: String,
    pub volume: u32,
    pub node: u32,
    pub first_epoch: u64,
    pub initial_epoch: u64,
    pub initial_target: u32,
    pub local_replica: bool,
    pub steps: Vec<Step>,
    pub mutant: Mutation,
    pub expected_invariant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleFile {
    pub version: u32,
    pub schedules: Vec<Schedule>,
}

#[derive(Debug, Clone, Default)]
pub struct ScheduleReport {
    pub steps: usize,
    pub issued: usize,
    pub started: usize,
    pub refused: usize,
    pub violations: Vec<String>,
    pub local_fence: Option<u64>,
}

pub fn parse_schedules(raw: &str) -> Result<ScheduleFile, String> {
    let file: ScheduleFile = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if file.version != 1 {
        return Err("unsupported handoff schedule version".into());
    }
    Ok(file)
}

pub fn run_schedule(schedule: &Schedule, mutation: Mutation) -> Result<ScheduleReport, String> {
    if schedule.first_epoch == 0 || schedule.initial_epoch <= schedule.first_epoch {
        return Err("native fixture requires 0 < FirstEpoch < initial epoch".into());
    }
    let mut epoch = schedule.initial_epoch;
    let mut owner = schedule.initial_target;
    let mut process = Process::new(1);
    let mut available = true;
    let mut issued = Vec::new();
    let mut slots: BTreeMap<String, Credentials> = BTreeMap::new();
    let mut report = ScheduleReport {
        local_fence: schedule.local_replica.then_some(schedule.initial_epoch),
        ..ScheduleReport::default()
    };
    let mut used_nonces = std::collections::BTreeSet::new();
    // Local disk state is deliberately irrelevant to current-process freshness. In
    // particular the no-local-replica fixture cannot be rescued by a local fence check.
    for step in &schedule.steps {
        report.steps += 1;
        match step {
            Step::Challenge { nonce } => {
                if report.steps != 1 {
                    return Err("challenge is startup observation, not a reset".into());
                }
                if *nonce == 0 || !used_nonces.insert(*nonce) {
                    return Err("invalid fixture challenge".into());
                }
                process = Process::new(*nonce);
            }
            Step::Restart { nonce } => {
                if *nonce == 0 || !used_nonces.insert(*nonce) {
                    return Err("reused process nonce in fixture".into());
                }
                process = Process::new(*nonce);
            }
            Step::ClockFault => process.quarantine(),
            Step::MetadataAvailable { available: value } => available = *value,
            Step::AuthorizedAttach {
                actor,
                only_if_target,
                unless_target,
                nonce,
                slot,
                expect_issued,
                expect_epoch,
                expect_target,
                expect_mutation,
            } => {
                let before = epoch;
                let before_owner = owner;
                let authorized =
                    *actor == Actor::Admin || (*actor == Actor::OwnAgent && *only_if_target);
                // other-agent only-if-target cannot act for this node; IO/reader/anonymous
                // are rejected by the real method authorization before mutation.
                let permitted = authorized
                    && available
                    && *nonce != 0
                    && !unless_target
                    && (!only_if_target || owner == schedule.node)
                    && epoch != u64::MAX;
                if permitted {
                    if mutation != Mutation::MintHandoffWithoutEpochAdvance {
                        epoch += 1;
                    }
                    owner = schedule.node;
                    let capability = Capability {
                        volume: schedule.volume,
                        node: owner,
                        incarnation: schedule.first_epoch,
                        epoch,
                    };
                    let record = Issuance {
                        before_epoch: before,
                        committed_epoch: epoch,
                        committed_target: owner,
                        nonce: *nonce,
                        capability,
                        authorized,
                    };
                    // This oracle runs even when another layer would later refuse Start.
                    if let Err(error) = record.check() {
                        report.violations.push(error);
                    }
                    let credentials = record.credentials(issued.len());
                    issued.push(record);
                    slots.insert(slot.clone(), credentials);
                    report.issued += 1;
                } else {
                    report.refused += 1;
                }
                if mutation == Mutation::None
                    && (permitted != *expect_issued
                        || epoch != *expect_epoch
                        || owner != *expect_target
                        || ((epoch, owner) != (before, before_owner)) != *expect_mutation)
                {
                    return Err(format!(
                        "{}: Attach expectations failed at step {}",
                        schedule.name, report.steps
                    ));
                }
            }
            Step::TargetStart {
                slot,
                expect_started,
            } => {
                let credentials = slots
                    .get(slot)
                    .ok_or_else(|| format!("missing credential slot {slot}"))?;
                let expected = Capability {
                    volume: schedule.volume,
                    node: schedule.node,
                    incarnation: schedule.first_epoch,
                    epoch,
                };
                let started =
                    owner == schedule.node && process.start(credentials, expected, mutation);
                if started {
                    report.started += 1;
                    if let Some(fence) = &mut report.local_fence {
                        *fence = (*fence).max(epoch);
                    }
                    if let Err(error) = check_start(&issued, credentials, process.nonce, expected) {
                        report.violations.push(error);
                    }
                } else {
                    report.refused += 1;
                }
                if mutation == Mutation::None && started != *expect_started {
                    return Err(format!(
                        "{}: Start expectation failed at step {}",
                        schedule.name, report.steps
                    ));
                }
            }
            Step::StopTarget => process.stop(),
            Step::TamperCapability { slot, field, value } => {
                let credentials = slots.get_mut(slot).ok_or("unknown slot")?;
                match field.as_str() {
                    "volume" => {
                        credentials.capability.volume =
                            u32::try_from(*value).map_err(|_| "volume overflow")?
                    }
                    "node" => {
                        credentials.capability.node =
                            u32::try_from(*value).map_err(|_| "node overflow")?
                    }
                    "incarnation" => credentials.capability.incarnation = *value,
                    "epoch" => credentials.capability.epoch = *value,
                    _ => return Err("unknown capability field".into()),
                }
            }
            Step::TamperProof { slot, field, value } => {
                let credentials = slots.get_mut(slot).ok_or("unknown slot")?;
                match field.as_str() {
                    "nonce" => credentials.nonce = *value,
                    "purpose" => {
                        credentials.purpose =
                            u8::try_from(*value).map_err(|_| "purpose overflow")?
                    }
                    "signature" => credentials.authenticated = *value != 0,
                    "digest" => credentials.bound_capability_id = *value,
                    _ => return Err("unknown proof field".into()),
                }
            }
        }
    }
    Ok(report)
}

pub fn check_fixture(file: &ScheduleFile) -> Result<(), String> {
    for schedule in &file.schedules {
        let clean = run_schedule(schedule, Mutation::None)?;
        if !clean.violations.is_empty() {
            return Err(format!("{} clean: {:?}", schedule.name, clean.violations));
        }
        if clean.steps != schedule.steps.len() {
            return Err("discarded fixture steps".into());
        }
        if schedule.mutant != Mutation::None {
            let mutant = run_schedule(schedule, schedule.mutant)?;
            let class = schedule
                .expected_invariant
                .as_ref()
                .ok_or("missing mutant expected invariant")?;
            if !mutant.violations.iter().any(|error| error.contains(class)) {
                return Err(format!(
                    "{}: mutation {:?} not caught",
                    schedule.name, schedule.mutant
                ));
            }
        } else if schedule.expected_invariant.is_some() {
            return Err("invariant without mutation".into());
        }
    }
    Ok(())
}
