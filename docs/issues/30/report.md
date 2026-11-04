# Issue #30: a volume created again starts above every earlier epoch — report

The pipeline's record for #30: [intent](intent.md), [spec](spec.md), [design](design.md),
[decisions](decisions.md). Loop rules: `.claude/skills/ship-issue/review-loop.md`.

## Review rounds

The spec and design loops ran before this pipeline's rules (commits 7548371, f8def04).

**Code loop, round 1** (whole change, fb99440 with main merged; code and security reviewers):
0 Critical, 0 High, 0 Medium, 5 Low, 8 Nits.
- **Low, both reviewers:** `Store::delete_volume(_, u64::MAX)` skipped a chunk fenced at
  `u64::MAX`, so `delete VOLUME` reported success and left it. Now `Option<u64>`, `None` for
  every chunk (D13); store and replica regression tests.
- **Low:** an earlier volume's version 2 token at a Hello was untested: a replica test now
  shows it `Stale`, dropping nothing.
- **Low:** the drop has no simulator check: D10 says why, and which tests stand in.
- **Low:** `docs/lab.md`, `docs/replication-protocol.md` and `docs/followups.md` disagreed
  with the change: updated.
- **Low:** S9 untested: D11 says why.
- **Nits fixed:** `lab.sh` checks the first epoch is a number before arithmetic; the
  `unverified` feature's comment; an engine case for a token that does not decode; R10 for a
  drop failing after its first batch. **Not changed:** skipping the drop for a volume already
  fenced above its first epoch is not sound (D12); the design's cost claim is corrected
  instead. Encoding each delete record twice to size the batches is left as it is.

**Code loop, round 2** (incremental: the 12 files 32d7475 changed; code and security
reviewers): **clear**. Every round 1 finding fixed; nothing broken. `None` (every chunk) is
reached only from the control socket's `delete`, never from a token or a peer. Two free Nits
fixed (8281c56): a paragraph reflowed, D13 named in the design.

**Code loop, round 3** (incremental: the merge of main, 4f9879c, which brought #10's held
groups to #30's drop): **clear**. The drop's `guard` lands a held group before any delete, and
#10's `settle_local` runs before all three callers (the peer's Hello, the attach's Hello,
`delete VOLUME`); a crash between them is covered by the batches' reasoning; the merge's
`first_epoch: 0` in #10's test is a version 1 token, as meant. One free Nit fixed: the
precondition (a held group is the caller's to settle) stated on `hello` and `delete_volume`.

## Tests

`make lint`, `make test`, `make race`, `make contracts` and `make sim` pass on 8281c56.

| Rule | Tested by |
|---|---|
| S1 | `TestCreateStartsAtTheFirstEpoch`, `TestCreateAgainStartsAboveTheDeletedVolume`, `TestCreateAfterAnInterruptedCreationStartsAboveItsClaim` |
| S2 | `TestMemRevisionStartsAtClock` |
| S3 | `TestALegacyVolume`, `TestCreateAfterADamagedHeaderStartsAboveIt`, `without_a_first_epoch_nothing_is_dropped` |
| S4 | `TestBodies`, `TestSignRefusesFirstEpoch`, the version 2 vectors in Go and Rust, `decode_unverified`, the `capability_token` fuzz target |
| S5 | the replica tests of the drop, `delete_volume_deletes_the_chunks_fenced_below_an_epoch` |
| S6, S7 | `a_volume_created_again_neither_adopts_nor_is_deposed`, `a_drop_under_a_target_of_the_earlier_volume_fails_its_io`; S7 on the lab: pending |
| S8 | what is left: nothing to test |
| S9 | none: old binaries cannot be built in the tree (D11; R5, R6) |
| S10 | the regenerated vectors with first epochs; the shared token bodies |

## Conformance

**PASS.** Every rule implemented; S1–S7 and S10 tested (S7's lab part pending); S8 describes
what is left; S9 untested by D11. Contract drift: none (no proto, frame, control-socket or
on-disk record change; the header's JSON gains `first_epoch`; token version 2 matches in Go
and Rust). ADRs: consistent (0002). Scope: the pipeline configuration of 4483623, and the
files round 1 added (`docs/lab.md`, `docs/followups.md`, `store_behavior.rs`). Docs: done.
Open review findings: none.

## Lab

The design's lab check (spec, Checks), with the metadata service and the nodes deployed from
this branch: **pending**, run on request (`/ship-spec 30 lab`), after #10's write campaign
frees the lab.
1. A volume created, attached on fl-a, 1 GiB written with a verify pattern, detached.
2. Its attach token kept; the volume deleted, its tombstone removed with `etcdctl del`.
3. Created again with another size and attached on fl-a: the epoch is above the old one.
4. Reads zeros over the first GiB, then writes and verifies; no degraded chunk.
5. Attached on fl-b: reads back the new pattern, not the old one.
6. The old token at `node.sh attach` on fl-a: refused, naming both epochs.

## ADRs

Relied on: 0002 (a future store must keep revisions growing, now said in `KV`'s comment).
None proposed.
