//! Simulation campaigns and mutation checks.
//!
//! `FLAMINIO_SIM_SEEDS` sets how many seeds the campaign runs (default 200), and
//! `FLAMINIO_SIM_FIRST` the first one. A failing seed reproduces with `run_seed(seed)`.

use super::*;
use crate::msg::MAX_DIGEST_BLOCKS;

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

fn scenario_config(nodes: u32, min_isr: usize) -> Config {
    Config {
        nodes,
        rf: 0,
        min_isr,
        chunks: 1,
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

/// Runs `seed` with `bugs`. A failure that is a catch (`not linearizable`, `replicas
/// diverge`) is returned; one of another class (`liveness:`, `event storm`) is counted in
/// `skipped` and passed over. Every failing seed must pass with no bug on: one that does not is
/// a bug in the protocol or the simulator, not a catch, and panics.
fn try_seed(bugs: Bugs, config: Configure, seed: u64, skipped: &mut Vec<u64>) -> Option<String> {
    let err = Sim::new(seed, config(seed)).with_bugs(bugs).run().err()?;
    if let Err(clean) = Sim::new(seed, config(seed)).run() {
        panic!(
            "seed {seed} fails with no bug on: fix it first.\nwith the bug: {}\nwith none: {clean}",
            err.lines().next().unwrap_or("")
        );
    }
    if err.contains("not linearizable") || err.contains("replicas diverge") {
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
    let first = env_u64("FLAMINIO_MUTATION_FIRST").unwrap_or(0);
    let seeds = first..first + env_u64("FLAMINIO_MUTATION_SEEDS").unwrap_or(budget);
    let mut skipped = Vec::new();
    let found = seeds
        .clone()
        .find_map(|seed| try_seed(bugs, config, seed, &mut skipped).map(|e| (seed, e)));
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
    const NAME: &str = "catches_fast_reads_across_a_fence";
    let bugs = target(|b| b.fast_read_no_fence_check = true);
    let hint = 103_223u64;
    let no_hint = searching() && std::env::var("FLAMINIO_MUTATION_NO_HINT").as_deref() == Ok("1");
    if !no_hint {
        let mut skipped = Vec::new();
        if let Some(err) = try_seed(bugs, Config::random, hint, &mut skipped) {
            eprintln!(
                "{NAME}: caught at seed {hint}: {}",
                err.lines().next().unwrap_or("")
            );
            return;
        }
        assert!(
            searching(),
            "{NAME}: the hint {hint} no longer catches: search again (docs/issues/11, the search)"
        );
    }
    find_failure(NAME, bugs, Config::random, 4_000_000);
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
