//! Simulation campaigns and mutation checks.
//!
//! `FLAMINIO_SIM_SEEDS` sets how many seeds the campaign runs (default 200), and
//! `FLAMINIO_SIM_FIRST` the first one. A failing seed reproduces with `run_seed(seed)`.

use super::*;

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
        runs += 1;
    }
    eprintln!(
        "{runs} runs: {} ops ({} completed), {} failovers, {} crashes, {} partitions, {} events",
        total.ops, total.completed, total.failovers, total.crashes, total.partitions, total.events
    );
}

fn scenario_config(nodes: u32, min_isr: usize) -> Config {
    Config {
        nodes,
        rf: 0,
        min_isr,
        chunks: 1,
        blocks_per_chunk: 2,
        timing: Timing {
            lease: 5 * SEC,
            drift_ppm: 0,
            heartbeat: 500 * MS,
            request_timeout: SEC,
            reconnect: 200 * MS,
            meta_timeout: 5 * SEC,
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

/// Runs seeds with a bug until one fails. Returns the failing seed and the failure.
fn find_failure(bugs: Bugs, max_seeds: u64) -> Option<(u64, String)> {
    (0..max_seeds).find_map(|seed| {
        Sim::new(seed, Config::random(seed))
            .with_bugs(bugs)
            .run()
            .err()
            .map(|e| (seed, e))
    })
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

// Each mutation test runs seeds until the bug shows (up to `FLAMINIO_MUTATION_SEEDS`,
// default 20000). The rarest need a few thousand seeds, so these run in release builds only:
// `make sim`.
macro_rules! mutation {
    ($name:ident, $bugs:expr) => {
        #[test]
        #[ignore = "slow in debug builds: run with `make sim`"]
        fn $name() {
            let budget = std::env::var("FLAMINIO_MUTATION_SEEDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20000);
            let found = find_failure($bugs, budget);
            let (seed, err) = found.expect("the simulator did not catch this bug");
            eprintln!(
                "{}: caught at seed {seed}: {}",
                stringify!($name),
                err.lines().next().unwrap_or("")
            );
        }
    };
}

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
mutation!(catches_late_join, target(|b| b.late_join = true));
mutation!(
    catches_reconciliation_pruning,
    target(|b| b.prune_reconcile = true)
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
