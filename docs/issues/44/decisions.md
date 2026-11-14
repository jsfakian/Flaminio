# Issue #44: Dirty-region resync — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

1. **Measure first, or build the durable record now:** measure first. `spec.md`, Open questions
   1; every rule assumes it.
2. **When the record (issue step 1) is worth building:** (a), by a budget set now: only if the
   replaced-target run's extra cost, scaled per GiB held to one 4 TiB disk, passes 10 minutes or
   1% of the data held; otherwise step 1 is dropped, and ARCHITECTURE §8.3 and §11.4 describe
   digests. `spec.md`, Open questions 2.
3. **`ChangeIsr` with more than 64 changes:** (a), its own issue, with a failing test first; this
   check stays at 64 chunks. `spec.md`, Open questions 3 and S7.
4. **The order of #45 and this issue:** #45 depends on this step only; step 3 (maintenance mode)
   follows #45. `spec.md`, Open questions 4.
5. **Blocks that differ under equal CRC-32C values:** (a), accepted for now, and noted on #46.
   `spec.md`, Open questions 5; `design.md`, Risks, ADRs and parallel work.

## Choices

Settled by the plan reviews. Each line: the choice, why, and the finding that raised it.

- **The check reports the bytes the member sent as well as received** (R1-B1). The target is
  on T, beside the source's replica: the member's digests are replies it sends, 5 bytes per
  block the source maps (R2-N3), and the source's cross no network. What it receives beyond
  the copies is the `Digest` requests and the `Discard`s. The byte difference of the two runs
  is taken each way.
- **Only the bytes received have a limit: W plus 32 MiB** (R1-B1). From the frame encoding and
  the transport's overhead on W, not from the 46 MiB of follow-ups §3: that is what fl-a
  received in a run whose target's node was not recorded, not a figure for digests.
- **The check refuses unless T and the member are the only alive, non-draining, non-witness
  nodes** (R1-C1, R2-L5). `Place` spreads second replicas over every such node; a chunk on
  fl-c leaves a tombstone the check cannot clear. The placement is still inspected after the
  create.
- **The summary line has two seams** (R1-C2): a pure `resync_summary_line` in
  `flaminio-node/src/engine.rs`, tested for its exact text, and `Engine::set_log`, a line sink
  the engine test sets. Only the summary goes through the sink; the other `eprintln!` sites stay.
- **The time compared between runs is the summary's `elapsed_us`** (R1-C3). The wall interval
  is polled every 0.5 s by a watcher on fl-a and includes the member's start: it is reported
  beside it, never subtracted.
- **The summary is evaluated after every applied view, for each member with counted rounds**
  (R1-C4), not only after a join: a view can remove its last replica outside an ISR. A member
  that holds no chunk gets no summary, and its totals are dropped.
- **S12 lists the refusals, each a command the script runs** (R1-C5): the nodes, the data
  plane running, `status` answering `ok none`, the sweep's process, the alive nodes, no volume
  attached elsewhere with a replica on either. "Exclusive lab use" is advice in the lab
  section, not a check.
- **The line carries `epoch`; `valid`, its reason and u128 are dropped** (R1-C6). The script
  picks the run's summary by the epoch. Block counts fit u64, and nothing could make an
  invalid summary happen in a test.
- **The check computes its own figures** (R1-N5), in `dirty-resync.sh`. `scripts/tools/summarize.py`
  reads fio and flaminio-bench JSON.
- **T is fl-a and the member fl-b: not parameters** (R2-L4). The watcher needs the administrator's
  key, which is on fl-a, and the summary is in T's journal: with T fixed, both are on T.
- **A value read from a remote reply is matched before any use** (R2-L2): a volume id by
  `valid_id`, the token, the epoch and the device path by their shape. A `status` answer that
  is neither `ok none` nor `ok V /dev/ublkbN` refuses: it is not skipped.
- **The cleanup deletes only a volume the run created** (R2-L3). The run refuses when its id
  exists, and sets its trap only after `create` succeeded: before that it has changed nothing.
- **The node filter defaults absent fields** (R2-L1): the client prints no false boolean, so the
  filter reads `alive` and `draining` with `// false`, and the fixtures have that shape.
- **The refusal for fl-c stays beside the one on alive nodes** (R2-L5): it covers a node that is
  draining, or that runs with a lapsed liveness record.
- **One time limit, 300 s** (R2-N5), replacing 1800 s: the allowance's slack of about 22 MiB is
  sized for minutes of the overlay's other traffic. Neither is tuned after a failure.
- **No Makefile target and no `lab.sh` command for the check** (R2-N1): it is run directly, as the
  other checks. The Makefile gains the script test's line in `lint`.
- **The check takes `metactl`, `data_vms` and `valid_id` out of `lab.sh` as the other checks
  do** (R3-L1), and its script test runs that extraction: a missing helper would fail only in
  the lab.
- **A failing `metactl get` of a volume attached on fl-c refuses** (R3-L2): what cannot be
  looked up is not assumed harmless.
- **A lost `create` answer prints `VOL=<id> make vol-delete`** (R3-N2): the volume may exist,
  the run does not own it, and an operator restores the lab.
- **The 300 s run from the watcher's first line** (R3-N3), counted by the watcher on T's clock.
- **The 300 s are bounded in the watcher** (R4-N1, round 5): each poll gets a `timeout 10`,
  new in this check, and the clock is read after every poll, so the watcher ends by itself and
  the host's wait on its ssh ends with it. ssh's keepalives, which `lib.sh` already has, only
  end a dead connection: that run is invalid (spec S11), and the cleanup does not depend on
  the watcher. No host-side timeout is added.
