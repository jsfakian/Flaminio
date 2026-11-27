# Issue #86 — report

## Review rounds

### Plan review round 1 — 412bf8ae49616116170f5b125d6c0d63d5f10488

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r86c1 (design-code), r86s1 (design-security).

- **R1-C1 — High:** Volume-wide sticky promotion is reached by ordinary workloads and turns every later takeover into a whole-volume reconciliation. Evidence: a write prunes only its own chunk (`flaminio-repl/src/replica.rs:231-238`), Settle only after reconciliation or a join; defaults of 256 MiB chunks and depth 128 give R about 65,536 on a 128 GiB sequential fill.
- **R1-C2 — Medium:** The unsupported-volume quarantine has no mechanism and no test; nor do the `Engine::start_target` count check and the agreement of the three constants. Evidence: `ReplicaService::new` is infallible (`flaminio-node/src/replica/mod.rs:127-146`); the test table has only core unit tests for S1/S4.
- **R1-C3 — Medium:** "Preserve #13 NotHeld / #61 bootstrap refusal" names behaviour the code does not have, and its test cannot be written. Evidence: `hello()` creates every advertised chunk it lacks (`mod.rs:239-257`); no `NotHeld` in the data plane.
- **R1-C4 — Low:** How the engine and the simulator obtain the captured Hello chunk set is not specified. Evidence: `Request::Hello` is a unit variant (`flaminio-repl/src/msg.rs:32`); the simulator delivers packets later (`sim/mod.rs:696`).
- **R1-C5 — Low:** #80 and #86 each name the other as prerequisite; neither says which introduces `encode_checked`. Evidence: nothing of it exists (`flaminio-net/src/session.rs:30-39,59-63`); #80 design lines 111-114.
- **R1-C6 — Low:** `ReplicaCore::restart()` and an add that triggers promotion are left open. Evidence: `replica.rs:136-145`; "every held chunk covered" against "added empty chunks start known-empty".
- **R1-C7 — Low:** The mutation harness would pass over the new checker's failures; a derived `Default` gives a budget of 0. Evidence: `flaminio-repl/src/sim/tests.rs:238`, `replica.rs:101`.
- **R1-C Nits:** `bypass_budget` against `bypass_uncertainty_budget`; `tag64`, `body13+8N+8T`, `audit0007/#83` read wrongly without spaces. Evidence: spec Tests, design "Checked codec" and "Lab, docs".
- **R1-S1 — Medium:** The quarantine of an unsupported or corrupt legacy volume has no defined behaviour and no test. Evidence: `store.delete_volume(uuid, Some(first))` runs before the core is consulted (`mod.rs:201`); narrowing with `as` and `expect` (`mod.rs:138-144`).
- **R1-S2 — Medium:** The plan understates when promotion happens and how long it costs; the ADR's consequence is wrong. Evidence: the prune a write carries is never above that write (`flaminio-repl/src/target.rs:1337`); Settle only at reconciliation and join (`target.rs:2262-2271,2578`); one stalled write holds the bound.
- **R1-S3 — Medium:** S7 "preserves" a refusal that does not exist in this tree, and names a test for it. Evidence: as R1-C3; intent says not to pretend other plans are implemented.
- **R1-S4 — Low:** The maximum-Version cover is built from a peer field that nothing validates. Evidence: no epoch comparison in `replica.rs:224-255` or `mod.rs:361-369,919-937`.
- **R1-S5 — Low:** A reply to Hello that is not Fenced still opens the session and extends the lease. Evidence: `target.rs:906-909`, then the `_` arm at `:988-999`.
- **R1-S6 — Low:** Who adds `encode_checked` if #86 lands first is not stated. Evidence: as R1-C5.
- **R1-S Nit:** the mutation name differs between spec and design. Evidence: spec.md:90, design.md:144.

Changes/resolution:
- R1-C1, R1-S2: coarsening is per chunk and not sticky. At N+R=B only the written chunk is coarsened (for a new chunk, the lowest-numbered chunk holding records); a cleared cover returns the chunk to precise records. The two triggers, the numbers and the lasting cost are in spec S2/S9, the design's risks and docs list and the ADR, which lists the volume-wide mode as a dropped option. Tests added: only the written chunk coarsens; coarsen, Settle, one more write; the default-geometry sequential fill.
- R1-C2, R1-S1: the quarantine is specified (spec S4, design "Unsupported volume"): where the mark lives, refusal before the earlier-incarnation delete whatever the first epoch, untouched chunks and fences, the existing delete as the way out, the logged and returned reason. Node tests named. The `Engine::start_target` check is removed from this plan (#80 owns it). A compile-time assertion ties the three constants.
- R1-C3, R1-S3: spec S7, the design and the ADR say an empty row carries the trust a missing row carries today and that create-on-Hello is unchanged; the refusal half of the S7 test is dropped and left to #13/#61.
- R1-C4: `Request::Hello { chunks }` carries the set from the Target to the engine, the node and the simulator.
- R1-C5, R1-S6: whichever of #80 and #86 lands first adds the entry point, the error type, `SessionError::Encode` and the `Conn::send` switch; #86 is expected first. Signature, bounds and zero bytes on refusal are unchanged.
- R1-C6: `restart()` leaves restart cover only; an add coarsens before inserting, so the new chunk starts empty. Both are named tests.
- R1-C7: the new checker's three error strings are named and added to `try_seed`'s catch classes; the budget is a constructor parameter with an explicit `Default`.
- R1-S4: the core drops a Write or Settle whose epochs differ from its session's (spec S3, design "Prune and restart proof"), with unit cases.
- R1-S5: a reply to Hello must be a valid Fenced before `session.open` and `confirmed_until`; Stale deposes first; `hello_answered_ok_grants_nothing` added.
- Nits: one mutation name; `tag (u8, 64)`, `body 13+8N+8T`, `ADR 0007`.

No finding was judged not to apply. Choices are recorded in [decisions](decisions.md).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 769b21e

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r86c2 (incremental design-code), r86s2 (incremental design-security).

- **R2-C1 — Nit:** Missing spaces before numbers still change the reading, and "T=checked sum lengths≤65,536" is unclear. Evidence: "Use65,536", "gives1,048,589", "MAX_BODY1,048,704", "audit7b94e2d", "on1/2 legal1GiB" in adr-proposal.md, design "Checked codec" and Facts, spec Tests.
- **R2-S1 — Low:** The over-budget Hello refusal is placed "before any store call", which the earlier-incarnation delete contradicts. Evidence: that delete is a store call that runs first and changes N (`flaminio-node/src/replica/mod.rs:194-212`); read literally, a new volume of the same name is refused on every retry.
- **R2-S2 — Low:** The moved-chunk create at N=B has no node test, and a missed preflight is durable harm. Evidence: `create_moved_chunk` calls `store.create_chunk` then `core.add_chunk` (`mod.rs:268-292`); a store with B+1 chunks is quarantined at the next restart.
- **R2-S3 — Low:** The reason given for the epoch check is wrong, and the test wording lets a weaker check pass. Evidence: a prune or Settle bound of a higher epoch clears restart cover and every record (`flaminio-repl/src/replica.rs:234-237,250-253`); a version below its prune is pruned as it is recorded (`:232-234`).
- **R2-S Nit:** The design lists no fixture change for the drop rule. Evidence: the node test helper `write()` hard-codes epoch 1 (`flaminio-node/src/replica/tests.rs:88-100`) and is used on sessions at epochs 5 and 2 (`:169-170,318-320`).

Changes/resolution:
- R2-C1: numbers and units are spaced in intent, spec, design and the ADR (every digit that followed a letter in prose, not only the listed ones); the preflight reads "T = the checked sum of the rows' extent counts ≤ 65,536".
- R2-S1: the design states the position: after the quarantine check, the earlier-incarnation block and the geometry-mismatch check, before `vol_id`, `core.handle`, any create and the fence, N counted on what the delete left. Spec S1 reads "before any create, fence or core change". `hello_over_budget_is_refused_before_create` gains the earlier-incarnation case.
- R2-S2: the design says `create_moved_chunk` calls `can_add_chunk` before the store create and names `moved_chunk_over_budget_is_refused_before_create` (budget 4: `Reply::Close`, unchanged `store.chunks()`, no quarantine after a restart).
- R2-S3: confirmed against the code. The epoch drop is now stated as a safety rule in spec S3, the design, the ADR and decisions.md, whose round-1 entry is corrected. Unit cases listed: version, prune and `below`, each lower and higher, and a higher prune and `below` against a restart-covered chunk. Simulator check added: a forged-epoch fault (`Config.forge_epoch_pct`, default 0), a shadow that takes a bound only at its session's epoch, and the mutation `accept_foreign_epoch`, caught as `uncertainty not covered`.
- R2-S Nit: the S3 test row says the helper `write()` takes the session's epoch and the check is not relaxed for it.

No finding was judged not to apply.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 3d89853

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r86c3 (incremental design-code), r86s3 (incremental design-security).

- **R3-C1 — Medium:** The forged-epoch fault is not restricted to chaos, and clean runs with it on are only a risk note. Evidence: `try_seed` panics when a seed fails with no bug (`flaminio-repl/src/sim/tests.rs:230`); `local_read_fail_pct` is limited to chaos for this reason (`sim/mod.rs:746-757`); the `from == to` case is unstated.
- **R3-C2 — Medium:** The node test volume is too small for the new tests. Evidence: `accepted()` sets `size: 4 * CHUNK` (`flaminio-node/src/replica/tests.rs:60-86`); `create_moved_chunk` returns `out of range` first (`mod.rs:277`), so the test passes for the wrong reason.
- **R3-C3 — Low:** Spec S1 says the refusal comes "before any create, fence or core change", but the design runs it after the earlier-incarnation delete. Evidence: that delete changes the store and the core (`mod.rs:195-210`).
- **R3-C4 — Low:** The Mutations row does not say how `accept_foreign_epoch` is made catchable. Evidence: it needs a config with `forge_epoch_pct` above 0 as the `mutation!` config argument.
- **R3-S1 — Low:** The corrected rationale says the drop closes the under-cover path; the same-epoch half stays open and the ADR sentence is false for it. Evidence: a Write with `version = (e, 5)` and `prune = (e, 100)` passes the check and is never recorded (`flaminio-repl/src/replica.rs:232-234`); a Settle with `below = (e, u64::MAX)` clears every record and cover.
- **R3-S2 — Low:** The earlier-incarnation case of `hello_over_budget_is_refused_before_create` can pass with the check in the wrong place. Evidence: a Hello advertising the indexes the earlier incarnation holds finds no missing chunk (`mod.rs:239-246`).
- **R3-S Nit:** Spec S1's "before any create, fence or core change" and "Neither deletes data" are false once the earlier-incarnation block has run. Evidence: `store.delete_volume` and `core.remove_volume` at `mod.rs:201-211`.

Changes/resolution:
- R3-C1: the design says the fault is applied in `Sim::send` only while `self.chaos`, drawing from the rng only when the percentage is above 0, to any destination including the target's own replica; `Config::random` leaves it 0. The clean campaign `forged_epochs_leave_runs_clean` (200 seeds at 20%) is a named test; the risk note is removed.
- R3-C4: the Mutations row names the test `catches_accept_foreign_epoch`, its `mutation!` call and the settings function `forged`; the budget is recorded after a real search.
- R3-C2: the node tests use a new helper `accepted_in(volume, count, epoch, session, chunks)` on 6-chunk volumes and assert each refusal by its text. The budget refusal names the service's budget; the quarantine reason is `too many chunks`.
- R3-C3, R3-S2, R3-S Nit: spec S1 says the refusal comes before any create or fence, after an earlier incarnation is discarded as today, and changes nothing of the volume the Hello names; the quarantine deletes nothing. The test has three cases at budget 4: same incarnation {0,1,2} then {3,4} refused; earlier incarnation {0,1,2} then {3,4} with a higher `first_epoch` succeeds with only {3,4} held; earlier incarnation then {0,1,2,3,4} refused with the earlier chunks gone.
- R3-S1: design, spec S3 and the ADR state what the drop closes (restart cover set in the fenced epoch, a lower version, a maximum above every later bound) and that a bound of the session's own epoch stays its holder's claim, unchecked as today. No rule is added. It is a line under Risks and an open question in the spec; decisions.md records it.

No finding was judged not to apply.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 3f063b838e53b02d1ffc511a63ae1ddeb1c525b4

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r86c4 (incremental design-code), r86s4 (incremental design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.

### Code/security review round 1 — cbb4097

Whole implementation and tests: `610f367cfc90d1e6f19e20d2848b8020d8ecb84e..cbb4097`.
The roughly 5,000-line change is split into A (replica/target/messages), B (node/net/fuzz
and integration docs) and C (simulator oracle/faults/histories), with a fresh code and
security reviewer for each part. Reviewers check cross-part call contracts. Built-in review
skills are unavailable in this environment; direct high-effort review is used and recorded.
Part A code review confirmed **R1-A-C1 — Medium**: the in-memory Replica Hello adapter
fences and creates chunks sequentially before discovering whole-set budget overflow. At
budget 2, held {0}, Hello at epoch 2 advertising {1,2} returns no reply but leaves fence 2
and chunks {0,1}. Production ReplicaService preflights before those changes; the simulator
adapter must match it. Fix: count unique missing chunks and refuse before core.handle/create.
All six reviews are complete. Part A security found no additional issue; Part B and C
code/security reviews are clear. No nits were reported. Part C security independently ran
the old-receipt controlled history (clean twin plus expected catch) with the adapter fix in
the working tree and its assigned simulator files unchanged; other runtime evidence
is the test engineer's and the recorded full checks. The adapter now preflights unique missing chunks before core/session/fence/create changes,
while preserving Stale replies. Its new regression failed before the fix (fence advanced
from 1 to 2) and passes after it. Tests cover unopened, precise and coarse state, unchanged
data/filter/uncertainty and old-session authority, exact-fit duplicates and stale requests.
All 20 replica tests pass. `make fmt lint test race hygiene contracts sim` passes with the
fix, including 20,000 normal seeds and all 163 release tests (ignored mutations included).
The fix is committed as `545d3726061dd6e0edf0cc1e7df78f762c3ed29e`.
Incremental code/security review and final conformance follow.

### Code/security review round 2 — 545d3726061dd6e0edf0cc1e7df78f762c3ed29e

Incremental range: `cbb4097..545d372`, all four changed files. A fresh code reviewer
confirms R1-A-C1 fixed and reports no new findings or nits. The fresh security reviewer
independently confirms the fix and reports no new findings or nits. The code/security loop
is **clear after two rounds**; no accepted or unresolved runtime findings remain.
The workflow's requested Sonnet model is unavailable; the code reviewer uses the inherited
default model at medium effort. Direct incremental review is used as the workflow requires.

## Implementation — 2026-11-27

Branch: `issue/86-bounded-uncertainty`, based on
`610f367cfc90d1e6f19e20d2848b8020d8ecb84e` (main after the user's merge of #81 / PR #102).
The published clear spec/design were approved on 2026-11-20. This issue precedes #80 because
its bounded Fenced representation is a dependency of #80's complete transport guarantee.
[Units](units.md) records the completed implementation and acceptance work.

[PR #103](https://github.com/jsfakian/Flaminio/pull/103) was published as a draft at the user's
request while validation was in progress. Full checks pass and code/security review is clear;
conformance passes S1–S9. Publication changes it from draft to ready after final CI. No lab checks are required by the approved design.

### Consistency regressions found by acceptance

The clean forged-epoch campaign initially failed at seed 4 (`forge_epoch_pct=20`,
`Config::random(4)`, budget 65,536, no mutation): write 294 was acknowledged, then a final
read returned 292. A mixed-fault repeat at budget 8 also failed. Independent runtime/model
analysis confirmed that a dropped in-flight write could be admitted after an older value
was copied to a joiner. BeginResync Ack now retires prior request receipts and replays all
pending chunk Writes before snapshot work. The resync tracks fresh receipts independently
of client completion. The original regression assertions remain and pass.

Broader runs found clean cases 1833, 9803 and 14081: a conditional source removal could commit
while its response was unresolved, mutable proposal state cleared, and a delayed Hello
recreated the removed chunk empty before a read. Source retirement now survives uncertain
CAS outcomes until a strictly newer authoritative version. It rejects pending source
receipts, invalidates fast-read generations and aborts affected snapshot provenance.
A separate unmapped-source proof showed that pending extents must seed `force_copy` before
Mapped, including Writes that finish before BeginResync Ack; abort preserves this evidence.
All three permanent cases and the new edge-case proofs pass.

The replay defense invalidated two old random mutation hints. A test-only deferred Write
completion provides an honest old-generation receipt history with replay still enabled;
normal simulation scheduling and RNG are unchanged. The old fast-fence hint 103223 no longer
catches; an actual search found no witness in [0,200000). Direct checks of previously
recorded candidates verified seed 1,001,529 (and 3,626,767), each with a passing clean twin.
The lower verified witness is pinned without claiming it is the first catch beyond the
searched range. No mutation assertion was skipped or weakened.

## Tests

Acceptance results, including the final post-review suite:

| Area | Result |
|---|---|
| Replica core | 20 tests pass after review fix; 28,800 independent exact-oracle operations, partial-page reuse and default 512×128 fill (four covers) |
| Node lifecycle | 39 focused tests and full node package (52 library + 19 integration) pass; real-store failed-write/reconnect/restart proof passes |
| Codec/session/fuzz | 15 net unit + 7 session and 4 fuzz-regression tests pass; real F09 workloads, maximum body, zero-byte refused sends and 17 hostile seeds |
| Target/engine | All 75 Target tests and local/remote captured-Hello adapter tests pass |
| Small-budget simulation | 200-seed normal/mixed campaigns at budgets 8/16, malformed-report campaign and pending/coarsen/restart/repair/Settle histories pass |
| Production-bound simulation | Seven #81 large-volume recovery cases and permanent clean cases 1833/9803/14081 pass |
| Independent controls | All 38 mutation catches pass, including new coverage/prune/budget/epoch/authority, replay omission, pending-seed omission and source-retirement controls |
| Broad forged-field simulation | Fresh [0,20000) clean campaign passes with 504,912 forged requests; 60.15 s test runtime |
| Control-plane/contracts | `make race hygiene contracts` passes after the source-retirement fix; generated outputs unchanged and both mutual-TLS interop tests pass |

The broad campaign is reproducible from `dataplane/` with
`FLAMINIO_SIM_SEEDS=20000 cargo test --release --offline -p flaminio-repl sim::tests::forged_epochs_leave_runs_clean -- --exact --nocapture`.
The default test count remains 200.

| Required check | Result |
|---|---|
| `make fmt lint test race hygiene contracts sim` | PASS after the review fix, source `545d372`; 20,000 normal seeds and all 163 release tests, including ignored mutation controls |
| `make sanitize QUICK=1` | PASS locally at `cbb4097` (address/leak and thread sanitizers); CI repeats it on the final published commit |
| GitHub CI | All seven jobs and GitGuardian passed at `cbb4097`; final publication checks are linked from PR #103 |

The adapter-only review fix adds no unsafe code or production node path. Its focused
regression failed before the correction and passes afterward; full checks were repeated.
Final CI status is available on [PR #103](https://github.com/jsfakian/Flaminio/pull/103/checks).

## Conformance

Independent checker on source `545d372` plus final documentation, using the inherited
default model at medium effort because the workflow's Sonnet model is unavailable.
Its output follows verbatim:

PASS

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 Bounded retention | yes | yes | `replica.rs:1059` budget boundaries; `:1633` atomic Hello refusal; node pre-create refusal tests pass. |
| S2 Conservative coarsening | yes | yes | `replica.rs:1106` chunk isolation/max-Version; `:1149` add-at-budget; `:1587` default fill gives four covers. |
| S3 Pruning | yes | yes | `replica.rs:1191,1233` independent restart/coarse proofs and foreign epochs; `target.rs:4088` actual repair/Settle integration passes. |
| S4 Lifecycle | yes | yes | Node quarantine, incarnation cleanup, failed-store/reconnect/restart and moved-chunk refusal tests pass; simulator replay/source-retirement regressions pass. |
| S5 Representable reports | yes | yes | `frame.rs:948` maximum N=T=B roundtrip/work counts; `:1212` real-core F09 workloads; explicit empty-row tests pass. |
| S6 Checked framing | yes | yes | `frame.rs:238,583` checked encode/decode; zero-byte refusal session test and all fuzz regressions pass. |
| S7 Consumer validation | yes | yes | `target.rs:959,1499` validation precedes authority; malformed/captured-required/extra-row tests and local/remote engine adapter tests pass. |
| S8 Bounded work | yes | yes | `replica.rs:1327,1361`, `target.rs:3420,3489` allocation/reuse, coarse updates, scan counters and geometry-bounded union tests pass. |
| S9 Compatibility/docs | yes | yes | Wire equality and mutual-TLS interoperability pass; protocol, dataplane README and followups document costs, rollout, quarantine and deferred limits. |

Contract drift: none

ADR conformance: consistent

Scope drift: none

Docs: done

Open review findings: none

## Lab

None required by the approved design. No shared-lab actions were run.

## ADRs

The policy was accepted with the approved plan. The numbered
[ADR 0017](../../adr/0017-bound-uncertainty-with-per-chunk-covers.md) is Accepted after
clear code/security review and PASS conformance, as covered by plan approval. Existing accepted fencing/ISR
decisions remain unchanged; clock ADR 0007 remains Proposed.
