//! Simulation campaigns and mutation checks.
//!
//! `FLAMINIO_SIM_SEEDS` sets how many seeds the campaign runs (default 200), and
//! `FLAMINIO_SIM_FIRST` the first one. A failing seed reproduces with `run_seed(seed)`.

use super::*;
use crate::meta::{IsrChange, MAX_CHANGES_PER_REQUEST};
use crate::msg::MAX_DIGEST_BLOCKS;

fn advance_scenario(sim: &mut Sim, until: u64) {
    while sim
        .events
        .peek()
        .is_some_and(|Reverse((at, _, _))| *at <= until)
    {
        assert!(sim.step());
    }
    sim.now = until;
}

fn wait_scenario(sim: &mut Sim, what: &str, done: impl Fn(&Sim) -> bool) {
    let deadline = sim.now + 10 * SEC;
    while !done(sim) {
        assert!(
            sim.now < deadline,
            "{what}: {}",
            sim.fail("scenario deadline")
        );
        assert!(sim.step(), "no event while waiting for {what}");
    }
}

fn scheduled_isr_changes(sim: &Sim) -> Vec<&[IsrChange]> {
    sim.payloads
        .iter()
        .filter_map(|event| match event {
            Some(Ev::Deliver(Packet {
                body:
                    Body::MetaReq {
                        body: MetaRequest::ChangeIsr { changes, .. },
                        ..
                    },
                ..
            })) => Some(changes.as_slice()),
            _ => None,
        })
        .collect()
}

fn start_scenario(sim: &mut Sim, node: NodeId) {
    let MetaReply::Applied { view, .. } = sim.meta.handle(&MetaRequest::Attach { vol: VOL, node })
    else {
        panic!("scenario attach refused");
    };
    sim.start_target(node, view);
}

fn large_scenario_config(chunks: u32, min_isr: usize) -> Config {
    let mut cfg = scenario_config(3, min_isr);
    cfg.chunks = chunks;
    cfg.blocks_per_chunk = 1;
    cfg.max_ops = 4_000;
    cfg.chaos = 20 * SEC;
    cfg.timing.meta_timeout = 60 * SEC;
    cfg
}

fn large_recovery(chunks: u32, moves: bool) -> Sim {
    use Action::*;
    let mut script = vec![
        (0, Attach(0)),
        (200 * MS, Write(0, 0, 0)),
        (300 * MS, Write(0, chunks - 1, 0)),
        (SEC, Cut(0, 1)),
        // Interrupt metadata while the failed peer's many chunk removals are pending.
        (1100 * MS, Cut(0, META)),
        (1500 * MS, Write(0, chunks - 1, 0)),
        (4 * SEC, Heal(0, META)),
        (6 * SEC, Heal(0, 1)),
        (8 * SEC, Attach(2)),
        (9 * SEC, Write(2, chunks / 2, 0)),
        (10 * SEC, Read(0, chunks - 1, 0)),
        (12 * SEC, Read(2, chunks - 1, 0)),
    ];
    if moves {
        script.push((7 * SEC, Write(0, chunks / 2, 0)));
    }
    let mut cfg = large_scenario_config(chunks, 1);
    cfg.timing.meta_timeout = SEC;
    let mut sim = Sim::scripted(cfg, vec![vec![0, 1]; chunks as usize], script);
    if moves {
        for chunk in (0..chunks).step_by(2) {
            assert!(matches!(
                sim.meta.handle(&MetaRequest::MoveReplica {
                    vol: VOL,
                    chunk,
                    from: 1,
                    to: 2,
                }),
                MetaReply::Applied { .. }
            ));
        }
    }
    sim
}

#[test]
fn large_chunk_recovery_uses_production_metadata_batches() {
    for chunks in [65, 129] {
        for moves in [false, true] {
            let report = large_recovery(chunks, moves).run().unwrap();
            assert!(report.max_meta_batch > 0);
            assert!(report.max_meta_batch <= MAX_CHANGES_PER_REQUEST);
            assert!(report.completed >= chunks as usize);
            assert_eq!(report.final_epoch, 2);
        }
    }
}

#[test]
fn large_chunk_recovery_catches_unbounded_meta_batch() {
    let report = large_recovery(65, false)
        .with_bugs(target(|b| b.unbounded_meta_batch = true))
        .run()
        .unwrap();
    assert!(
        report.max_meta_batch > MAX_CHANGES_PER_REQUEST,
        "the emitted-size checker did not expose the original 65-chunk mutation"
    );
}

#[test]
fn simulator_refuses_oversized_isr_without_metadata_mutation() {
    let mut sim = Sim::scripted(
        large_scenario_config(65, 1),
        vec![vec![0, 1]; 65],
        Vec::new(),
    );
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "initially settled target", |s| {
        s.settled().is_some()
    });
    let before = sim.meta.volume(VOL).unwrap().clone();
    let changes = (0..65)
        .map(|chunk| IsrChange {
            chunk,
            expected_ver: before.chunks[chunk as usize].isr_ver,
            new_isr: BTreeSet::from([0]),
            new_replicas: None,
        })
        .collect();
    sim.deliver(Packet {
        from: 0,
        to: META,
        to_inc: 0,
        conn: None,
        body: Body::MetaReq {
            req: u64::MAX,
            body: MetaRequest::ChangeIsr {
                vol: VOL,
                epoch: before.epoch,
                changes,
            },
        },
    });
    assert_eq!(sim.meta.volume(VOL), Some(&before));
    assert!(
        !sim.payloads.iter().any(|event| matches!(
            event,
            Some(Ev::Deliver(Packet {
                body: Body::MetaRep { req: u64::MAX, .. },
                ..
            }))
        )),
        "oversized refusal fabricated an authoritative success reply"
    );
}

#[test]
fn simulator_counts_an_oversized_emission_even_when_network_drops_it() {
    let mut sim = Sim::scripted(
        large_scenario_config(65, 1),
        vec![vec![0, 1]; 65],
        Vec::new(),
    )
    .with_bugs(target(|b| b.unbounded_meta_batch = true));
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "initially settled target", |s| {
        s.settled().is_some()
    });
    let before = sim.meta.volume(VOL).unwrap().clone();
    sim.script_step(Action::Cut(0, META));
    sim.script_step(Action::Cut(0, 1));
    let at = sim.now + 2 * SEC;
    advance_scenario(&mut sim, at);
    assert_eq!(
        sim.report.max_meta_batch, 65,
        "a dropped oversized emission escaped the size checker"
    );
    assert!(
        scheduled_isr_changes(&sim).is_empty(),
        "proposal reached the service boundary"
    );
    assert_eq!(sim.meta.volume(VOL), Some(&before));
}

#[test]
fn large_recovery_learns_a_hidden_commit_after_reply_loss() {
    let mut cfg = large_scenario_config(129, 1);
    cfg.timing.meta_timeout = SEC;
    let mut sim = Sim::scripted(cfg, vec![vec![0, 1]; 129], Vec::new());
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "initially settled target", |s| {
        s.settled().is_some()
    });
    sim.script_step(Action::Cut(0, 1));
    wait_scenario(&mut sim, "first batch committed before its reply", |s| {
        s.meta.volume(VOL).unwrap().chunks[0].isr == BTreeSet::from([0])
    });
    assert_eq!(
        sim.meta
            .volume(VOL)
            .unwrap()
            .chunks
            .iter()
            .filter(|c| c.isr_ver == 1)
            .count(),
        64
    );
    assert!(
        sim.payloads.iter().any(|event| matches!(
            event,
            Some(Ev::Deliver(Packet {
                body: Body::MetaRep {
                    reply: MetaReply::Applied { .. },
                    ..
                },
                ..
            }))
        )),
        "no committed reply available to lose"
    );
    sim.script_step(Action::Cut(0, META));
    let at = sim.now + 500 * MS;
    advance_scenario(&mut sim, at);
    sim.script_step(Action::Heal(0, META));
    wait_scenario(
        &mut sim,
        "all batches recovered after the hidden commit",
        |s| {
            s.meta
                .volume(VOL)
                .unwrap()
                .chunks
                .iter()
                .all(|c| c.isr == BTreeSet::from([0]))
        },
    );
    assert!(
        sim.meta
            .volume(VOL)
            .unwrap()
            .chunks
            .iter()
            .all(|c| c.isr_ver == 1),
        "a hidden committed transition was applied twice"
    );
    sim.script_step(Action::Heal(0, 1));
    let report = sim.run_mut().unwrap();
    assert_eq!(report.max_meta_batch, MAX_CHANGES_PER_REQUEST);
}

/// A completion check stronger than final-value checking: the effective-quorum member has
/// not acknowledged this write, and its metadata removal/join is still deferred.
fn deferred_write_check(sim: &Sim, op: u64, member: NodeId) -> Result<(), &'static str> {
    let h = &sim.history[op as usize];
    if matches!(h.result, Some((_, OpResult::Written)))
        && sim.nodes[member as usize]
            .replica
            .block(VOL, h.chunk, h.ext.start)
            != h.write
    {
        return Err("deferred effective-quorum member lacks an acknowledged write");
    }
    Ok(())
}

fn deferred_shrink_scenario(early_ack: bool) -> (Sim, u64) {
    let mut sim = Sim::scripted(
        large_scenario_config(65, 1),
        vec![vec![0, 1]; 65],
        Vec::new(),
    )
    .with_bugs(target(|b| b.ack_before_shrink = early_ack));
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "initially settled target", |s| {
        s.settled().is_some()
    });
    sim.script_step(Action::Slow(0, META, 30 * SEC));
    sim.script_step(Action::Cut(0, 1));
    let at = sim.now + 2 * SEC;
    advance_scenario(&mut sim, at);
    assert!(
        scheduled_isr_changes(&sim)
            .iter()
            .any(|changes| changes.len() == 64 && changes.iter().all(|c| c.chunk != 64))
    );
    assert_eq!(
        sim.nodes[0]
            .target
            .as_ref()
            .unwrap()
            .pending_membership()
            .len(),
        65
    );
    let op = sim.submit(0, 64, Extent::new(0, 1), Some(42));
    let at = sim.now + 10 * MS;
    advance_scenario(&mut sim, at);
    assert!(sim.meta.volume(VOL).unwrap().chunks[64].isr.contains(&1));
    (sim, op)
}

#[test]
fn deferred_chunk_simulation_catches_ack_before_shrink() {
    let (mut fixed, op) = deferred_shrink_scenario(false);
    assert_eq!(deferred_write_check(&fixed, op, 1), Ok(()));
    assert!(fixed.history[op as usize].result.is_none());
    fixed.script_step(Action::Heal(0, 1));
    fixed.script_step(Action::Slow(0, META, 500));
    fixed.run_mut().unwrap();
    let (broken, op) = deferred_shrink_scenario(true);
    assert_eq!(
        deferred_write_check(&broken, op, 1),
        Err("deferred effective-quorum member lacks an acknowledged write")
    );
}

fn deferred_join_scenario(late_join: bool) -> (Sim, u64) {
    let mut sim = Sim::scripted(
        large_scenario_config(65, 1),
        vec![vec![0, 1]; 65],
        Vec::new(),
    )
    .with_bugs(target(|b| b.late_join = late_join));
    for chunk in 0..65 {
        assert!(matches!(
            sim.meta.handle(&MetaRequest::ChangeIsr {
                vol: VOL,
                epoch: 0,
                changes: vec![IsrChange {
                    chunk,
                    expected_ver: 0,
                    new_isr: BTreeSet::from([0]),
                    new_replicas: None,
                }],
            }),
            MetaReply::Applied { .. }
        ));
    }
    sim.script_step(Action::Slow(0, META, 30 * SEC));
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "65 completed resyncs waiting for metadata", |s| {
        let pending = s.nodes[0].target.as_ref().unwrap().pending_membership();
        pending.len() == 65 && pending.iter().all(|(_, joining, _)| *joining == Some(1))
    });
    assert!(
        scheduled_isr_changes(&sim)
            .iter()
            .any(|changes| changes.iter().all(|c| c.chunk != 64))
    );
    sim.script_step(Action::Cut(0, 1));
    let op = sim.submit(0, 64, Extent::new(0, 1), Some(42));
    let at = sim.now + 10 * MS;
    advance_scenario(&mut sim, at);
    assert_eq!(
        sim.meta.volume(VOL).unwrap().chunks[64].isr,
        BTreeSet::from([0])
    );
    (sim, op)
}

#[test]
fn deferred_chunk_simulation_catches_late_join() {
    let (mut fixed, op) = deferred_join_scenario(false);
    assert_eq!(deferred_write_check(&fixed, op, 1), Ok(()));
    assert!(fixed.history[op as usize].result.is_none());
    fixed.script_step(Action::Heal(0, 1));
    fixed.script_step(Action::Slow(0, META, 500));
    fixed.run_mut().unwrap();
    let (broken, op) = deferred_join_scenario(true);
    assert_eq!(
        deferred_write_check(&broken, op, 1),
        Err("deferred effective-quorum member lacks an acknowledged write")
    );
}

fn deferred_move_scenario(early_drop: bool) -> Sim {
    let mut sim = Sim::scripted(
        large_scenario_config(65, 2),
        vec![vec![0, 1]; 65],
        Vec::new(),
    )
    .with_bugs(target(|b| b.drop_before_join = early_drop));
    start_scenario(&mut sim, 0);
    wait_scenario(&mut sim, "initially settled target", |s| {
        s.settled().is_some()
    });
    let op = sim.submit(0, 64, Extent::new(0, 1), Some(42));
    wait_scenario(&mut sim, "write durable on original ISR", |s| {
        s.history[op as usize].result.is_some()
    });
    sim.script_step(Action::Cut(0, 2));
    for chunk in 0..65 {
        assert!(matches!(
            sim.meta.handle(&MetaRequest::MoveReplica {
                vol: VOL,
                chunk,
                from: 1,
                to: 2,
            }),
            MetaReply::Applied { .. }
        ));
    }
    start_scenario(&mut sim, 0);
    wait_scenario(
        &mut sim,
        "first 64 move additions leave chunk 64 deferred",
        |s| {
            scheduled_isr_changes(s)
                .iter()
                .any(|changes| changes.len() == 64 && changes.iter().all(|c| c.chunk != 64))
        },
    );
    let at = sim.now + 100 * MS;
    advance_scenario(&mut sim, at);
    sim
}

fn deferred_move_check(sim: &Sim) -> Result<(), &'static str> {
    let ch = &sim.meta.volume(VOL).unwrap().chunks[64];
    if !ch.replicas.contains(&1) && sim.nodes[2].replica.block(VOL, 64, 0) != Some(42) {
        return Err("move source removed before the deferred destination synchronized");
    }
    Ok(())
}

#[test]
fn deferred_chunk_simulation_catches_drop_before_join() {
    let mut fixed = deferred_move_scenario(false);
    assert_eq!(deferred_move_check(&fixed), Ok(()));
    assert!(
        fixed.meta.volume(VOL).unwrap().chunks[64]
            .replicas
            .contains(&1)
    );
    fixed.script_step(Action::Heal(0, 2));
    fixed.run_mut().unwrap();
    assert!(
        fixed
            .meta
            .volume(VOL)
            .unwrap()
            .chunks
            .iter()
            .all(|c| c.replicas == [0, 2] && c.isr.len() >= 2)
    );
    let broken = deferred_move_scenario(true);
    assert_eq!(
        deferred_move_check(&broken),
        Err("move source removed before the deferred destination synchronized")
    );
}

fn seeds() -> std::ops::Range<u64> {
    let var = |name, default| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let first = var("FLAMINIO_SIM_FIRST", 0);
    first..first + var("FLAMINIO_SIM_SEEDS", 200)
}

/// The campaign with resync probes of one block each, as the data plane splits a chunk larger
/// than [`MAX_MAPPED_BLOCKS`]: a resync starts only once every probe has answered.
#[test]
fn campaign_with_split_mapped_probes() {
    for seed in seeds() {
        let mut cfg = Config::random(seed);
        cfg.max_mapped = 1;
        if let Err(e) = Sim::new(seed, cfg).run() {
            panic!("seed {seed}: {e}");
        }
    }
}

/// The campaign with whole-chunk digest requests, as the data plane sends them.
#[test]
fn campaign_with_whole_chunk_digests() {
    for seed in seeds() {
        let mut cfg = Config::random(seed);
        cfg.max_digest = MAX_DIGEST_BLOCKS;
        if let Err(e) = Sim::new(seed, cfg).run() {
            panic!("seed {seed}: {e}");
        }
    }
}

#[test]
fn campaign() {
    let mut total = Report::default();
    let mut runs = 0;
    for seed in seeds() {
        let r = run_seed(seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        total.ops += r.ops;
        total.completed += r.completed;
        total.failovers += r.failovers;
        total.crashes += r.crashes;
        total.partitions += r.partitions;
        total.events += r.events;
        total.failed_local_reads += r.failed_local_reads;
        runs += 1;
    }
    eprintln!(
        "{runs} runs: {} ops ({} completed), {} failovers, {} crashes, {} partitions, {} events, \
         {} failed local reads",
        total.ops,
        total.completed,
        total.failovers,
        total.crashes,
        total.partitions,
        total.events,
        total.failed_local_reads
    );
}

fn bounded(seed: u64, budget: u32) -> Config {
    Config {
        uncertainty_budget: budget,
        ..more_chunks(seed)
    }
}

fn bounded_eight(seed: u64) -> Config {
    bounded(seed, 8)
}

fn bounded_sixteen(seed: u64) -> Config {
    bounded(seed, 16)
}

fn forged(seed: u64) -> Config {
    Config {
        forge_epoch_pct: 20,
        ..Config::random(seed)
    }
}

fn invalid_fenced(seed: u64) -> Config {
    Config {
        invalid_fenced_pct: 20,
        ..Config::random(seed)
    }
}

#[test]
fn small_uncertainty_budgets_cover_faults_and_repair() {
    for budget in [8, 16] {
        let mut total = Report::default();
        for seed in 0..200 {
            let r = Sim::new(seed, bounded(seed, budget))
                .run()
                .unwrap_or_else(|e| panic!("budget {budget}, seed {seed}: {e}"));
            total.uncertainty_checks += r.uncertainty_checks;
            total.coarsened_checks += r.coarsened_checks;
            total.restart_covered_reports += r.restart_covered_reports;
            total.accepted_write_attempts += r.accepted_write_attempts;
            total.repair_write_attempts += r.repair_write_attempts;
            total.uncertainty_prunes += r.uncertainty_prunes;
            total.uncertainty_settles += r.uncertainty_settles;
            total.crashes += r.crashes;
            total.partitions += r.partitions;
        }
        assert!(total.uncertainty_checks > total.accepted_write_attempts);
        assert!(total.coarsened_checks > 0, "budget {budget}: {total:?}");
        assert!(total.restart_covered_reports > 0, "{total:?}");
        assert!(total.repair_write_attempts > 0, "{total:?}");
        assert!(total.uncertainty_prunes > 0, "{total:?}");
        assert!(total.uncertainty_settles > 0, "{total:?}");
        assert!(total.crashes > 0 && total.partitions > 0, "{total:?}");
        eprintln!("200 bounded fault runs at B={budget}: {total:?}");
    }
}

#[test]
fn forged_epochs_leave_runs_clean() {
    let mut injected = 0;
    for seed in 0..200 {
        let r = Sim::new(seed, forged(seed))
            .run()
            .unwrap_or_else(|e| panic!("forged seed {seed}: {e}"));
        injected += r.forged_epoch_requests;
    }
    assert!(injected > 0);
    eprintln!("200 clean runs with {injected} forged-epoch requests at 20%");
}

#[test]
fn forged_epoch_resync_regressions_leave_runs_clean() {
    // Found while searching the old-acknowledgement mutation with pending replay enabled.
    // These are clean failures, not mutation catches; preserve them without excluding seeds.
    for seed in [1_833, 9_803, 14_081] {
        Sim::new(seed, forged(seed))
            .run()
            .unwrap_or_else(|e| panic!("forged resync seed {seed}: {e}"));
    }
}

#[test]
fn invalid_fenced_replies_leave_runs_clean() {
    let mut injected = 0;
    for seed in 0..200 {
        let r = Sim::new(seed, invalid_fenced(seed))
            .run()
            .unwrap_or_else(|e| panic!("invalid Fenced seed {seed}: {e}"));
        injected += r.invalid_fenced_replies;
    }
    assert!(injected > 0);
    eprintln!("200 clean runs with {injected} invalid Hello replies at 20%");
}

#[test]
fn invalid_fenced_grants_no_authority_before_retry() {
    let mut cfg = scenario_config(3, 1);
    cfg.uncertainty_budget = 8;
    cfg.invalid_fenced_pct = 100;
    let mut sim = Sim::scripted(cfg, vec![vec![0, 1]], Vec::new());
    start_scenario(&mut sim, 0);
    let authority = |s: &Sim| {
        let target = s.nodes[0].target.as_ref().unwrap();
        [target.fenced_authority(0), target.fenced_authority(1)]
    };
    let before = authority(&sim);
    advance_scenario(&mut sim, 2 * MS);
    assert!(sim.report.invalid_fenced_replies >= 2);
    assert_eq!(sim.check_failure, None);
    assert_eq!(authority(&sim), before);
    assert!(sim.settled().is_none());
    sim.cfg.invalid_fenced_pct = 0;
    sim.run_mut().unwrap();
}

#[test]
fn pending_writes_coarsen_then_restart_and_repair_settle() {
    for budget in [8, 16] {
        let mut cfg = scenario_config(3, 1);
        cfg.uncertainty_budget = budget;
        cfg.blocks_per_chunk = budget + 4;
        cfg.client_every = (MS, 30 * MS);
        let mut sim = Sim::scripted(cfg, vec![vec![0, 1]], Vec::new());
        start_scenario(&mut sim, 0);
        wait_scenario(&mut sim, "initially settled target", |s| {
            s.settled().is_some()
        });
        // The local ISR member accepts every attempt while the remote member cannot yet
        // acknowledge any. No completion proof can advance past this pending window.
        sim.script_step(Action::Slow(0, 1, 30 * SEC));
        let pending: Vec<_> = (0..budget + 2)
            .map(|block| sim.submit(0, 0, Extent::new(block, 1), Some(block as Value + 1)))
            .collect();
        let at = sim.now + 10 * MS;
        advance_scenario(&mut sim, at);
        assert_eq!(sim.check_failure, None);
        for &op in &pending {
            let h = &sim.history[op as usize];
            assert!(
                h.result.is_none(),
                "write completed before remote durability"
            );
            assert_eq!(sim.nodes[0].replica.block(VOL, 0, h.ext.start), h.write);
            assert_eq!(sim.nodes[1].replica.block(VOL, 0, h.ext.start), Some(0));
        }
        assert!(sim.report.coarsened_checks > 0, "B={budget}");
        assert_eq!(
            sim.nodes[0].replica.core.report(VOL),
            vec![(0, vec![Extent::new(0, budget + 4)])]
        );
        let settled_before: u64 = sim.nodes.iter().map(|n| n.uncertainty.stats.settles).sum();
        sim.crash(0);
        sim.restart(0);
        start_scenario(&mut sim, 2);
        let r = sim.run_mut().unwrap();
        assert!(r.restart_covered_reports > 0, "{r:?}");
        assert!(r.repair_write_attempts > 0, "{r:?}");
        assert!(r.uncertainty_settles > settled_before, "{r:?}");
        assert!(
            pending
                .iter()
                .all(|&op| sim.history[op as usize].result.is_none())
        );
        eprintln!("pending/restart/repair at B={budget}: {r:?}");
    }
}

#[test]
fn bounded_faults_repeat_the_same_history_and_checks() {
    for budget in [8, 16] {
        for seed in 0..20 {
            let mut cfg = bounded(seed, budget);
            cfg.forge_epoch_pct = 20;
            cfg.invalid_fenced_pct = 20;
            let (first, first_history) = Sim::new(seed, cfg.clone()).run_with_history();
            let (again, again_history) = Sim::new(seed, cfg).run_with_history();
            assert!(first.is_ok(), "B={budget}, seed {seed}: {first:?}");
            assert_eq!(format!("{first:?}"), format!("{again:?}"));
            assert_eq!(first_history, again_history, "B={budget}, seed {seed}");
        }
    }
}

fn scenario_config(nodes: u32, min_isr: usize) -> Config {
    Config {
        nodes,
        rf: 0,
        min_isr,
        chunks: 1,
        uncertainty_budget: crate::replica::UNCERTAINTY_BUDGET,
        blocks_per_chunk: 2,
        max_op: 1,
        max_io: 1,
        max_bulk: 4,
        max_mapped: MAX_MAPPED_BLOCKS,
        max_digest: 1,
        timing: Timing {
            lease: 5 * SEC,
            drift_ppm: 0,
            heartbeat: 500 * MS,
            request_timeout: SEC,
            reconnect: 200 * MS,
            meta_timeout: 5 * SEC,
            refresh: SEC,
        },
        chaos: 15 * SEC,
        settle_limit: 120 * SEC,
        max_ops: 1000,
        fault_every: 0,
        failover_pct: 0,
        spike_pct: 0,
        client_every: (0, 0),
        detect: (0, 0),
        read_pct: 0,
        move_pct: 0,
        fast_reads: false,
        local_read_fail_pct: 0,
        forge_epoch_pct: 0,
        invalid_fenced_pct: 0,
    }
}

/// Split brain: the target's node is cut off from everything but its own replica, and the
/// volume fails over. The old target keeps serving its client from the local replica, which
/// the new target cannot fence. Its reads must stop before the new target's writes complete:
/// the lease (confirmed by the replica it lost) and the takeover wait see to that.
fn split_brain() -> Sim {
    use Action::*;
    let script = vec![
        (0, Attach(0)),
        (500 * MS, Write(0, 0, 0)),
        (1000 * MS, Cut(0, 1)),
        (1000 * MS, Cut(0, 2)),
        (1000 * MS, Cut(0, META)),
        (1200 * MS, Attach(2)),
        (1500 * MS, Write(2, 0, 0)),
        // Within the old lease, before the new target may write: allowed, sees the old value.
        (2000 * MS, Read(0, 0, 0)),
        // After the old lease expired and the new write completed: must not return.
        (7000 * MS, Read(0, 0, 1)),
        (7000 * MS, Write(2, 0, 1)),
        (7500 * MS, Read(0, 0, 0)),
        (10 * SEC, Heal(0, 1)),
        (10 * SEC, Heal(0, 2)),
        (10 * SEC, Heal(0, META)),
    ];
    Sim::scripted(scenario_config(3, 1), vec![vec![0, 1]], script)
}

/// A failover while an ISR removal is committed but its reply has not reached the old target.
/// The new target sees an ISR without the removed member, fences all of it, and serves at
/// once. The old target, still holding a lease, must not read from the member it is
/// removing: that member is the one replica the new target did not fence.
fn removal_in_flight() -> Sim {
    use Action::*;
    let script = vec![
        (0, Attach(3)),
        (500 * MS, Write(3, 0, 0)),
        // The old target's metadata link becomes slow: its next ISR change is applied 2 s
        // after it is sent, and the reply takes 2 s more.
        (1000 * MS, Slow(3, META, 2 * SEC)),
        // Replica 2 is cut off from the old target (which proposes its removal) and from
        // node 0, the next target.
        (2000 * MS, Cut(3, 2)),
        (2000 * MS, Cut(0, 2)),
        // The old target reaches replica 2 again, and loses the others.
        (3500 * MS, Heal(3, 2)),
        (3600 * MS, Cut(3, 0)),
        (3600 * MS, Cut(3, 1)),
        (5500 * MS, Attach(0)),
        (5600 * MS, Write(0, 0, 0)),
        (6000 * MS, Read(3, 0, 0)),
        (12 * SEC, Heal(3, 0)),
        (12 * SEC, Heal(3, 1)),
        (12 * SEC, Heal(0, 2)),
        (12 * SEC, Slow(3, META, 500)),
    ];
    Sim::scripted(scenario_config(4, 1), vec![vec![0, 1, 2]], script)
}

#[test]
fn scenario_split_brain() {
    split_brain().run().unwrap();
}

#[test]
fn scenario_removal_in_flight() {
    removal_in_flight().run().unwrap();
}

/// The split brain again, with the old target's client reads on the fast path.
#[test]
fn scenario_split_brain_fast_reads() {
    let mut sim = split_brain();
    sim.cfg_mut().fast_reads = true;
    sim.run().unwrap();
    let mut sim = split_brain().with_bugs(target(|b| b.fast_read_no_lease = true));
    sim.cfg_mut().fast_reads = true;
    let err = sim.run().expect_err("bug not caught");
    assert!(err.contains("not linearizable"), "{err}");
}

#[test]
fn scenario_split_brain_catches_missing_lease_and_wait() {
    for bugs in [
        target(|b| b.no_lease = true),
        target(|b| b.no_takeover_wait = true),
    ] {
        let err = split_brain()
            .with_bugs(bugs)
            .run()
            .expect_err("bug not caught");
        assert!(err.contains("not linearizable"), "{err}");
    }
}

#[test]
fn scenario_removal_in_flight_catches_reads_from_removing() {
    let err = removal_in_flight()
        .with_bugs(target(|b| b.read_from_removing = true))
        .run()
        .expect_err("bug not caught");
    assert!(err.contains("not linearizable"), "{err}");
}

/// A configuration for each seed.
type Configure = fn(u64) -> Config;

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// Whether this run is a search (`scripts/dev/mutation-search.sh`): `FLAMINIO_MUTATION_FIRST`
/// set.
fn searching() -> bool {
    env_u64("FLAMINIO_MUTATION_FIRST").is_some()
}

/// Runs `seed` with `bugs`. Independent coverage, budget and authority failures are catches,
/// alongside linearizability/agreement. Other classes (`liveness:`, `event storm`) are counted
/// in `skipped` and passed over. Every failing seed must pass with no bug on: one that does not
/// is a bug in the protocol or the simulator, not a catch, and panics.
fn try_seed(bugs: Bugs, config: Configure, seed: u64, skipped: &mut Vec<u64>) -> Option<String> {
    let err = Sim::new(seed, config(seed)).with_bugs(bugs).run().err()?;
    if let Err(clean) = Sim::new(seed, config(seed)).run() {
        panic!(
            "seed {seed} fails with no bug on: fix it first.\nwith the bug: {}\nwith none: {clean}",
            err.lines().next().unwrap_or("")
        );
    }
    if [
        "not linearizable",
        "replicas diverge",
        "uncertainty not covered",
        "uncertainty budget exceeded",
        "invalid Fenced accepted",
    ]
    .iter()
    .any(|class| err.contains(class))
    {
        Some(err)
    } else {
        skipped.push(seed);
        None
    }
}

/// Runs seeds with a bug until one is caught, and prints the result. Without
/// `FLAMINIO_MUTATION_FIRST`, from seed 0 over `budget` (`FLAMINIO_MUTATION_SEEDS` overrides
/// it), and a bug not caught fails the test. With it, from that seed over
/// `FLAMINIO_MUTATION_SEEDS` (default `budget`), and the test passes either way: a search.
fn find_failure(name: &str, bugs: Bugs, config: Configure, budget: u64) {
    find_failure_class(name, bugs, config, budget, None);
}

fn find_failure_class(
    name: &str,
    bugs: Bugs,
    config: Configure,
    budget: u64,
    expected: Option<&str>,
) {
    let first = env_u64("FLAMINIO_MUTATION_FIRST").unwrap_or(0);
    let seeds = first..first + env_u64("FLAMINIO_MUTATION_SEEDS").unwrap_or(budget);
    let mut skipped = Vec::new();
    let found = seeds.clone().find_map(|seed| {
        let error = try_seed(bugs, config, seed, &mut skipped)?;
        if expected.is_none_or(|class| error.contains(class)) {
            Some((seed, error))
        } else {
            skipped.push(seed);
            None
        }
    });
    let skipped = match skipped.first() {
        Some(s) => format!(
            " ({} other failures passed over, the first at seed {s})",
            skipped.len()
        ),
        None => String::new(),
    };
    match found {
        Some((seed, err)) => eprintln!(
            "{name}: caught at seed {seed}: {}{skipped}",
            err.lines().next().unwrap_or("")
        ),
        None if searching() => eprintln!(
            "{name}: searched [{}, {}): no catch{skipped}",
            seeds.start, seeds.end
        ),
        None => panic!(
            "{name}: the simulator did not catch this bug in [{}, {}){skipped}",
            seeds.start, seeds.end
        ),
    }
}

/// [`Config::random`] with 3 to 8 chunks: chunks fenced and reconciled at different times, and
/// clients waiting on some of them (docs/issues/11).
fn more_chunks(seed: u64) -> Config {
    let mut c = Config::random(seed);
    c.chunks = 3 + (seed % 6) as u32;
    c
}

fn campaign_more_chunks(seeds: std::ops::Range<u64>) {
    let (mut runs, mut with_resync) = (0, 0);
    for seed in seeds {
        let r = Sim::new(seed, more_chunks(seed))
            .run()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        runs += 1;
        if r.waited_pumps_with_resync > 0 {
            with_resync += 1;
        }
    }
    eprintln!("{runs} runs, {with_resync} with a resync item queued while a chunk was waited");
}

#[test]
fn campaign_with_more_chunks() {
    campaign_more_chunks(seeds());
}

#[test]
#[ignore = "slow in debug builds: run with `make sim`"]
fn campaign_with_more_chunks_in_release() {
    campaign_more_chunks(0..2_000);
}

/// The pump with today's switch on gives the same runs wherever no chunk is ever waited
/// (docs/issues/11, S2): one-chunk runs, and runs with no client operation submitted while its
/// chunk was not `Ready`.
#[test]
fn todays_pump_gives_the_same_runs() {
    let configs: [Configure; 2] = [Config::random, more_chunks];
    let zeroed = |r: &Report| {
        let mut r = r.clone();
        r.waited_pumps = 0;
        r.waited_pumps_with_resync = 0;
        r.waiting_submits = 0;
        format!("{r:?}")
    };
    let (mut compared, mut waited) = (0, 0);
    for config in configs {
        for seed in seeds() {
            let cfg = config(seed);
            let chunks = cfg.chunks;
            let (new, new_history) = Sim::new(seed, cfg.clone()).run_with_history();
            let (today, today_history) = Sim::new(seed, cfg)
                .with_bugs(target(|b| b.today_pump = true))
                .run_with_history();
            if let Ok(r) = &new {
                assert!(
                    r.waiting_submits > 0 || r.waited_pumps == 0,
                    "seed {seed}: waited pumps with no client operation waiting: {r:?}"
                );
                if r.waited_pumps > 0 {
                    waited += 1;
                }
            }
            // Picked by today's run: the pump under test does not decide.
            let must_match = chunks == 1 || today.as_ref().is_ok_and(|r| r.waiting_submits == 0);
            if !must_match {
                continue;
            }
            compared += 1;
            assert!(
                new_history == today_history,
                "seed {seed} ({chunks} chunks): the histories differ"
            );
            match (&new, &today) {
                (Ok(a), Ok(b)) => assert_eq!(zeroed(a), zeroed(b), "seed {seed}: the reports"),
                (Err(a), Err(b)) => assert_eq!(a, b, "seed {seed}: the errors"),
                _ => panic!("seed {seed}: {new:?} with the new pump, {today:?} with today's"),
            }
        }
    }
    eprintln!("{compared} runs compared, {waited} with waited pumps");
}

fn target(f: impl FnOnce(&mut crate::target::Bugs)) -> Bugs {
    let mut b = Bugs::default();
    f(&mut b.target);
    b
}

fn replica(f: impl FnOnce(&mut crate::replica::Bugs)) -> Bugs {
    let mut b = Bugs::default();
    f(&mut b.replica);
    b
}

// Each mutation test runs seeds until the bug shows, up to a budget (`FLAMINIO_MUTATION_SEEDS`
// overrides it), on `Config::random` unless given a configuration. The rarest need thousands of
// seeds, so these run in release builds only: `make sim`. `scripts/dev/mutation-search.sh`
// finds each one's first catch.
macro_rules! mutation {
    ($name:ident, $bugs:expr) => {
        mutation!($name, $bugs, 20_000);
    };
    ($name:ident, $bugs:expr, $budget:expr) => {
        mutation!($name, $bugs, $budget, Config::random);
    };
    ($name:ident, $bugs:expr, $budget:expr, $config:expr) => {
        #[test]
        #[ignore = "slow in debug builds: run with `make sim`"]
        fn $name() {
            find_failure(stringify!($name), $bugs, $config, $budget);
        }
    };
    ($name:ident, $bugs:expr, $budget:expr, $config:expr, $class:expr) => {
        #[test]
        #[ignore = "slow in debug builds: run with `make sim`"]
        fn $name() {
            find_failure_class(stringify!($name), $bugs, $config, $budget, Some($class));
        }
    };
}

// Searched from seed 0 on 2026-11-27 with scripts/dev/mutation-search.sh (200 seeds,
// two ranges). The independent class and clean same-seed rerun are required by the harness:
// drop/coarse-prune/foreign-epoch/invalid-Fenced first catch at 0; budget bypass at 1.
mutation!(
    catches_drop_on_coarsen,
    replica(|b| b.drop_on_coarsen = true),
    200,
    bounded_eight,
    "uncertainty not covered"
);
mutation!(
    catches_coarse_prune_early,
    replica(|b| b.coarse_prune_early = true),
    200,
    bounded_eight,
    "uncertainty not covered"
);
mutation!(
    catches_bypass_uncertainty_budget,
    replica(|b| b.bypass_uncertainty_budget = true),
    200,
    bounded_sixteen,
    "uncertainty budget exceeded"
);
mutation!(
    catches_accept_foreign_epoch,
    replica(|b| b.accept_foreign_epoch = true),
    200,
    forged,
    "uncertainty not covered"
);
mutation!(
    catches_accept_invalid_fenced,
    target(|b| b.accept_invalid_fenced = true),
    200,
    invalid_fenced,
    "invalid Fenced accepted"
);
// Searched [0, 200), in two ranges on 2026-11-27. First catch: seed 4, with a clean
// same-seed rerun required by try_seed; omitting replay admits an older snapshot value.
mutation!(
    catches_skipped_pending_resync_replay,
    target(|b| b.skip_pending_resync_replay = true),
    200,
    forged,
    "not linearizable"
);

mutation!(
    catches_ack_before_shrink,
    target(|b| b.ack_before_shrink = true)
);
mutation!(catches_reads_without_lease, target(|b| b.no_lease = true));
mutation!(
    catches_no_takeover_wait,
    target(|b| b.no_takeover_wait = true)
);
mutation!(catches_no_reconciliation, target(|b| b.no_reconcile = true));
mutation!(
    catches_trust_single_digest,
    target(|b| b.trust_single_digest = true)
);
mutation!(catches_late_join, target(|b| b.late_join = true));
// Rare by nature: a target must die between two repairs that are usually sent together.
mutation!(
    catches_reconciliation_pruning,
    target(|b| b.prune_reconcile = true),
    60_000
);
// Rare like pruning: a target must die after settling and before its repairs reach a replica.
mutation!(
    catches_early_settle,
    target(|b| b.early_settle = true),
    60_000
);
mutation!(
    catches_dirty_set_missing_inflight,
    target(|b| b.no_dirty_inflight = true)
);
mutation!(
    catches_old_sessions_accepted,
    replica(|b| b.any_session = true)
);
mutation!(
    catches_copies_over_live_writes,
    replica(|b| b.no_resync_filter = true)
);
mutation!(
    catches_move_without_resync,
    target(|b| b.move_without_resync = true)
);
mutation!(
    catches_drop_before_join,
    target(|b| b.drop_before_join = true)
);
mutation!(
    catches_fast_reads_without_lease,
    target(|b| b.fast_read_no_lease = true)
);
// Rare too: a move must drop and re-add the local replica during one slow device read. First
// caught at seed 18962 (2026-10-28; searched again from seed 0 on 2026-11-04, docs/issues/11:
// unchanged).
mutation!(
    catches_fast_reads_from_a_replaced_replica,
    target(|b| b.fast_read_ignores_membership = true),
    60_000
);
// The rarest: a slow device read must straddle a complete takeover. Searched from seed 0 on
// 2026-11-04 (docs/issues/11): first caught at seed 103223, a two-chunk run that the order of
// reconciliation (§6 step 3) made catch; the one-chunk seeds 1001529 and 3626767 catch as before.
// `make sim` tries the hint only; a search (`FLAMINIO_MUTATION_FIRST`, with
// `FLAMINIO_MUTATION_NO_HINT=1` to skip the hint) covers a range, as for the other checks.
#[test]
#[ignore = "slow in debug builds: run with `make sim`"]
fn catches_fast_reads_across_a_fence() {
    hinted(
        "catches_fast_reads_across_a_fence",
        target(|b| b.fast_read_no_fence_check = true),
        103_223,
        4_000_000,
    );
}

// S11: a move drops and recreates a replica while an old Write Ack is delayed. Pending
// replay stays enabled; forged epochs can drop its new send, making the old evidence unsafe.
mutation!(
    catches_keep_dropped_acks,
    target(|b| b.keep_dropped_acks = true),
    20_000,
    forged,
    "not linearizable"
);

/// A check whose first catch is too far for `make sim` to search: it tries `hint` only, and
/// fails if that no longer catches. A search (`FLAMINIO_MUTATION_FIRST`) tries the hint first,
/// unless `FLAMINIO_MUTATION_NO_HINT=1`, then its range, over `budget` seeds by default.
fn hinted(name: &str, bugs: Bugs, hint: u64, budget: u64) {
    let no_hint = searching() && std::env::var("FLAMINIO_MUTATION_NO_HINT").as_deref() == Ok("1");
    if !no_hint {
        let mut skipped = Vec::new();
        if let Some(err) = try_seed(bugs, Config::random, hint, &mut skipped) {
            eprintln!(
                "{name}: caught at seed {hint}: {}",
                err.lines().next().unwrap_or("")
            );
            return;
        }
        assert!(
            searching(),
            "{name}: the hint {hint} no longer catches: search again (docs/issues/11, the search)"
        );
    }
    find_failure(name, bugs, Config::random, budget);
}

// The checks on the campaign with more chunks (docs/issues/11).
mutation!(
    catches_no_reconciliation_more_chunks,
    target(|b| b.no_reconcile = true),
    20_000,
    more_chunks
);
mutation!(
    catches_trust_single_digest_more_chunks,
    target(|b| b.trust_single_digest = true),
    20_000,
    more_chunks
);
mutation!(
    catches_early_settle_more_chunks,
    target(|b| b.early_settle = true),
    20_000,
    more_chunks
);
mutation!(
    catches_dirty_set_missing_inflight_more_chunks,
    target(|b| b.no_dirty_inflight = true),
    20_000,
    more_chunks
);
mutation!(
    catches_move_without_resync_more_chunks,
    target(|b| b.move_without_resync = true),
    20_000,
    more_chunks
);
mutation!(
    catches_copies_over_live_writes_more_chunks,
    replica(|b| b.no_resync_filter = true),
    20_000,
    more_chunks
);
