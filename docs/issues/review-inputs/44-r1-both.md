# Plan review: issue #44 (Dirty-region resync), round 1, code and security

Scope: `docs/issues/44/` (intent.md, spec.md, design.md) against the code of the worktree
`/home/jsfakian/Documents/src/flaminio-44`. Paths below are relative to that worktree.

The observer itself holds up against `target.rs`: the three places a round ends
(`abort_resync` :1288, the join :2327, `adopt_replicas` :2639) are all named, copies only ever
come from a completed digest (so copied <= compared holds per round), no wire message, RPC,
token or on-disk field changes, and nothing is added to the write or read path. No new safety
rule, so no injected bug is owed. The findings are in the measurement and the lab check.

### R1-B1 — High
**The check measures the bytes the member received, but the digests travel the other way.**
S9 reports "the bytes the member received", S14 calls the difference between the two runs "the
most that a record outliving the target could save", and S7.3 sizes W "so that the digests of a
whole plan are not lost among the copies". In the check's topology the target is on T, beside
the source replica, so:
- the member's digests are a reply it *sends* to T (`Response::Digests`, `target.rs:948`;
  requests go to source and member at `:1861-1880`), 5 bytes per block
  (`flaminio-net/src/frame.rs:306-318`);
- the source's digests never cross the overlay (T's own replica).

What the member receives in the replaced-target run beyond the same-target run is the Digest
requests and about 12 000 `Discard` frames (the whole plan's holes in `max_io` = 256-block
pieces, `target.rs:966`, `flaminio-io/src/node.rs:330`), not the roughly 5 MiB of digests for
4 GiB mapped. The byte difference of S14 would therefore understate the cost it is meant to
decide open question 2(a) with. Intent Why 3 has the same error: it reads the 46 MiB of
follow-ups §3 as digests ("3 GiB of digests" per TiB) although that figure is whatever fl-a
received in a run whose target placement is not recorded; by the frame encoding a digest reply
costs 1.25 MiB per GiB mapped per remote party. The design's allowance (2 x 46 MiB) inherits it.
**Fix:** S9/S14 report the member's `tx_bytes` as well as `rx_bytes` on wg0 (or T's rx), and
take the byte difference from both; keep the W-plus-allowance failure on rx (copies). Restate
S7.3's reason and S11's allowance without the 46 MiB-as-digests premise, correct intent Why 3,
and add the tx counter to the script-test fixtures.

### R1-C1 — Medium
**Nothing makes the volume land on T and the member only, and a miss leaves a tombstone the
check cannot clear.** S7 requires every chunk on T and the member; the design only inspects the
placement after `create` ("require all 64 replica sets exactly {T,MEMBER}"). `Place` puts the
first replica on the preferred node and spreads the second over the least loaded candidates,
which are every alive, non-draining, non-witness node (`meta/domain/domain.go:354-358`,
`meta/service/service.go:397-402`). With fl-c's node up, some chunks land on fl-c; the check
then refuses, deletes the volume, and the tombstone stays pending for fl-c, which has no sweep
(`scripts/lab.sh:1782-1786`, `docs/lab.md:207-229`). That breaks S12 before a run has started.
**Fix:** before creating anything, list the nodes and refuse unless T and the member are the
only alive, non-draining data nodes (add it to S12); give the refusal a script test.

### R1-C2 — Medium
**S4's engine test cannot observe the line.** The engine logs with `eprintln!`
(`flaminio-node/src/engine.rs:1772`) and `flaminio-node/tests/engine.rs` has no way to read
what was logged; the design's "capture structured log ... exact counts, one line" names no
seam, and states that `Event` and the drain are "the only production reporting seams needed".
**Fix:** name the seam: a pure function that formats the summary line, unit-tested for the
exact text, plus whatever the engine test asserts on (for example a line sink the test
injects). Say which, and where it lives.

### R1-C3 — Medium
**The time figure's resolution is unspecified and likely coarser than the signal.** The design
polls "every ISR" for the end of the interval without a period, through metactl over ssh; the
repo's idiom is `sleep 5` (`scripts/checks/resync.sh:50`, `wait_isr_full` in `scripts/lab.sh`).
By intent Why 3 the digest work is about 0.05 s per GiB, so about 0.2 s for the 4 GiB the
member holds: S14's time difference would be poll and node-start latency, and S13's 10% test
would pass or fail on it.
**Fix:** state the poll period and where the loop runs (on T, as `reconcile.sh` times on NODE),
and define S14's time difference on the summary's `elapsed_us`, with the wall interval
reported beside it.

### R1-C4 — Medium
**S3's condition can become true without a join, and the design then never reports.** The
design evaluates the summary only for "members whose committed joins advanced". A member is
also "in the ISR of every chunk it holds" when a view drops its last out-of-ISR replica: a
move's source outside the ISR is not resynced (`leaving`, `target.rs:2334-2341`) and its
replica is removed in `adopt_replicas` (`:2609-2641`). Its totals, aborted rounds included,
then stay unreported and are added to the next outage's summary with the old start time.
Totals of a member that no longer holds any chunk are never dropped either. The test row
"source removal is not a spurious summary" does not say which of these it means.
**Fix:** evaluate S3 after every applied view for each member with unreported totals; say what
happens when the member holds nothing (drop the totals, no summary); one unit test for each.

### R1-C5 — Low
**The check's preconditions differ between spec and design, and two have no mechanism.**
S12 refuses only when T has a volume attached or a node is fl-c. The design adds: the member
unattached (needed: `node.sh down` removes its device), both nodes running, and "exclusive lab
use with no other volume replicas/workloads on them". The last is not in the spec, has no
stated way to be checked, and is false in the documented lab, where vol1 has replicas on fl-a
and fl-b (`docs/lab.md:138`). Neither document checks that the sweeps run (the CSI node
plugins, `docs/lab.md:210`): without them the cleanup's wait cannot end
(`scripts/lab.sh:1796`).
**Fix:** S12 lists the exact refusals, each as something the script can ask: no volume
attached on T or the member (control `status`, `flaminio-io/src/control.rs:104`), no attached
volume elsewhere with a replica on either, the sweeps present; the design names the command
for each and drops "exclusive use" as a check.

### R1-C6 — Low
**The design adds fields and a failure the spec does not have.** `valid` with "a bounded
reason", `epoch` in the line, u128 accumulation, and "invalid summary" as a failure of S11
trace to no rule; S4 says the line carries the figures of S2 and S3. The reason has no format
for the script to parse, and nothing can make an "impossible" anomaly happen in a test. Block
counts cannot overflow u64.
**Fix:** drop `valid`, the reason and u128; keep `epoch` and add it to S4 (the script uses it
to pick the run's summary). Or add all of them to S4 and S11 with a format and a test.

## Nits

- **N1** `spec.md:5`: "Decisions and design: to come" is stale; link `design.md`.
- **N2** `design.md:17`: `node.sh` does not validate `attach` (only `detach` and `delete` call
  `volume_id`, `scripts/guest/node.sh:76-91`). The check's own validation is what holds; fix
  the sentence.
- **N3** `design.md:19-21, 173-176`: audit #81, audit D01, ADR 0007 and #83 are not on this
  branch and no rule of S1-S15 depends on them; cut to the one fact used (the check stays at
  64 chunks, intent Why 7).
- **N4** `design.md:146`: the implementation checks omit `make hygiene` (AGENTS.md's list
  before a pull request).
- **N5** `design.md:144`: says what the S14 figures are, not which file computes them
  (`scripts/tools/summarize.py` exists); name it.

Reviewed commit: 07339ef6cc0c9ea6303d22478c59e0ffaf8dc99b
