# Plan review — issue #72, round 1, focus: design-code (plus plan code checklist on intent and spec)

Scope: `docs/issues/72/{intent,spec,design,capacity-contracts,adr-proposal}.md` in
`/tmp/flaminio-plans-2026-11-12/72`, checked against that worktree's code and the dependency
drafts (#13, #17, #18, #45, #50, #61, #70, #76, #78, #79, #80).

Not checked: the GitHub issue's own "Done when" (no network in this review); the goal was
judged against `intent.md`. The cited code facts in intent/design (`domain.go:302–398`,
`service.go:382–417`, `disks.go:158–182`, `replica/mod.rs:71,848–859`, `store.rs:99–122,2159`,
`engine.rs:241`, `kv.MaxTxnOps=120`, non-overlap of `data_used` and `data_quarantined`) are
correct.

## High

### R1-C1 — High — S5 does not say what admission does when the topology has no repair destination
Summary: with the reserve unit a node and strict node domains, a cluster whose data nodes equal
RF has no legal destination for any lost replica (RF=3 on 3 nodes; RF=2 on the 2-node edge site
of accepted ADR 0006). S5 says to "refuse new commitments that destroy certified protection"
and that reserve "cannot promise recovery from ... an impossible strict topology"; S3 requires
"protected E"; the design's certificate "cannot certify an infeasible one". Read one way, no
volume can be created on those clusters; read the other, E is silently zero. Neither is stated,
and the design claims ADR 0006 is consistent.
Evidence: spec.md S3 (46–51), S5 (60–65); design.md 89–95, 194; `docs/adr/0006-edge-two-node-degraded-writes.md:9,15`.
Fix: add a rule: a scenario with no legal destination by topology contributes no E and is
reported as `reserve infeasible: topology` with its generation; admission continues. Give the
S5 test row its expected outcome for N=RF and the 2-node edge profile.

### R1-C2 — High — S8's move rule selects no move on realistic disks
Summary: S8/design choose only moves that "reduce predicted spread by at least 1 point". A move
is one chunk replica (`MoveReplicaRequest` has one `chunk`). A fully written 256 MiB chunk
(CSI default, `control/csi/controller.go:137`) is 0.25% of a lab 100 GiB disk
(`docs/lab.md:11`), so one move changes spread by at most 0.5 points, and by nothing when two
devices tie at the maximum. Balancing would never start. Separately, the objective is physical
utilization, but nothing reports physical bytes per chunk replica (`SyncCapacity` has device
totals, `Assignment` has full logical charges), so the "predicted" physical improvement of a
move cannot be computed; a thin chunk moves no bytes.
Evidence: spec.md S8 (81–86); design.md 118, 125–126; capacity-contracts.md 64–67, 85–89.
Fix: make the threshold hysteresis on spread (start above tolerance, stop at tolerance minus a
margin) rather than a per-move gain, and state the predictor (logical charge, or add a
per-replica mapped-bytes report) and the convergence claim in the same unit.

### R1-C3 — High — the class lock is held across grant acknowledgements that arrive by 5 s polling
Summary: every create, grow, move, policy change and release is one class-serialized operation
whose step 3 waits for each touched device to acknowledge a grant whose hash binds that
operation's plan, so the grant changes on every operation. The only delivery path specified is
the `SyncCapacity` response (grant) and the next `SyncCapacity` request (ack), at "every 5 s".
That is 5–10 s per operation per class, against the design's own defaults of 1 start/s/class
and 8 active moves/class, and against #45: rebuilding one node's 16,384 chunk replicas
(1 TiB at 64 MiB) is 16,384 moves plus as many releases, about two days of admission alone.
One unreachable device "blocks or aborts" the class. Each loser of the class CAS must re-plan
and re-stage its pages, and each plan carries "certificate pages" with a witness per lost
replica per scenario, with no incremental form or size bound (S4, S11 ask for bounded work).
Q1 concedes throughput in general, not this arithmetic or the missing delivery mechanism.
Evidence: design.md 34, 58–76, 89–93, 119; capacity-contracts.md 64–69, 78–83, 113–115.
Fix: specify grant delivery (long poll or wake-up), skip the ack round when no limit changes,
allow a batch of moves per operation, bound the certificate (aggregate or incremental witness),
and restate the throttle defaults from the resulting rate; or take per-device escrow now.

### R1-C4 — High — P0 re-specifies #61's store identity without naming it, and S1's replaced-store rule has no simulator check here
Summary: #61's draft owns "a known node that returns on another store never counts in an ISR":
`ReplicaRef`, protocol v2 Hello `device_uuid`, a `LostStore` simulator fault and an
`identity_alias` injected bug, snapshot-paged `KV.ListPage` "reusable by #72". #72 never
mentions #61, lists the same work as its own P0, and differs from it: `ListReplicaAssignments`
/`Assignment` here against `AuthorizeReplicaSession`/`SessionAssignment` there; no actual UUID
in `Fenced` here; administrator `RegisterDevice` here against `Node.store_uuid/store_generation`
there. #72's tests for "missing/replaced stores are not silently substituted" are capacity
vectors and store tests only: a replication safety rule with no simulator check or injected
bug (AGENTS.md Principles). #45 and #61 both put several disks per node out of scope.
Evidence: spec.md S1 (33–38); design.md 17, 21–32, 164; capacity-contracts.md 71–73, 85–89,
126–130; `/home/jsfakian/Documents/src/flaminio-61/docs/issues/61/design.md:12–13,38–41,58–63,123–145,155`;
`/home/jsfakian/Documents/src/flaminio-61/docs/issues/61/intent.md:9–12,120`; `docs/issues/README.md` ("a shared root cause ... in an issue of its own").
Fix: make #61 (or the identity issue it moves to) the prerequisite that owns identity, session
authorization, wire v2 and their simulator check; keep in #72 only what capacity adds
(several stores per node, class, charges), on that contract.

## Medium

### R1-C5 — Medium — "min ISR and RF count distinct hosts" is a safety rule outside the spec and the reference model
Summary: the same-host shadow rule appears only in design, contracts and the ADR. The Go check
counts members (`uint32(len(isr)) < h.MinISR`), and the table it mirrors is the Rust model
`flaminio-repl/src/meta.rs`, exported by `make meta-vectors`. The design's files omit
`meta.rs`, `replica.rs`, `types.rs` and the vectors, name no injected bug (for example, a
shadow counted toward min ISR), and its Contracts line lists `make proto` and `make contracts`
only.
Evidence: design.md 26–29, 149–150, 155, 167–168; capacity-contracts.md 8–12, 32;
`meta/domain/domain.go:238`; `dataplane/crates/flaminio-repl/src/meta.rs:1–27`; `Makefile:105–106`.
Fix: add a spec rule; change the model in `meta.rs` first, regenerate the vectors, and name
the simulator scenario and its mutation.

### R1-C6 — Medium — the protobuf tags are not collision-free
Summary: `Volume device_class=20, data_locality=21` collide with #50's `Volume encryption=20,
key_source_storage_mode=21`; `CreateVolumeRequest device_class=9, data_locality=10` collide
with #79's `CreateVolumeRequest.use_profile=9`. The contracts call the assignments
"collision-free" and list only #13/#18's tag 9 (also claimed by #17 and #79).
Evidence: capacity-contracts.md 23–25, 36–38;
`/home/jsfakian/Documents/src/flaminio-50/docs/issues/50/contracts.md:37–38`;
`/tmp/flaminio-plans-2026-11-12/79/docs/issues/79/initialization-contract.md:45`;
`/home/jsfakian/Documents/src/flaminio-17/docs/issues/17/design.md:185`.
Fix: take tags from one shared registry across the batch and drop the "collision-free" claim.

### R1-C7 — Medium — the refs exceed #80's bound for certain, and nothing covers volumes above the reduced cap
Summary: the plan says #80's B is tightened "if those additions exceed the bound". They do:
#80 budgets 288 + 264(R+1) bytes per chunk; keeping the node projections and adding
`replica_refs`, `isr_refs` (each ref about 173 bytes with a 128-byte node id and a 36-byte
UUID) and two moving refs adds about 346(R+2). For R=3 that is about 3,070 bytes against
1,344, so N is about 1,360 instead of 3,119 (my estimate), before `released_refs` and
`fenced_refs`, which have no count bound. #80 requires "bounded complete shapes" and that
existing volumes are not made unreadable at profile enablement; #72 has no rule for a volume
admitted under the old cap.
Evidence: capacity-contracts.md 14–18, 24, 39–41; design.md 157–158;
`/tmp/flaminio-plans-2026-11-12/80/docs/issues/80/design.md:29–35`, `.../80/contracts.md:44–49`.
Fix: bound every repeated ref field, state the revised formula and caps (or drop the node
projections from the wire), and say what happens to existing volumes above the new cap.

### R1-C8 — Medium — the NoSpace target stop has no exit when the full replica stays in the ISR
Summary: on NoSpace the target deposes itself and the ISR may not shrink. Deposition kills the
ublk device (`flaminio-io/src/ublk.rs:619–629`), so the "affected write gets ENOSPC" rule ends
the whole device. The newer-epoch attach must reconcile the uncertain write onto every ISR
member, which allocates on the same full replica, gets NoSpace and stops again ("a denied copy
... stops the target too"). Moving the replica off the disk also needs a target
(`meta/domain/domain.go:291`). The design names no way out other than freeing space on that
disk, and no owner for the re-attach. Q2 records the availability trade-off, not this loop.
Evidence: spec.md S6 (67–73); design.md 107–114.
Fix: specify the exit (for example an epoch-checked `ChangeIsr` that drops the full member
above min ISR, or reconcile allocation under the recovery limit), who re-attaches, and a
simulator scenario that reaches readiness again.

### R1-C9 — Medium — the budget formulas do not say per device or per class, and the boundary is stated three ways
Summary: `L+P <= floor(overcommit*(soft*T-E))` has no subscripts. The spec charges "against
its device class" and tests "the class overcommit budget"; a class-wide test lets one device
be promised more than it holds at ratio 1.0, which the local limit then turns into ENOSPC.
The boundary is "inclusive at the threshold" (S3), `<=` (formula), "below soft/high" and
"comparisons at threshold refuse additional work" (design), so the "each watermark ±1" test
has no single expected value.
Evidence: spec.md 25–26, 46–51; design.md 50–54.
Fix: state the per-device and per-class conditions separately with one comparison operator
each, and a worked boundary example.

### R1-C10 — Medium — locality's default and values differ from ARCHITECTURE §8.2, and `prefer_node` is not addressed
Summary: S9 makes `dataLocality` default `disabled` with values disabled/preferred.
ARCHITECTURE §8.2 documents default `preferred` and a `required` value; the Docs section
updates §9–11 only. Creation already places a replica on the first consumer's node through
`prefer_node` (`control/csi/controller.go:233`, `Place`'s `prefer`), which is also part of
#76's create identity. The plan does not say whether `disabled` turns that off, nor what
`required` returns.
Evidence: spec.md S9 (88–92); design.md 32–33, 184; `docs/ARCHITECTURE.md:443`;
`meta/domain/domain.go:355–369`.
Fix: state the default's relation to §8.2 (and update it), refuse `required` explicitly, and
define `data_locality` against `prefer_node`.

### R1-C11 — Medium — a data-plane restart during a metadata outage can no longer fence, route or allocate
Summary: grants and routes are not persisted ("no stale local grant file is authority", "No new
on-disk records") and allocation stays disabled until both are fetched; assignments are fetched
"before Hello creates/fences replicas". Today a restarted replica needs only the target's
token. ARCHITECTURE §8.5 promises that in a metadata outage only chunks that lost a replica
block; with this design they stay blocked after the replica returns, and a fence waits on
metadata.
Evidence: design.md 24, 99–100, 156; capacity-contracts.md 82–83; `docs/ARCHITECTURE.md:518–519`;
`dataplane/crates/flaminio-net/src/session.rs:3–11`.
Fix: say which state a restarted node may use without metadata (for example chunks already in
a store route to it and fence at once; the last grant persists as an upper bound that only
tightens), or record the regression as an open risk with its test.

### R1-C12 — Medium — one design for four phases resting on plans that do not exist yet
Summary: the intent names no step of the issue; the design covers P0–P3 and needs hooks from
#13, #17, #18, #45, #70, #76, #78 and #80. #45 has no design and puts its reserve step out of
scope; #70 is a draft. Test rows such as "#45 consumes the same charge" and "#70 must call this
protocol" cannot be written against anything. The test table is not split by phase, so no
phase can be shipped and verified on its own, while the spec says partial phases leave their
capabilities disabled.
Evidence: intent.md 4, 39–40; spec.md S12 (106–111); design.md 15–19, 166, 198–199;
`/home/jsfakian/Documents/src/flaminio-45/docs/issues/45/intent.md:117`; `docs/issues/README.md` ("The issue's step").
Fix: make this plan one step (for example the ledger and create admission on one store per
node), with its own rules and tests; move the other phases to their own steps or issues.

### R1-C13 — Medium — the test plan names categories, not tests, and S11 cannot fail
Summary: apart from `meta/service/capacity_test.go` no test, simulator scenario file or
mutation is named (compare #78's `sim/placement_admission.rs` and shared JSON schedules). S11
is "measure ... publish": no threshold, so no result fails it. S8's "converge within
tolerance" has no bound on moves or time for the test to assert.
Evidence: spec.md 100–104, 113–125; design.md 160–173.
Fix: name the simulator module, scenarios and mutations per safety rule, and give S8 and S11 a
pass criterion (a bound, or an explicit "measurement only, not acceptance").

## Low

### R1-C14 — Low — a full plan page needs 121 compares
Summary: "no page holds more than 119 mutation keys plus root" gives 120 writes, but each step
also compares the class head, the root and every touched ledger revision: 121 compares, which
`meta/kv/etcd.go:91` refuses (`MaxTxnOps = 120`).
Evidence: capacity-contracts.md 112–115; design.md 65, 134.
Fix: cap a page at 118 ledger keys, or state which keys need no compare.

### R1-C15 — Low — NoSpace is specified in network byte order; frames are little-endian
Evidence: capacity-contracts.md 132; `dataplane/crates/flaminio-net/src/frame.rs:1,343`.
Fix: little-endian `req`, as every other frame. (The 10-byte body, tag included, is right.)

### R1-C16 — Low — two components the plan relies on do not exist
Summary: "TCP connections may remain pooled per node pair": today one connection is one
session, with no pool (`flaminio-net/src/session.rs:1`, `frame.rs:3`). "The actual node
bulk-copy dispatcher": bulk work is limited per target by a count, `max_bulk`
(`flaminio-repl/src/target.rs:393–432`); there is no per-node byte-rate limiter, and the
design (node dispatcher) and contracts (each disk dispatcher) place it differently.
Evidence: design.md 119–121; capacity-contracts.md 129–130, 135–136.
Fix: describe both as new work, and say on which node (target, source or destination) the
64 MiB/s limit applies.

### R1-C17 — Low — "copying waits for ready" has no mechanism
Summary: step 4 publishes the move in the chunk record atomically with the decision; a target
that refreshes its view starts the move at once. Nothing says what holds it until step 5's
`ready` (the destination refusing Hello, and the target's retry, or a later write of `moving`).
Evidence: design.md 69–75; capacity-contracts.md 87–88.
Fix: name the gate and the target's behaviour while it is closed.

### R1-C18 — Low — #76's garbage collection can revoke a create while its capacity operation holds the class
Summary: #76 deletes a sealed, unadmitted root "unchanged in two observations" of its 5 s pass.
The capacity prepare (steps 2–3) keeps the root sealed and unadmitted for at least that long
(R1-C3). The commit then fails its root compare and aborts; a large create can repeat this.
Evidence: design.md 62–72, 82; `/tmp/flaminio-plans-2026-11-12/76/docs/issues/76/design.md:101–105,118`.
Fix: state the rule that keeps a root referenced by an active capacity operation out of #76's
collection, or admit under #76 first.

### R1-C19 — Low — when a recovery assignment stops using the recovery limit is not stated
Summary: `Assignment.purpose` lets rebuild/drain allocations pass the normal limit. If the
purpose outlives the copy, later client writes to that replica spend reserve; if it ends at
`active`, writes arriving during resync are split between limits.
Evidence: design.md 101–102; capacity-contracts.md 85–89.
Fix: define the transition and test it.

### R1-C20 — Low — the lab plan is not runnable as written
Summary: no `make`/`scripts/lab.sh` command or `scripts/checks/` entry is named, and each lab
VM has one data disk (`docs/lab.md:11,165`) while `agent.Pick` refuses several.
Evidence: design.md 175–180.
Fix: name the commands, the check script and how the extra disks are provided, or scope the
lab check to one disk per node.

## Nits

- design.md 22–23: "a metadata assignment maps (volume first_epoch,chunk,node) to UUID" cannot
  hold the two refs of a same-node move; the key at line 44 includes the UUID. Use one wording.
- capacity-contracts.md 24: `Volume.fenced_refs=22` and `capacity_generation=23` have no rule
  or use in spec or design.
- capacity-contracts.md 46, 53: "uint32 for ppm" but `overcommit_ppm` is u64; and
  `OpenCapacityReport.expected_revision` does not say of which key.
- spec.md S9: "held consumer" and "held epoch" are #17's terms, absent from Terms.

Reviewed commit: 813d9a5cb1d210c427c7afe8fa73cc1f3669fd7a
