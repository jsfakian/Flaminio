# Issue #45 — report

## Review rounds

### Plan review round 1 — 9acc145

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High:** #13's hand-offs missing: released nodes not excluded from destinations, every `Applied=false` taken as a concurrent winner, S15 lacking #13's two reasons, no failed-for-good declaration; `ForgetNode`'s drain mark and rewrites collide with S9 and the invariant. Evidence: #13 decisions.md:230-234, design.md:162-171, :1281-1287; spec S2, S16, S19; #45 design.md:97-101, spec.md:23-27, :60-62, :122-124.
- **R1-C2 — Medium:** #17's S15–S17 deferred, not taken; "restricted to Active" contradicts #17's S16 standing records; "release" used for #17's unhold. Evidence: #17 spec S15-S17, design.md:66-108, :414-462; #45 design.md:103-107, intent.md:125-126, spec.md:163.
- **R1-C3 — Medium:** S15's "no destination" is not #18's S5 refusal; the choice drops `bestEffort`'s fall-back and #18's S14 refusal classes. Evidence: #18 spec S5, S10, S14; design.md:131-203; drain.go:135-137.
- **R1-C4 — Medium:** #72's refusals not skipped; "No #72 draft" stale; the controller wrongly made to call a reservation ledger. Evidence: #72 spec.md:58, :96-99, :123; #45 design.md:22, :119-121.
- **R1-C5 — Medium:** who records Missing and timeout Failed, what `enabled` gates, and who creates the clock unspecified. Evidence: design.md:44-49, :91, :140, :143-144; decisions.md:51-55.
- **R1-C6 — Medium:** the new enums fail buf STANDARD, and an unset action means FAIL. Evidence: buf.yaml `lint: use: STANDARD`; design.md:129, :133.
- **R1-C7 — Low:** #78's gate used but not declared; gate bumped on Missing↔Active against #78's rule. Evidence: design.md:50-51, :82-85; #78 design.md:26; intent.md:7; spec.md:4.
- **R1-C8 — Low:** timeout Failed does not compare the policy. Evidence: design.md:46.
- **R1-C9 — Low:** the mixed-version test cannot prove S19. Evidence: design.md:155.
- **R1-C10 — Low:** no Go test proves the invariant for the service's writes. Evidence: design.md:157.
- **R1-C11 — Low:** the probe that earns credit is undefined. Evidence: design.md:62; decisions.md:51-58.
- **R1-C12 — Low:** small clusters (ADR 0006) not considered. Evidence: ADR 0006; spec S6, S7.
- **R1-C13 — Low:** the lab plan is not runnable as written. Evidence: design.md:162, :167-182.
- **Nit (code):** stale "blocked"/"disabled"/"unapproved" text beside settled notes. Evidence: design.md:75, :140, :170, :186-188; adr-proposal.md:32-36, :71-73.
- **R1-S1 — High:** a failed node also marked draining is never rebuilt; #13 marks every forgotten node draining, and a stopped drain leaves its mark. Evidence: spec S9; design.md:97; #13 design.md:162-168, :211; drain.go:171-189; #18 S13; domain.go:287.
- **R1-S2 — Medium:** #13's move rules absent from reasons and behaviour; a refused move re-asked each pass; the lab's "no record names it" false under #13. Evidence: #13 S1, S2, S14, S16; #18 Eligible; spec.md:137; design.md:97-101.
- **R1-S3 — Medium:** loops in every instance can depose each other's maintenance targets; the scan and re-asks multiply. Evidence: #17 design.md:66, :72-76; design.md:91, :97, :106.
- **R1-S4 — Medium:** `isr_minimum` judged from records misses unheld volumes, whose stuck maintenance targets hold #17's slots. Evidence: spec S15; target.rs:2252; #17 S11, S14, S17.
- **R1-S5 — Medium:** #72 contradicted: the heartbeat's `alive` value, its refusals, filling disks. Evidence: #72 design.md:44-47, S1, S5, S8, S9; design.md:22, :38-41.
- **R1-S6 — Medium:** S7's conditional write depends on #78, undeclared. Evidence: design.md:50, :81-85; spec.md:4; service.go:905-907.
- **R1-S7 — Medium:** `SetNodeState.action` has FAIL as its zero value; enums fail buf lint. Evidence: design.md:133; buf.yaml.
- **R1-S8 — Low:** the agent may heartbeat, and a heartbeat now keeps a node out of Missing. Evidence: auth.go:58-59, :73-74; engine.rs:726; #72 S10.
- **R1-S9 — Low:** the lab fails the nodes it stops for single-node verification. Evidence: design.md:170-173.
- **R1-S10 — Low:** a fresh lease every heartbeat roughly triples etcd's heartbeat writes. Evidence: design.md:40-41; kv/etcd.go:120-150.
- **R1-S11 — Low:** the mixed-version guard proves nothing about other instances. Evidence: design.md:155.
- **R1-S12 — Low:** what `enabled` turns on is unspecified. Evidence: design.md:75, :140, :170; decisions.md.
- **Nit 1 (security):** #14's confirmation step is gone under #13. Evidence: design.md:180; #13 S13.
- **Nit 2 (security):** the failed-destination refusal needs a fixed prefix. Evidence: design.md:85; #18 S7, S12, S14.

Changes/resolution (all made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 1"):
- **R1-C1, R1-S1, R1-S2:** #18's `choose` with #13's order (bound waits; released nodes not eligible; only releasing nodes left waits); `Applied=false` classified by #13's `reason`; no move #13 refuses is asked; S15 gains both #13 reasons. Failed for good: `NODE_STATE_ACTION_FORGET` and `forget-node`, refused unless Failed, running #13's `ForgetNode` with the state compared in its mark transaction; Admit of a forgotten node refused; automatic Failed never forgets. S9 and the design rebuild a failed node whatever its drain mark. The invariant and its simulator check carve out failed for good (#13's safety). Lab: release deletion on, fl-b checked by "no replica or move names it, no release after its sweep"; runbook forgets a dead failed node once re-protected. Tests added.
- **R1-C2, R1-S3, R1-S4:** the design's maintenance paragraph states #17's S15–S17 rules one by one, each tested; S12 uses #17's S16 with no "also active"; "unhold" (`UnholdVolume`) in the intent and Q5. One active loop: only the `rebuild-observer` owner acts, its `AttachMaintenance` comparing that ownership; no scan without a Failed node; 30 s back-off after a refusal (S15, S17). At the minimum: a chunk with fewer ISR members on alive nodes than the minimum gets, in an unheld volume, no move and no maintenance attach (S9, S12, S15); tests RF 3/min 2 and RF 1.
- **R1-C3:** S11 names #18's `choose` (`strict`, `bestEffort`) and S14's two refusal classes; S15 uses #18's S5 wording and reason; tests for a collision and a destination failed mid-pass.
- **R1-C4, R1-S5:** #72 declared a dependency; stale text removed; `MoveReplica` charges, the rebuild skips a refused destination (blocked with the last refusal when none is left); the heartbeat writes #72's `alive/<node>` value; test for a full destination.
- **R1-C7, R1-S6:** #78 declared a dependency (#76 not needed, #78's Q2); gate bumped only into and out of Failed; a Fail-racing-a-move test.
- **R1-C6, R1-S7:** `NODE_STATE_*` and `NodeStateAction { NODE_STATE_ACTION_UNSPECIFIED = 0; …_FAIL = 1; …_ADMIT = 2; …_FORGET = 3 }`; UNSPECIFIED refused with `InvalidArgument`; service and auth tests.
- **R1-C9, R1-S11:** S19 stated as an operator's rule (as #18's Q5), in `docs/lab.md`; the test claim dropped.
- **R1-C5, R1-S12:** one flag, off by default, gating timeout failure and the rebuild's requests; Missing always recorded by the observer owner in its 5 s pass; the observer and clock always run, the first owner creating the clock; test "policy off: Missing recorded, never Failed by timeout".
- **R1-C8:** timeout Failed compares the policy key; racing test.
- **R1-C10:** service test on Mem and etcd: `volumes/` and `chunks/` revisions unchanged across each transition but FORGET.
- **R1-C11:** the probe is a linearizable etcd `Get` of `rebuild-observer` within 1 s, in a process whose gRPC server is serving (not a loopback TLS `ListNodes`: the server certificate has no client use).
- **R1-C12:** small clusters named in the ADR proposal, runbook and `docs/lab.md`; two-node service test.
- **R1-C13:** `make rebuild-test` / `scripts/lab.sh rebuild-test`, each step's `metactl` call, the help text and `docs/lab.md` entry.
- **R1-S8:** `NodeHeartbeat` only by an administrator or the node's `io` (S8); auth test.
- **R1-S9:** the lab turns the policy off before the single-node checks; covered by `rebuild-test.sh`.
- **R1-S10:** the cached lease is renewed after a committed transaction, a fresh one granted only on expiry; tests on both stores.
- **Nits:** stale text rewritten to the settled rules; tombstones confirmed by the nodes' sweeps (#13 S13); fixed prefix `node failed: ` (`domain.ErrNodeFailed`).

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred.

### Plan review round 2 — ba9d1d5

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: every round-1 finding fixed; R1-C1 and R1-C2 opened R2-B1, R2-S1 and R2-C1, R1-C11's probe is R2-B2, R1-S4 left R2-S2, and R1-C13 carried R2-C2.

- **R2-B1 — Medium:** #13's own `ForgetNode` RPC and `metactl forget` still forget a node that is not Failed, bypassing S4's refusal. Evidence: design.md:69-74, :258; spec.md:50-51; #13 design.md:265, :892-895, :1237; decisions.md:86-87.
- **R2-S1 — Medium:** FORGET accepts a Failed node that is alive now or only partitioned; forgetting cannot be undone. Evidence: design.md:69-72, :319-320; spec S6; #82 spec.md:22-26; #13 design.md:1270-1272, spec.md:102, :147.
- **R2-C1 — Medium:** a new maintenance attach may pick the Failed node itself, and #17 S17 then pins the copy there. Evidence: decisions.md:107-110; spec.md:93-94; design.md:192-194, :202-203; target.rs:1649-1651; intent.md:125.
- **R2-B2 — Medium:** the probe shows neither that a heartbeat could commit (etcd NOSPACE) nor that a node could connect (expired serving certificate). Evidence: design.md:100-103; main.go:116; pki.go:285; decisions.md:165-168.
- **R2-S2 — Low:** S12 still attaches maintenance targets that can never end: another chunk's move below the minimum, or a lost destination. Evidence: spec.md:91-94; design.md:177-182, :188-189, :333-335; #17 "Work", S11, S14.
- **R2-C2 — Low:** under #13 the lab's "holds nothing" fails (a fence-only slot stays), and enabling at 5 s fails any node already missing. Evidence: design.md:293, :300-301; spec.md:176-177; #13 spec.md:10-11, S3.
- **R2-N1 — Nit:** a forgotten destination is `Applied=false` with #13's reason, or #78's `is draining`, not a `FailedPrecondition` of its own. Evidence: design.md:164; #13 design.md:1004.
- **R2-N2 — Nit:** which answers start the 30 s back-off, against S11's "for the pass". Evidence: design.md:167-168, :173; spec.md:86.
- **R2-N3 — Nit:** "the commit compares the gate and the revisions read" reads as one `node-state/` compare per candidate, past 120 in the creation claim. Evidence: design.md:118-120.
- **R2-N4 — Nit:** for how long a non-owner asks nothing. Evidence: design.md:206-207.

Changes/resolution (all made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 2"):
- **R2-B1:** FORGET is the only way to forget a node: the Failed check and `node-state/` compare sit in `ForgetNode`'s mark transaction, so #13's RPC makes them too; `metactl forget NODE REASON` sends `SetNodeState` FORGET and `forget-node` goes. Recorded as a change to #13 made by whichever of #13 and #45 lands second. Test: `ForgetNode` of an Active node refused.
- **R2-S1:** the mark transaction compares `alive/<id>` absent (`node N is alive`); spec S4 and S20, the runbook and `docs/lab.md` say Failed is no evidence a node is gone, and forgetting needs evidence from outside the cluster (#82 S1, S2). Test: FORGET of a Failed node that heartbeats refused.
- **R2-C1:** a standing record stands by #17's S16; a new ask chooses among S16's nodes that are also Active, else `no maintenance node` (spec S12). Test added.
- **R2-B2:** the probe is the tick's checkpoint, a conditional write: an interval is credited only if its checkpoint commits within the bound; none while the serving certificate is outside its validity. Tests: a refused checkpoint and an expired certificate add zero.
- **R2-S2:** a volume not held with any pending move that cannot be carried out (below the minimum on alive nodes, or destination lost) gets no maintenance attach; its other chunks wait, reported (spec S12, S15 narrowed). The Risks line names an already started target. Tests for both cases.
- **R2-C2:** fl-b is checked as "no release, only each volume's fence-only record"; step 1 first requires every data node Active and alive in `metactl nodes`.
- **R2-N1:** forgotten destination handled as `Applied=false` with `node T is forgotten`, or #78's `is draining`.
- **R2-N2:** the back-off starts on a collision, no candidate left, or a release reason, and covers S11's "for the pass".
- **R2-N3:** no admission transaction compares a `node-state/` key; #78's gate covers them with one key.
- **R2-N4:** a non-owner asks nothing for the rest of the step and every step without the ownership; it acts again once it holds it.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred.

### Plan review round 3 — 890f77b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R2-B1, R2-S1, R2-C1, R2-B2, R2-S2, R2-C2 and R2-N1 to R2-N4 fixed; R2-B1's #13 test list opened R3-C1, R2-C1 left the drain's path (R3-C3), R2-B2 opened R3-S1, R3-C2 and R3-C4, and R2-S2 left R3-B1.

- **R3-C1 — Medium:** the change to #13's tests ("first make the node Failed and not alive") breaks `TestForgetRacesAPlacement`, makes `TestForgetRacesAMoveBack` pass with the compare removed, and changes `TestForgetNodeRefusals`' answers. Evidence: decisions.md:208-209; design.md:127, :133-135; domain.go:367-368; service.go:399; #13 design.md:1004-1044.
- **R3-B1 — Low:** an unheld volume with a pending move to a destination gone for good never gets a maintenance attach; the plan only says its chunks wait. Evidence: intent.md:54; #13 design.md:170-183; spec.md:127; decisions.md:231-233; design.md:198-203, :352.
- **R3-S1 — Low:** the certificate rule stops credit without moving ownership, and reads the wall clock. Evidence: design.md:97-99, :105-118; flaminio-metaclient/src/lib.rs:6; main.go:116; pki.go:285.
- **R3-C2 — Low:** "credited only if it commits within the bound" and "a late checkpoint adds zero" cannot hold: a late commit keeps its credit. Evidence: design.md:105-116, :299; decisions.md:223-224; adr-proposal.md:31.
- **R3-C3 — Low:** R2-C1's protection can be bypassed through a drain's maintenance ask on a Failed node. Evidence: #17 design.md:443-444; design.md:127, :209-218.
- **R3-C4 — Low:** the probe adds a retained etcd revision every second, and nothing compacts. Evidence: node.rs:331; scripts/guest/etcd.sh:45-52.
- **R3-N1 — Nit:** #82's evidence rule is for its node-wide recovery, not a requirement on FORGET. Evidence: spec.md:156; design.md:350-351; decisions.md:215.
- **R3-N2 — Nit:** the probe requires a valid serving leaf, which `--insecure` does not load. Evidence: design.md:105-108; main.go:111-129.

Changes/resolution (all made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 3"):
- **R3-C1:** the #13 change states each test: the move and placement races keep the node Active and alive at their read, the hook committing FAIL, expiring `alive/`, then `ForgetNode` (each still fails with #78's gate compare removed); `TestForgetNodeRefusals` expects `node failed: ` for the move and placement and keeps the mark's own refusals; the other forget tests make the node Failed and not alive first; new refusals of `ForgetNode` for an Active node and a heartbeating Failed node.
- **R3-B1:** named in Risks and the runbook as a risk not accepted, with the operator's step (attach the volume, or `flaminio-meta maintain` and root ending the target once its other moves are done, #17 S14); withdrawing such a move is a follow-up (spec S20); the runbook's forget condition no longer requires no move naming the node.
- **R3-S1, R3-C2, R3-C4:** the per-tick checkpoint is cut. A tick credits its interval (1 s cap, none past 2 s) when its read of `alive/` shows a heartbeat committed within the last 5 s; the clock is written once a 5 s pass, only while a node is Missing. No certificate check or wall clock remains; a late clock write keeps its earned credit and the next pass anchors on the reread; at most one revision every 5 s during an outage, compaction as heartbeats already need (runbook, `docs/lab.md`, a follow-up for the lab's `etcd.sh`). Tests updated.
- **R3-C3:** spec S7 and the design apply "also Active" to every new maintenance ask, the drain's included, with a drain test; recorded as a change to #17 made by whichever of #17 and #45 lands second.
- **R3-N1:** worded as an analogy with the standard #82 sets for node-wide recovery; #82's evidence provider is not part of FORGET.
- **R3-N2:** with no certificate check, an `--insecure` instance credits from heartbeats like any other.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred.

### Plan review round 4 — d490961

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-C1 fixed in substance (two gaps: R4-4); R3-B1, R3-S1, R3-C3, R3-C4, R3-N1 and R3-N2 fixed; R3-C2 fixed in a new form (reply-loss rule: R4-6); R3-C3's file not listed (R4-N1).

- **R4-1 — Medium:** the new credit rule changes the user-settled S5 definition, while S5 and decisions.md say otherwise. Evidence: decisions.md:54-57, :60-61, the VM-clock paragraph, :308-311; design.md:105-119; spec S5.
- **R4-2 — Low:** the "last rise" has no defined start, so a stale heartbeat can credit after a new owner starts or a late tick. Evidence: design.md:105-107, :317.
- **R4-3 — Low:** any heartbeat credits, a Failed node's or an administrator's included, and this is not stated. Evidence: design.md:42-43, :118-120; spec S6.
- **R4-4 — Low:** #13's placement race depends on where `node-state/` is read, and `node T is forgotten` loses its test. Evidence: service.go:239-256; design.md (R3-C1 change list).
- **R4-5 — Low:** the per-second `List` of `alive/` reads #72's report values; the tick and the pass share state with no owner named. Evidence: meta/kv/kv.go:35; design.md:105-107.
- **R4-6 — Low:** the credit write's reply-loss rule is not stated. Evidence: design.md:131-134.
- **R4-N1 — Nit:** `control/drain/drain.go` is missing from design.md's file list. Evidence: design.md:19, :322.

Changes/resolution (all made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 4"):
- **R4-1:** round 3's rule kept: crediting only intervals that contain a heartbeat seen would stretch the wait by cluster size (heartbeats every 3 s, 1 s ticks; about 3× with one live node, 1.8× with two). Recorded plainly that the user-settled S5 item changed, for the user to confirm, with a note appended to the settled item; the VM-clock paragraph, spec S5's gloss ("at most 5 s of each outage"), the design and the ADR proposal corrected to match.
- **R4-2:** the newest revision and its rise start unset and reset after a failed read, a late tick or an ownership change; only a rise the owner saw counts. Tests added.
- **R4-3:** only keys of data nodes Active at the last pass count. Test: a Failed node's heartbeat alone credits nothing.
- **R4-4:** `nodes()` reads `node-state/` after `alive/` and before `drain/`, so the placement hook runs after the state read; new `TestForgottenRefusedWithoutFailed` keeps `node T is forgotten` covered on the move and placement paths. Both recorded as changes to #13.
- **R4-5:** a keys-only read (`kv.ListRevs`, etcd `WithKeysOnly`); the tick and the 5 s pass run in two goroutines sharing state under `clockMu`, held for in-memory updates only.
- **R4-6:** the write subtracts its snapshot of accrued credit when it commits, or when a reread finds exactly the intended value; otherwise the credit stays for the next write. Tests for both.
- **R4-N1:** `control/drain/drain.go` listed, with its #17 and #18 changes.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition changed (R4-1) and awaits the user's confirmation.

### Plan review round 5 — f902a18

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R4-1 to R4-5 and R4-N1 fixed (R4-1's bound off by one tick: R5-2); R4-6 partly fixed (R5-1).

- **R5-1 — Medium:** the lost-reply rule double counts credit when the lost write commits after the reread: the snapshot A "stays" although the late commit already stored it. Evidence: design.md "Persisting the credit" (~:136-148); the test rows cover only a lost reply after a commit and without one.
- **R5-2 — Low:** "at most 5 s of each outage" is up to 6 s: the rise is timed when the owner sees it, up to one tick after the heartbeat committed. Evidence: design.md ~:108-113; spec S5; adr-proposal.md; decisions.md.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 5"):
- **R5-1:** the write sets, never adds: the owner reads the clock's credit B0 on taking the ownership and keeps a monotone total T credited since; each write puts B0 + T, compared by sequence, and any outcome is followed by a reread of the sequence only. A late commit stores an older total that the next write replaces; nothing is subtracted. After an unknown outcome, timeout Failed decides against the clock as reread. Tests: lost reply with and without a commit; a write that times out, whose reread sees the old value, then commits late: persisted credit equals the true credit, once.
- **R5-2:** the bound is up to 6 s of each outage (5 s plus one tick of observation delay), stated the same in spec S5, the design, the ADR proposal, the settled-item note and the VM-clock paragraph; tested as stated.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (round 4, R4-1) still awaits the user's confirmation, now with the exact 6 s bound.

### Plan review round 6 — 7f66c24

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R5-1 fixed in substance (two edges: R6-1, R6-2); R5-2 fixed.

- **R6-1 — Low:** B0's read is not tied to the ownership commit, so a read before it can lower the stored credit; T's reset at a new ownership revision and a failed B0 read are unstated. Evidence: design.md:139-140, :275.
- **R6-2 — Low:** two of the three named tests pass under round 4's rule too, and the new owner's B0 case has no named test. Evidence: design.md:342.
- **R6-3 — Low:** "rereads the sequence only" conflicts with deciding timeout Failed "against the clock as reread". Evidence: design.md:144-148.
- **R6-N1 — Nit:** the 6 s is explained as "one tick" to see the rise, while a tick up to 2 s late still credits; the bound holds by the 1 s cap. Evidence: design.md:121.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 6"):
- **R6-1:** B0 is read after the ownership transaction commits; T starts at zero and B0 is reread at each new ownership revision; nothing is credited or written before the B0 read succeeds. Tests: an old owner's write in flight at the takeover, a regained ownership, nothing before the B0 read.
- **R6-2:** new test: a new owner's first write is exactly the previous stored credit plus its own T (fails if B0 is read before the ownership commits or T is not reset); the late-commit test asserts the older stored total and its replacement (the test round 4's rule fails); the lost-reply tests marked as regressions.
- **R6-3:** the pass rereads the whole clock; timeout Failed is decided against the stored clock, never the in-memory B0 + T; a lagging stored value only delays a failure.
- **R6-N1:** the 6 s explained as at most six ticks (the one that sees the last heartbeat and those in the 5 s after), each crediting at most 1 s, a late tick included; same wording in spec S5, the design, the ADR proposal and the settled-item note.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 7 — 08d9550

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R6-1, R6-2 and R6-3 fixed (R6-1's residual: R7-B1); R6-N1 fixed in wording (residual: R7-N1).

- **R7-B1 — Low:** "ownership revision" is ambiguous between an acquisition and each renewal; if a renewal were a put, T would reset and B0 be reread at every renewal, stalling the clock. Evidence: design.md:97, :144-145; the test "a regained ownership rereads B0" does not cover a renewal.
- **R7-N1 — Nit (the report's R7-B2):** "at most six ticks" assumes ticks 1 s apart; the 6 s bound holds because each tick credits its own interval, capped at 1 s. Evidence: design.md:121-123; decisions.md:424-428; spec S5; adr-proposal.md.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 7"):
- **R7-B1:** an acquisition is the conditional write that creates `rebuild-observer` or replaces another owner's, with a new nonce; a renewal is a keepalive of the key's lease (`kv.Renew`), which puts nothing, so the key's revision and nonce stay the acquisition's. T resets and B0 is read at an acquisition only, and the owner keeps both across a renewal. "Ownership revision" replaced in the design and annotated in round 6's entry. Test: a renewal changes neither B0, T nor the key's revision.
- **R7-N1:** the bound is explained as the sighting tick's interval, at most 1 s, plus the 5 s after the sighting, in spec S5, the design, the ADR proposal, the settled-item note and round 6's entry; "at most six ticks" dropped. The bound is unchanged.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 8 — 26b8a54

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R7-B1 fixed in intent (residuals R8-1 to R8-3); R7-N1 fixed.

- **R8-1 — Medium:** `kv.Renew` is named but not specified or listed: no signature, file list or tests; the acquisition's lease is not registered for it; a renewal after expiry could re-grant as `PutTTL` does; Mem would extend whatever is at the key, a new owner's included; the cadence against the 5 s TTL is unstated. Evidence: design.md:101, :354; meta/kv/kv.go:38-53; etcd.go:48, :118-146; mem.go:91-99.
- **R8-2 — Low:** the renewal test may not fail if a renewal resets T: its revision assertion cannot fail, and it does not span a renewal with credit on both sides. Evidence: design.md:354.
- **R8-3 — Low:** a lost lease is found only at the next compare, so the owner keeps crediting in memory. Evidence: design.md:144-147, :284-287.
- **R8-N1 — Nit:** a run-on line at design.md:101-103.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 8"):
- **R8-1:** `Renew` kept and fully specified, after checking the cut: the heartbeat's renewal exists only inside a TTL put, and a put per renewal would add steady etcd revisions (undoing R3-C4) and change the key's revision under every ownership compare. `Renew(ctx, key, rev) error` extends the TTL only while the key is the entry written at `rev`, else `kv.ErrLost`; it writes nothing and never grants; scoped to the acquisition in both stores. The lease rule is stated once: a committed transactional TTL put registers its lease under the key with its revision, for heartbeats and the acquisition. Cadence: every 1 s tick, in the tick's goroutine. `meta/kv/{kv.go,etcd.go,mem.go}` and the kvtest suite listed, with four cases on Mem and etcd.
- **R8-2:** the test credits x, renews, credits y, and expects the next write to store B0 + x + y with exactly one B0 read and no write gap; it fails if a renewal or a new key revision resets T or rereads B0.
- **R8-3:** the ownership ends at once on `ErrLost`, on a refused ownership compare (confirmed by a reread of the key), or after 5 s with no renewal succeeding: nothing more is credited, written or asked, and B0 and T are dropped. Test added.
- **R8-N1:** the paragraph is reflowed, with three other run-on lines.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 9 — c8d0892

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R8-1 fixed in specification (the etcd form misses its own scoping rule: R9-1); R8-2 fixed; R8-3 fixed in intent (residuals R9-2, R9-3); R8-N1 fixed.

- **R9-1 — Medium:** the etcd `Renew` does not check the key's revision: an overwritten or deleted key leaves its lease alive, so a displaced owner's keepalive succeeds for ever, etcd disagrees with Mem, and the kvtest case fails on real etcd. Evidence: design.md:112-116, :122-128, :381; mem.go:70-99; etcd.go:26-41.
- **R9-2 — Low:** the 5 s no-renewal bound equals the TTL and is measured at the answer, so the owner can outlive its key. Evidence: design.md:122-128.
- **R9-3 — Low:** the reread before a refused transaction counts against the ownership has no rule for a failed reread, and no test for an unchanged key. Evidence: design.md:122-128.
- **R9-4 — Low:** what `PutTTL` registers, and the registration when two puts to one key overlap, are unspecified; no test for a refused transaction's registration. Evidence: design.md:50-54; etcd.go:118-146.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 9"):
- **R9-1:** the etcd `Renew` first reads the key and requires `ModRevision == rev`, then `KeepAliveOnce`; only a key gone or at another revision, or `rpctypes.ErrLeaseNotFound`, is `ErrLost`, and every other error passes through. A displaced owner's leftover lease is dropped from the registry and left to expire, not revoked. The displaced-owner and deleted-key cases run on real etcd, with a second `KV`.
- **R9-2:** the bound is 4 s against the 5 s TTL, from the send time of the last renewal that succeeded (or the acquisition); test with a renewal answered late.
- **R9-3:** a reread showing another revision or none, or a failed reread (fail closed), ends the ownership; the acquisition's own revision leaves the ownership, B0 and T, and the refusal is another compare's. Tests for each.
- **R9-4:** `PutTTL` and a transactional TTL put register the lease and the put's revision; the registration is replaced under `e.mu` only by a committed put with a newer revision; a refused transaction leaves it and revokes the lease it granted. Tests for each.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 10 — db80c68

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R9-1 and R9-3 fixed; R9-2 fixed in design (the test and the evaluation point: R10-L1, R10-L2); R9-4 fixed (residual: R10-L3).

- **R10-L1 — Low:** the test "a renewal answered 3 s late" cannot happen: renewals run under a 1 s context. Evidence: design.md:~123 and the test row; etcd.go `retry`.
- **R10-L2 — Low:** where the 4 s bound is evaluated is not stated, so the owner could credit up to the TTL; and the acquisition may put on a lease extended long before, so "the key cannot expire before send + 5 s" is not guaranteed for it. Evidence: design.md (Cadence; the lease rule).
- **R10-L3 — Low:** dropping the registration "at rev" on `ErrLost` is unconditional, so a late `Renew` could delete a newer registration. Evidence: design.md (etcd `Renew`).

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 10"):
- **R10-L1:** the test is a renewal answered at its 1 s context limit, on a fake clock: the ownership ends 4 s after the send, 3 s after the answer; the send time is taken before `Renew` is called, so before its `Get`.
- **R10-L2:** the bound is checked under `clockMu` before every credit, every use of B0 or T and every write or ask. The ownership is held only from the first renewal that succeeds after the acquisition commits, and the bound runs from that renewal's send; chosen over a fresh lease for the acquisition because it reuses `Renew` and adds no second lease path. B0 is read once the ownership is held. Tests added.
- **R10-L3:** on `ErrLost` the registration is dropped, under `e.mu`, only if it is still the same lease and revision; test with a late `ErrLost` after a new registration.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 11 — c9e3472

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R10-L1, R10-L2 and R10-L3 fixed.

- **R11-L1 — Low:** the delay of the first credit is not stated: what happens to the time between the acquisition and the first successful renewal, and whether those ticks still advance the sighting baseline. Evidence: design.md:136-142, :201.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 11"):
- **R11-L1:** the ticks from the acquisition until the first renewal succeeds credit zero and still advance the tick time, the newest revision and its rise, so no later tick credits that span and none is dropped as late because of it; the delay is the renewal's round trip (at most its 1 s context) plus one tick per failed renewal, and only delays a failure. Both added to the "first renewal fails" test.

Status: needs_incremental_review. This change was made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 12 — 1da70f8

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R11-L1 fixed.

- **R12-L1 — Low:** the stated delay of the first credit is short by one tick: the renewal runs after the tick's read, so the tick in which it first succeeds has already credited zero. Evidence: design.md:136-137, :144-146; decisions.md (R11-L1).
- **R12-L2 — Low:** the "first renewal fails" test pins neither property: "at most 1 s" passes if the rise was not advanced, the span check needs failures over 2 s, and the delay is not asserted. Evidence: design.md:419.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 12"):
- **R12-L1:** the delay is stated as up to the next tick after the first renewal succeeds: the renewal's round trip (at most 1 s), plus that tick, plus one tick per failed renewal; in the design and in round 11's entry.
- **R12-L2:** the test has at least three failed renewals (over 2 s), a heartbeat rise seen during them, then a success; it asserts that the tick of the success credits zero and that the first credited tick is the next one, adding exactly its own 1 s interval.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 13 — bac60f2

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R12-L1 and R12-L2 fixed.

- **R13-L1 — Low:** the "first renewal fails" test does not pin the delay's round-trip term: it does not assert the first credit when the successful renewal is answered at its 1 s limit. Evidence: design.md:146, :418.
- **R13-N1 — Nit:** round 11's entry carries "(added in round 12, R12-L1)" inside the list of the delay's terms, easy to misread. Evidence: decisions.md:542-544.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changed after approval: plan review round 13"):
- **R13-L1:** a variant of the test in which the successful renewal is answered at its 1 s context limit: the first credit is still at the first tick after the answer, adding exactly its own interval.
- **R13-N1:** the parenthetical is now a sentence of its own: "Round 12 added the tick after the success to this delay: see below, R12-L1."

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13; no further approval is inferred. The user-settled S5 definition's change (R4-1) still awaits the user's confirmation.

### Plan review round 14 — e7dc21462ccc4f121569641a24f3bd64dd01e240

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
