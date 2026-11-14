# Issue #44: Dirty-region resync measurement first — design

How the existing [spec](spec.md)'s S1–S15 are built; [intent](intent.md);
[decisions](decisions.md). Approved 2026-11-13; not implemented. This is a measurement-first slice, not
implementation of durable dirty records, selective ISR removal or maintenance mode.
**Issue #44 stays open.**

The facts it starts from:
- `Resync` has round/member/session, todo/discard/live and pending acknowledgements
  (`flaminio-repl/src/target.rs:156`). `drive_resync` chooses dirty or whole and probes Mapped
  (`:2308`); `on_mapped` limits the plan and `on_digest` queues differing blocks (`:1445,1487`).
- Copy/discard acknowledgements remove extents (`:977–989`); `abort_resync` preserves dirty
  work (`:1288`). A finished round becomes joining before metadata commits (`:2327`);
  `apply_view` recognizes the committed join (`:2547`). Counting must distinguish those stages.
- `Event` has only SessionFailed (`:132`); the engine's drain prints that variant with
  `eprintln!` (`flaminio-node/src/engine.rs:1768-1773`). Neither the engine nor its tests
  (`flaminio-node/tests/engine.rs`, in-memory stores) can read what was logged.
- Digests travel to the target. It asks the source and the member (`:1861-1880`) for the
  blocks of the plan that the source maps (`on_mapped`, `:1458-1461`); each answers
  `Response::Digests` (`:948`), 5 bytes per block (`flaminio-net/src/frame.rs:306-318`; an
  unwritten block gets the zero block's CRC, `flaminio-store/src/store.rs:1288-1328`). The
  source is the target's own node when that is in the ISR (`source`, `:1644-1653`): T in the
  check. So only the member's digests cross the overlay, and the member *sends* them. Beyond
  the copies it *receives* the `Digest` requests and one `Discard` (25 bytes with its length
  prefix, `frame.rs:1,279-284`) per `max_io` = 256 blocks of hole (`:966`,
  `flaminio-io/src/node.rs:330`).
- `Place` spreads the replicas after the first over every alive, non-draining, non-witness
  node (`meta/domain/domain.go:354-358`, `meta/service/service.go:397-402`). A deletion
  pending for fl-c, which has no sweep, stops the lab's wait (`scripts/lab.sh:1782-1786`).
- Existing `scripts/checks/resync.sh` measures a move into an empty replica and remains
  unchanged. `node.sh` supplies attach/detach/delete and non-formatting up/down; it validates
  the volume id for detach and delete only (`volume_id`, `scripts/guest/node.sh:102-121`), so
  the check validates its own (`valid_id`, `scripts/lab.sh:1633`). `node.sh down` deletes the
  node's ublk device (`:23-38`). The control socket answers `status` with `ok none` or
  `ok VOLUME /dev/ublkbN` (`flaminio-io/src/control.rs:104-107`); it offers no local-copy
  diagnostic; do not add one just for counts.
- The check stays at 64 chunks: a larger volume may be unable to change its ISRs (intent, Why 7).

## Round accounting (S1, S2, S5, S6)

Add observer-only `ResyncPlan::{Dirty,Whole}`, `RoundCounts` and per-member `ResyncTotals` to
`target.rs`. Counts are blocks, in u64. The elapsed time is a saturating difference of the
target's clock: it cannot panic or alter protocol flow. Keep stats outside
all readiness, retry, queue and send decisions. Test-only observer disable supports comparison.

- At `drive_resync`, record plan origin and union length of initial todo, including empty dirty
  plans. Start duration at that first round; do not count subsequent Mapped filtering as planned.
- At `on_digest`, only once both source/member digest replies satisfy the existing request and
  round identity checks, add `d.ext.len` to compared. Repeated comparison work counts again;
  duplicate/stale acknowledgements rejected by the existing inflight map do not count twice.
- At accepted Copy/Discard Ok, add the original acknowledged extent length, even if the member's
  resync filter skipped part. Unacknowledged sends and late replies after abort contribute zero.
- Retain an observer record when `drive_resync` drops active resync into joining; close it only
  when `apply_view` commits the join. Its last plan origin supplies that chunk's joined count.
- Every other round termination counts as abort: explicit abort, failed probe/session and
  removal of the replica in `adopt_replicas`. Hook existing branches without rerouting them.
  Carry their counts into totals; later rounds never erase earlier acknowledged work.
- A detached/deposed target drops all unreported observer state and pending summary events.
  No synthetic partial summary, durable record or retry survives the target.

Keep one active/pending observer record per protocol round/chunk and one totals entry per
member, made at its first counted round; never retain all historical rounds or payloads. No
replication decision reads observer state.

## Committed summary and node log (S2–S6)

After applying an entire metadata view (`apply_view`, its `adopt_replicas` included), look at
every member that has a totals entry, whether or not it joined in this view:
- **It holds nothing** (no chunk's replica set contains it): drop its totals, emit nothing.
- **It is in the ISR of every chunk whose replica set contains it:** emit once and drop its
  totals. That is its last join, or a view removing its last replica outside an ISR: a move's
  source is not resynced (`:2334-2341`) and `adopt_replicas` removes it (`:2609-2641`).
  `elapsed_us` runs from its first counted round to this view.
- **Otherwise** wait. Never summarize at a joining proposal.

Store joined chunk IDs with their last round's origin; repeated joins of one chunk count that
chunk once in this summary. Moves use the same observer. Aborted counts remain across retries.

Add to `target.rs` `ResyncSummary { node, epoch, planned, compared, copied, discarded,
joined_chunks, dirty_chunks, aborted_rounds, elapsed_us }` (all u64 but `node`; `epoch` is the
target's) and `Event::ResyncSummary(ResyncSummary)`. In `flaminio-node/src/engine.rs`:
- **The line** comes from a pure
  `pub fn resync_summary_line(volume: &str, member: &str, s: &ResyncSummary) -> String`:
  `flaminio-node: resync_summary volume=V member=N epoch=E planned_blocks=P compared_blocks=D copied_blocks=C discarded_blocks=X joined_chunks=J dirty_chunks=K aborted_rounds=A elapsed_us=U`
- **The sink:** `Engine::set_log(&mut self, sink: Box<dyn FnMut(&str) + Send>)`, set as
  `set_arrivals` is (`:906`); by default it prints with `eprintln!`. `drain_one` matches both
  variants, resolves the member in the existing node-name table, and gives the line to the
  sink. The other `eprintln!` sites, SessionFailed's included, stay as they are.

IDs already obey metadata validation; no payload/token is logged. This event/log is
observational, never a wire response, metric or control-socket summary. Logging stays outside
store commits and protocol decisions.

## New check and workload (S7–S9)

Add `scripts/checks/dirty-resync.sh`; run only by explicit lab request. It sources
`scripts/lib.sh` and takes `metactl`, `data_vms` and `valid_id` out of `lab.sh`, which runs
commands when sourced, with `sed -n` as `reconcile.sh:15` and `resync.sh:12` do:
`/^metactl()/,/^}/p`, `/^data_vms()/,/^}/p` and, `valid_id` being one line, `/^valid_id()/p`.
What they use (`vssh`, `vmvar`, `vms_of`, `META_PORT`) comes with `lib.sh`. T is `$VM_A` (fl-a,
which holds the administrator's key, `scripts/lab.sh:529`) and MEMBER `$VM_B` (fl-b): neither
is a parameter, so the watcher, the journal cursor and the summary below are all on T. Quote
every argument. A value read from a remote reply is matched before it reaches a command or
socket line, and a mismatch stops the run: a volume id by `valid_id`, the token as base64, the
epoch as digits, the device as `/dev/ublkbN`.
Before it creates or stops anything it refuses, each with its own message, unless (S12):
1. on T and MEMBER: `systemctl is-active flaminio-node`, the control socket answers `status`
   with exactly `ok none`, and `pgrep -x flaminio-csi` finds the sweep (as
   `scripts/guest/confirm.sh:38-39`);
2. `metactl nodes` lists no other node with `(.alive // false)`, without `(.draining // false)`
   and without `(.labels // {} | has("flaminio.io/witness"))` (as `scripts/lab.sh:614`): the
   candidates of `Place`. The client prints no false field (`EmitUnpopulated: false`,
   `meta/cmd/flaminio-meta/main.go:501`);
3. fl-c, when it exists (`data_vms`) and its `flaminio-node` is active, answers `status` with
   `ok none`, or with `ok V /dev/ublkbN` for a V that passes `valid_id` and whose `metactl get`
   has neither T nor MEMBER in a replica set. Any other answer, or a failing `metactl get V`,
   refuses. This covers what 2
   lets through: a node that is draining, or that runs with a lapsed liveness record;
4. before each run's create, `metactl get` of its volume id answers NotFound (as
   `scripts/lab.sh:1757-1760`).

Record binary hashes/commit, node states and service invocation/journal cursors before changes.
No formatting, VM suspend, network partition or disk rollback occurs.

Each run uses a unique validated `dirty-resync-...` volume, 16 GiB/256 MiB chunks, RF2/minISR1.
Prefer T; inspect metadata and require all 64 replica sets exactly {T,MEMBER} (a node that
came alive since refusal 2 fails the run here). Attach on T.
Create deterministic-offset random payload files plus SHA-256 expected manifests, never reused
from another run. Baseline: 64 MiB at each chunk's start (4 GiB total), complete durable writes,
then await every ISR full and all baseline work finished before stopping MEMBER's data plane.
Wait for all 64 committed ISRs to exclude MEMBER before writing W, eliminating baseline work
from this target's dirty accounting. Stop on any unexpected metadata/protocol error.

W=128 MiB=32,768 blocks: in chunks 0…31, overwrite 2 MiB at offset 8 MiB and write 2 MiB at
previously unwritten offset 128 MiB. Chunks 32…63 receive no new writes; W≤baseline/16.
Write chunks 0…15, then 16…31. Same-target mode leaves T attached throughout. Replaced-target
mode detaches T between halves, attaches the same volume on T through metadata for a strictly
new epoch/token, and waits for ordinary takeover readiness before its second half. Never
reuse the old token or shorten the full-ISR/wait/reconciliation gates.

After all W is durable and no writes remain, start a watcher on T in one `vssh … bash -s`, as
`reconcile.sh:40-41` times on NODE. It prints its `date +%s.%N`, calls `flaminio-meta get` every
0.5 s until all 64 ISRs hold both nodes, and prints its clock again. After its first line,
read MEMBER's `rx_bytes` and `tx_bytes` (`/sys/class/net/wg0/statistics/`) in one command and
start MEMBER with FORMAT=0; after its second, read both again. The journal cursor and the
measured target's epoch identify exactly one summary for this run/member; allow it 10 s to
flush before verification.
- **Bytes:** `rx_delta` (copies, Discards, requests) and `tx_delta` (the member's digests and
  acknowledgements), each reported.
- **Resync time:** the summary's `elapsed_us`: the time figure of S13 and S14.
- **Wall interval:** the watcher's two instants. It holds the ssh, the node's start and up to
  0.5 s of polling: reported beside `elapsed_us`, never subtracted between runs.

A negative delta, counter wrap, command failure, or ISRs not full within 300 s of the
watcher's first line (the watcher counts them, on T's clock) invalidates the run: the
allowance below is sized for minutes of the overlay's other traffic. Neither limit is tuned
after a failure. What bounds it:
- **In the watcher:** each poll is `sudo timeout 10 flaminio-meta … get`. The `timeout` is new
  in this check: no `flaminio-meta` call under `scripts/` has one (the VMs have the command:
  `scripts/checks/reconcile.sh:57` puts it on a `dd`). A poll that fails or is cut is polled
  again. The loop sleeps 0.5 s between polls and reads T's clock after every poll: it exits
  non-zero once 300 s have passed, so about 310 s after its first line (300 s, one sleep and
  one poll of at most 10 s; process start-up is not counted).
- **On the host:** the wait is the watcher's ssh, bounded by the watcher's own limit. The
  keepalives only end a dead connection, with 255 after 120 s without an answer
  (`ServerAliveInterval=15`, `ServerAliveCountMax=8`, `scripts/lib.sh:28-29`). An ssh that ends
  without the second line fails the run as invalid (S11), like any command failure; it is not
  a run over the limit. The watcher may then go on, on T, until its own limit: it only reads
  (`get`, its clock), so the cleanup neither waits for it nor depends on it.

## Verification, budget and restoration (S10–S13)

Verify all 16 GiB through T in aligned chunks against an expected sparse image/hash manifest:
baseline with overwritten bytes, both halves of W, and every never-written range as zeros.
This covers holes exhaustively, not a few samples.

After capture and target verification, detach T, stop only T's data plane, and freshly attach
on MEMBER at a newer metadata epoch. T remains unavailable until verification finishes, so
its replica cannot serve or repair these reads. Wait for ordinary removal/reconciliation with
MEMBER as sole reachable ISR; verify the same complete expected image. Failure or need for T
is failure, not permission to restart it early. This post-capture verification is not part of
the measured resync totals. Then detach MEMBER and restart T non-formatting during cleanup.

Fail for S10 mismatch, a missing or extra summary, one of another epoch than the measured
target's, aborted_rounds>0, copied_blocks>32,768, or joined_chunks!=64. Same-target requires
dirty_chunks=64 and planned_blocks≤32,768; replaced-target requires dirty_chunks=0. Capture
summaries only through the measured endpoint, so verification's new target cannot contaminate
the result.

Network allowance on what MEMBER receives is **32 MiB** (W/4), fixed before running, from the
encoding, not from a past resync: about 8 MiB of transport overhead on W (follow-ups §3: 4 356
MiB received for 4 096 MiB copied, 6.3%); 0.3 MiB of frames for a whole plan's 12 224
Discards (the 12 224 MiB T does not map, in 256-block pieces of 25 bytes), under 2 MiB even
at 150 bytes each on the wire, its Digest requests included; the remaining 22 MiB for
sessions, heartbeats and the overlay's other traffic over at most 300 s. Require
rx_delta≤W_bytes+32 MiB (=160 MiB). `tx_delta` has no limit: by the encoding a whole plan's
digests are about 5.1 MiB (5 bytes × the 1 064 960 blocks of it T maps, 4 160 MiB), which the
runs confirm or not. An allowance failure records a failed run and requires explanation, never
an automatic larger allowance. wg0 also carries etcd, metadata and Kubernetes traffic: the raw
counters are kept, and S13's spread shows a disturbed run.

Trap EXIT/INT/TERM only once `create` has returned success for the run's id: before that the
run has changed nothing, and the cleanup can only delete a volume this run created. A `create`
that fails (AlreadyExists: the id, or a tombstone of it) or whose answer is lost stops the run
and deletes nothing. A lost answer may leave the volume: the message prints
`VOL=<id> make vol-delete` for an operator, as the fl-c case prints `vol-confirm`. The trap
always attempts detach on both nodes, starts both data planes FORMAT=0 if needed, deletes
metadata, explicitly deletes only this volume's replicas on both nodes and waits for
sweep/tombstone completion. Never confirm a tombstone.
Preserve the original failure and report cleanup failures as failure with pending actions;
never claim restoration when ssh/service state is unknown. Remove only owned payload files.
Run order S,R,S,R,S,R on the same binaries/day, each with a fresh volume and complete cleanup.

## Results, tests and files (S1–S15)

Add `scripts/tools/dirty-resync-test.sh` to `make lint`; fake commands, VMs, clocks, journal and
rx and tx counters. Keep new scripts out of ordinary CI lab execution. Rust crate paths below are
under `dataplane/crates/`.

Files touched:
- `flaminio-repl/src/target.rs`: the observer, `ResyncSummary`, the event, unit tests (S1–S3, S5).
- `flaminio-repl/src/sim/`: the checker and the observer twins (S1, S6).
- `flaminio-node/src/engine.rs`, `flaminio-node/tests/engine.rs`: the line, the sink (S4, S5).
- `scripts/checks/dirty-resync.sh` (S7–S14); `scripts/tools/dirty-resync-test.sh` (S7–S14).
- `Makefile`: `@bash scripts/tools/dirty-resync-test.sh` after `:94`, in `lint`; shellcheck
  takes both scripts already (`SHELL_SCRIPTS`, `:28`). No target and no `lab.sh` command: the
  check is run directly, as the other checks, so `make help` and `lab.sh`'s usage stay.
- `docs/lab.md`, `docs/followups.md` §2 and §3, `docs/benchmarks/` (S14, S15).

| Rules | Concrete proof |
|---|---|
| S1, S2 | `flaminio-repl/src/target.rs` unit harness: dirty/whole, empty plan, mapping holes, matching/mismatching digests, acknowledged partial-filter copies/discards, lost/duplicate/stale replies, aborted rounds and retries. Exact block totals. |
| S2, S3 | Same harness: last committed join after multi-chunk metadata batch emits once; pending join emits nothing; last-origin classification, reset, second outage and move destination. A view that removes the member's last out-of-ISR replica (a move's source) while it holds other chunks: one summary, with every counted round. A member left holding nothing: no summary, and its next rounds count from zero with a new start. |
| S5 | Target/engine tests depose/detach before/after pending join, including already queued summary; no old-target summary leaks to replacement. |
| S1, S6 | Simulator checker observes copied≤compared and no pending copy/discard at joined round. Enabled/disabled observer twins receive identical inputs: exact protocol outputs/order and protocol state match; consume events without random draws. Existing campaigns/mutations retain first-catch seeds. |
| S4 | `flaminio-node/tests/engine.rs`. Unit: `resync_summary_line` gives the exact text for fixed figures. Engine: the harness of `start_node` (`:196`, in-memory stores) sets a sink through `set_log` that sends each line to the test; a member stopped, written around and restarted gives exactly one line, with the member's name, the target's epoch, exact counts and no token bytes. |
| S7–S9 | Script tests validate exact geometry/ranges, known payload/zero manifest, committed ISR barriers, fresh replacement epoch/token, FORMAT=0, the watcher's period and clock, rx and tx read together, cursors, and separate pre/post-capture verification. The test runs the script's own extraction against the repository's `lab.sh` (only `vssh` and what runs in the VMs are faked) and fails if `metactl`, `data_vms` or `valid_id` is undefined after it. |
| S10, S11 | Prepared script fixtures fail each bad hash/hole, T-accessible member verification, summary count/epoch/abort, wrong plan origin/planned/copied totals, rx threshold (a large tx alone does not fail), timeout and negative interval. |
| S12, S13 | Script tests reject, with no create, attach or stop recorded: fl-a or fl-b stopped, attached, answering `status` with anything but `ok none`, or without a sweep; a third alive node, from fixtures in the client's protojson shape with the false fields absent (a dead, draining or witness one does not refuse); fl-c answering a malformed `status` (no `metactl get` recorded) or one naming a volume with a replica on either, or one whose `metactl get` fails; the run's id existing. `create` failing with AlreadyExists: no delete and no detach recorded; its answer lost: the same, and the message has `VOL=<id> make vol-delete`. ISRs full at 299 s of the watcher's clock pass, at 301 s fail; a `flaminio-meta get` that hangs is cut by its `timeout`, the watcher still exits non-zero at its limit and the run is invalid (S11); a watcher ssh that ends with 255 before its second line makes the run invalid, and the cleanup runs without waiting for the watcher. A malformed token, epoch or device path stops before its use. Inject failure at every action, require all cleanup attempts/no confirmation/no formatting; six interleaved runs and unchanged binaries/day. |
| S14, S15 | `dirty-resync.sh` computes them itself, in its `summarize` function, from the per-run figures it recorded (`scripts/tools/summarize.py` reads fio and flaminio-bench JSON: not used). The script test's six faked runs give known figures: median and spread=(max−min)/median; >10% marks unstable, zero median is stable only if all values zero. Differences of rx, tx and `elapsed_us` use medians; wall intervals are listed, not subtracted; preserve raw logs/counters and label extrapolation. `docs/lab.md` matches the actual script. |

Implementation checks: `make lint test race hygiene contracts sim`; no additional safety rule
is added, so no new protocol mutation is claimed. No tests or lab actions ran while drafting
this plan.

## Lab and docs

Later, explicitly authorized: `scripts/checks/dirty-resync.sh`; six runs
above, pass only with all S10/S11 checks and complete restoration. It creates/deletes private
volumes and replica data, stops/restarts both data planes, and changes target epochs; it never
reformats disks. Do not run alongside other lab work: the check asks only for S12's refusals.

Store raw data/logs in `docs/benchmarks/data/<date>-dirty-resync/`, summary in
`docs/benchmarks/<date>-dirty-resync.md`. Report each mode's medians/spread for rx bytes, tx
bytes, `elapsed_us`, wall interval and every counter; the both-mode differences of rx, tx and
`elapsed_us`, total and per GiB of baseline held; binary commit and workload sizes. The rx
difference is small by construction (about 0.3 MiB of Discards, plus requests) and follows
the holes (12 224 MiB here), not the data held. The digests' cost is the tx difference: the
results say both beside the per-GiB figures. Add the procedure to `docs/lab.md` and followups §3
results; §2 step2 records that durable dirty records need persisted uncertainty. Keep
architecture's desired design and #44's four steps unchanged; record no durable-record
decision from measurements automatically.

## Risks, ADRs and parallel work

Existing spec questions remain open: measure-first approval, the future record's worth/budget,
CRC-32C collision risk and #45 maintenance sequencing. Do not invent acceptance of them.
Settled at approval, 2026-11-13: questions 1 to 5, each with its recommendation (see decisions.md, Settled at approval).
Digest equality remains 32-bit equality; this slice provides measurements, not authenticated
integrity. Observation allocations/logging add overhead; record binaries and compare modes
without interpreting extrapolated 4 TiB costs as measurements. Counts include acknowledged
work, while rx counts include retransmission/background noise and need not equal copied bytes.
A node that becomes alive between refusal 2 and the create can still take chunks: the run
fails, and if it is fl-c the cleanup's tombstone wait cannot end. It stops and prints
`scripts/lab.sh vol-confirm <volume> fl-c`, for an operator.

No new ADR/protocol/token/on-disk record is needed. The accepted ADRs are relied on as they
are: 0001 (languages), 0002 (etcd) and 0005 (the lease set is the ISR), none of which the
observer or the check touches.

Parallelizable: observer/core plus engine logging and script/parser work can proceed against
the event/log contracts. Integrate only after observer noninterference tests pass; the optional
lab phase remains separate and leaves issue #44 open.
