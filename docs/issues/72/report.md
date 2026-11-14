# Issue #72 — report

## Review rounds

### Plan review round 1 — 813d9a5cb1d210c427c7afe8fa73cc1f3669fd7a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72c1 (design-code), r72s1 (design-security).

- **R1-C1 — High:** S5 does not say what admission does when the topology has no repair
  destination (RF = node count, ADR 0006's 2-node edge). Evidence: spec S3/S5, design 89–95,
  `docs/adr/0006-edge-two-node-degraded-writes.md:9,15`.
- **R1-C2 — High:** the balancing rule (≥ 1 point per move) selects no move on real disks, and no
  per-replica physical figure exists to predict it. Evidence: spec S8, design 118–126,
  `control/csi/controller.go:137`, `docs/lab.md:11`.
- **R1-C3 — High:** the class lock is held across grant acknowledgements that arrive by 5 s
  polling; rebuild throughput, contention and certificate size are unbounded. Evidence: design
  34, 58–93, 119; contracts 64–83, 113–115.
- **R1-C4 — High:** P0 re-specifies #61's store identity under other names, and the replaced-store
  rule has no simulator check here. Evidence: spec S1, design 17–32, #61 design 12–155.
- **R1-C5 — Medium:** "minimum ISR and RF count distinct hosts" is a safety rule outside the spec
  and the Rust model. Evidence: design 26–29, `meta/domain/domain.go:238`, `flaminio-repl/src/meta.rs`.
- **R1-C6 — Medium:** protobuf tags collide with #50 (`Volume` 20/21) and #79
  (`CreateVolumeRequest` 9). Evidence: contracts 23–25, 36–38; #50 contracts 37–38; #79
  initialization contract 45.
- **R1-C7 — Medium:** the refs exceed #80's bound; nothing covers volumes above the reduced cap.
  Evidence: contracts 14–18, 39–41; #80 design 29–35.
- **R1-C8 — Medium:** the NoSpace target stop has no exit while the full replica stays in the
  ISR. Evidence: spec S6, design 107–114, `flaminio-io/src/ublk.rs:619–629`.
- **R1-C9 — Medium:** budget formulas do not say per device or per class; the boundary is stated
  three ways. Evidence: spec 25–26, 46–51; design 50–54.
- **R1-C10 — Medium:** locality's default and values differ from ARCHITECTURE §8.2; `prefer_node`
  is not addressed. Evidence: spec S9, `docs/ARCHITECTURE.md:443`, `domain.go:355–369`.
- **R1-C11 — Medium:** a data-plane restart during a metadata outage can no longer fence, route or
  allocate. Evidence: design 24, 99–100; `docs/ARCHITECTURE.md:518–519`.
- **R1-C12 — Medium:** one design for four phases resting on undesigned plans; the intent names
  no step. Evidence: intent 4, 39–40; spec S12; design 15–19, 198–199.
- **R1-C13 — Medium:** the test plan names categories, not tests; S11 cannot fail. Evidence: spec
  100–125, design 160–173.
- **R1-C14 — Low:** a full plan page needs 121 compares. Evidence: contracts 112–115,
  `meta/kv/etcd.go:91`.
- **R1-C15 — Low:** NoSpace in network byte order; frames are little-endian. Evidence:
  contracts 132, `flaminio-net/src/frame.rs:1,343`.
- **R1-C16 — Low:** connection pooling and a per-node bulk dispatcher do not exist. Evidence:
  design 119–121, contracts 129–136.
- **R1-C17 — Low:** "copying waits for ready" has no mechanism. Evidence: design 69–75,
  contracts 87–88.
- **R1-C18 — Low:** #76's garbage collection can revoke a create while its capacity operation
  holds the class. Evidence: design 62–82; #76 design 101–105, 118.
- **R1-C19 — Low:** when a recovery assignment stops using the recovery limit is not stated.
  Evidence: design 101–102, contracts 85–89.
- **R1-C20 — Low:** the lab plan is not runnable as written (no command; one disk per lab VM).
  Evidence: design 175–180, `docs/lab.md:11,165`.
- **R1-C-N1 — Nit:** two wordings of the assignment key. **R1-C-N2 — Nit:** `fenced_refs` and
  `capacity_generation` have no use. **R1-C-N3 — Nit:** ppm types and `expected_revision`'s key.
  **R1-C-N4 — Nit:** #17's terms in S9 without definition.
- **R1-S1 — High:** one node can hold a device class closed, with nothing ending the wait.
  Evidence: design 36, 62–75, 100–101.
- **R1-S2 — High:** "minimum ISR and RF count distinct nodes" has no injected bug, simulator check
  or vectors. Evidence: contracts 9–12, design 26–29, `meta/domain/domain.go:238`.
- **R1-S3 — Medium:** replica-side route enforcement is a principle, not a mechanism; mid-session
  chunk creation is not covered. Evidence: contracts 30, 127–128;
  `flaminio-node/src/replica/mod.rs:267–294,347–349`; `flaminio-net/src/token.rs:116–131`.
- **R1-S4 — Medium:** when a replica may send `NoSpace` and a target act on it is open. Evidence:
  design 99–113, contracts 133–134, `docs/ARCHITECTURE.md:518–519`.
- **R1-S5 — Medium:** no enforcement or detection against an old metadata-service instance after
  activation. Evidence: design 192–194, contracts 146–148, `meta/service/service.go:414–417`.
- **R1-S6 — Medium:** `Volume` tags 20/21 collide with #50. Evidence: contracts 24, 36; #50
  contracts 37–38.
- **R1-S7 — Medium:** the ref fields exceed #80's bound; two repeated fields have no count limit.
  Evidence: contracts 14–17, 39–41; #80 design 30–33, contracts 48–49.
- **R1-S8 — Medium:** administrator-registered totals can never be checked against the disk.
  Evidence: design 37–38, contracts 49–70.
- **R1-S9 — Low:** `SyncCapacity` has no rate bound; report age has no defined clock. Evidence:
  design 38, 42–43; contracts 68.
- **R1-S10 — Low:** `NoSpace` in network byte order. Evidence: contracts 132,
  `flaminio-net/src/frame.rs:156–165,561`.
- **R1-S11 — Low:** ALPN alone does not refuse an old peer. Evidence: contracts 126–127,
  `flaminio-net/src/tls.rs:71–93`.
- **R1-S12 — Low:** new strings used as key segments, labels and CLI arguments have no validation
  rule. Evidence: design 42, contracts 7, 102, 138–141.
- **R1-S13 — Low:** caller-controlled paging inputs lack rules and tests. Evidence: contracts
  71–73, 100–102.
- **R1-S14 — Low:** locality priority can be claimed by pod churn. Evidence: design 119–121.
- **R1-S-N1 — Nit:** no connection pooling exists. **R1-S-N2 — Nit:** the threshold is stated
  inclusive and exclusive. **R1-S-N3 — Nit:** decided or ready authorizes a route.

Changes/resolution:
- **R1-C12:** the plan is now one step, named in the intent: the issue's step 1 with the
  admission half of step 2, on one store per node. A per-node ledger charged in the placement
  transaction; creates refused from soft (80 %), moves (drain, rebuild, administrator) admitted to
  hard (95 %); reports with the heartbeat; `InitCapacity` for existing nodes; `ListNodes` shows
  it. The intent lists what stays open (configurable per-class policy, overcommit above 1.0,
  backpressure, the reserve with #45, growth with #70, rebalancing, locality, metrics, several
  disks per node after #61, measured limits). Spec, design, contracts and ADR proposal are cut to
  it; later phases' rules, contracts and fields are gone, not guarded.
- **Resolved by the cut** (the text they were on no longer exists): R1-C1 (no reserve
  certificate), R1-C2 and R1-S14 (no balancing or locality), R1-C3, R1-S1 and R1-C18 (no class
  operation or grants: each placement compares and writes its ledgers in its own transaction),
  R1-C4, R1-S3, R1-S11, R1-S13 (no device identity, routes, Hello v2, ALPN or paged assignment
  listing; identity stays #61's), R1-C5 and R1-S2 (no refs: replicas and ISRs stay node lists,
  `CheckIsrChange` and the vectors unchanged, so no new membership rule), R1-C7 and R1-S7 (no
  `Volume`, `ChunkMeta` or `IsrChange` field: #80's bound untouched), R1-C8, R1-S4, R1-C15,
  R1-S10, R1-C11, R1-C19 (no `NoSpace`, grants or recovery limits: out-of-space stays today's
  replica failure), R1-C14 and R1-C17 (no plan pages or ready state; every new transaction's
  compares and writes are in the contracts table, all ≤ 120), R1-C16 and R1-S-N1 (no pooling or
  dispatcher), R1-S12 (no new string inputs: no device class, reserve domain, purpose or state),
  R1-C-N1–N4 and R1-S-N3.
- **R1-C6, R1-S6:** the plan now adds only `NodeHeartbeatRequest.capacity = 72` and
  `Node.capacity = 72`, plus new messages and `InitCapacity`; checked against #45 (`Node` 6–10)
  and #61 (`Node` 20–21). No device class field; `CreateVolumeRequest` and `Volume` unchanged.
- **R1-C9, R1-S-N2:** one per-node rule, `max(L, U) + n × c ≤ floor(T × w / 1,000,000)` and the
  same for slots, equality admitted, with a worked example (spec S5); `domain.Room` is its only
  implementation.
- **R1-C10:** locality is cut and ARCHITECTURE §8.2 untouched; `prefer_node` is defined against
  capacity: kept only if the node's room holds a replica of every chunk, else ignored as today.
- **R1-C13:** every rule maps to named tests (file and case) in the design, including interleaved
  creates, moves, completions and confirmations on both stores that fail without the ledger
  compares; the old S8 and S11 are cut. The design states why no simulator check is owed.
- **R1-C20:** `make capacity-check` (`scripts/lab.sh capacity-check`) on one disk per node, fl-a
  and fl-b only, metadata-only RF 1 volumes, with steps, pass criteria and cleanup through
  `delete_volume`; `make meta-smoke` reports and initializes.
- **R1-S5:** not excluded by a mechanism in this step (decisions.md): an old instance's
  placements are uncharged, and the worst outcome is today's replica failure, never a lost
  acknowledgement. The upgrade order replaces every instance before `capacity-init`, whose rows
  name a recorded charge above the ledger. Open as Q3.
- **R1-S8:** no administrator-typed totals: the node reports its store's layout figures with its
  heartbeat, validated (S1).
- **R1-S9:** no new RPC or write path: the report is the liveness record's value and expires with
  it, so freshness needs no clock; the heartbeat rate exposure is today's.
- **Found while fixing:** keeping a completed move's source charged would grow on every move back
  (the lab's drain test) with nothing to release it, so `ChangeIsr`'s completing change releases
  it in the same transaction; data no record charges counts through physical use, with its gap
  stated as Q2.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 8ad446a17f420736478593a9dd81afe2af160770

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72c2 (incremental design-code), r72s2 (incremental design-security).

- **R2-S1 — High / R2-C1 — Medium (one defect):** the completed-move test ("`Moving.From` not in
  `NewReplicas`") holds for every ISR-only change, which carries no replicas, so a move's source
  was released at least twice and its ledger could underflow. Evidence:
  `flaminio-node/src/engine.rs:1818–1824`, `meta/service/service.go:761–762`,
  `meta/domain/domain.go:276–279`, design.md:64–66.
- **R2-C2 — Medium:** where a ledger is read absent nothing is compared, so a concurrent
  `InitCapacity` leaves a charge nothing releases; the three listings can count a deletion twice,
  permanently; `ConfirmDeletion` with an unreadable ledger is unspecified. Evidence:
  capacity-contracts.md table, design.md "ChangeIsr", "Initialization".
- **R2-C3 — Medium:** test helpers, `domain_test.go`'s `NodeLoad` literals and lab scripts that
  create break without reports and ledgers, and are not in Files touched. Evidence:
  `meta/service/service_test.go:58`, `meta/domain/domain_test.go:146–302`, `scripts/checks/`.
- **R2-C4 — Low:** every create attempt rescans all chunk records. Evidence: design.md
  "createVolume" step 2.
- **R2-C5 — Low:** contention, `ChangeIsr` unchanged, unreadable ledgers and the drain's
  `FAILED_PRECONDITION` have no named test. Evidence: spec Tests, design Tests.
- **R2-C6 — Low:** lab step 4 depends on the disk size, which the plan does not state.
  Evidence: design.md "Lab".
- **R2-C nits:** 116 is stated as the limit though the claim fits 118; "56 sources" should be
  "56 distinct source nodes".
- **R2-S2 — Medium:** an interrupted creation that is then deleted leaks its unwritten chunks'
  charges permanently; "until #76 recovers it" is false. Evidence: `service.go:435–444, 539–570`,
  intent.md:51–52, spec S4 and Q2.
- **R2-S3 — Medium:** releasing a source at completion while its copy stays lets a drain then
  undrain admit creates on top of the copies under `max(L, U)`, about 150 % of the disk.
  Evidence: `docs/followups.md:244–249`, `service.go:384–396`, `domain.go:395–404`, spec S3, S5.
- **R2-S4 — Low:** older binaries do not ignore the tombstone's `charges`: their
  `ConfirmDeletion` rewrites the record without it; Q3's detection claim has no test. Evidence:
  `service.go:72–75, 635–661`, design.md:202.
- **R2-S5 — Low:** an unreadable ledger blocks its node permanently; `ConfirmDeletion` does not
  say what it does with one. Evidence: capacity-contracts.md:8–10, design.md:73–76.
- **R2-S6 — Low:** administrators can still type totals (`heartbeat` figures, S10, `meta-smoke`),
  contrary to the R1-S8 resolution. Evidence: capacity-contracts.md:108–109,
  `scripts/lab.sh:529–534, 612`, decisions.md:25–26.
- **R2-S7 — Low:** removing nodes without room from the candidates makes a full zone drop out
  of `resolveDomain`, so `bestEffort` falls back. Evidence: `domain.go:343–352, 363`.
- **R2-S nit:** an invalid report warns on every heartbeat.

Changes/resolution:
- **R2-S1, R2-C1, R2-S3:** a completed move no longer releases anything. It records its source
  as a left copy (`left` on the chunk record, not in `ChunkMeta`), still charged, as the copy
  stays on the disk. A move back to a left copy reuses it at no charge. A volume's tombstone
  names its left copies, so the sweep deletes them and the confirmation releases them. The
  completion test is `ApplyIsrChange`'s own, in one `domain.CompletedMove` both use. `ChangeIsr`
  writes no ledger, so a double release cannot happen. `TestCompletedMoveOnly` covers an
  ISR-only change, the join, a removal during a move, the add step, and join plus drop in one
  request; only the drop records a left copy, once, and no ledger changes. The admission rule
  is restated once in spec S5: the ledger and the physical use, each plus the charge, within
  the budget. L now bounds physical use plus unwritten commitments for every named copy. The
  worked example is re-derived with a second, drain-then-undrain example, tested by
  `TestDrainUndrainCreates`. Q2 is closed; copies no record names from before this change are a
  stated risk, and the lab check refuses such a node.
- **R2-S2:** the claim writes its per-node charge to `capacity-claims/<volume>`. The last header
  write deletes it; deleting a creating volume moves it into the tombstone and names its nodes.
  `TestDeleteInterruptedCreationReleasesAll` covers it. The "until #76" wording is gone: #76
  must keep this key or derive the same charges from its sealed plan.
- **R2-C2, R2-S4, R2-S5:** `InitCapacity` marks its targets, takes one snapshot of
  `/flaminio/v1/` (a deletion is in it once), and writes comparing the mark. `ConfirmDeletion`
  compares the ledger in every state (0 when absent) and rewrites a mark, so a racing
  confirmation counts once (`TestInitRacingConfirm`). `all` recomputes every node: the way out
  of an older binary's rewrite, a leaked charge or an unreadable ledger
  (`TestInitAllRepairs`, `TestConfirmByLedgerState`). The design no longer claims older binaries
  ignore the new fields; Q3 says what they drop.
- **R2-S6:** a report is accepted only from the node's own `io` identity; an administrator's or
  agent's is `PERMISSION_DENIED`. `meta-smoke` uses the nodes' real reports. `meta-interop.sh`
  heartbeats each node as itself. decisions.md's R1-S8 entry is corrected.
- **R2-S7:** nodes without room stay candidates for failure domains; a full zone refuses with
  `RESOURCE_EXHAUSTED` under either policy (`TestPlaceRoom`).
- **R2-C3:** the helpers and their lines are listed, and `Place` takes a room map (nil for the
  domain tests). `scripts/lab.sh meta up` runs `capacity-init`. `resync.sh` already needs the
  nodes up. The stubbed `lab-deletion-test.sh` is unaffected. `guest/meta.sh` and `confirm.sh`
  create nothing.
- **R2-C4:** the chunk scan runs once per create; attempts reread nodes, reports and ledgers.
- **R2-C5:** named `TestCreateRetriesOnLedgerConflict`, `TestCreateContention`,
  `TestChangeIsrWritesAsBefore`, `TestUnreadableLedger` and
  `TestDrainSkipsAnUnreportedDestination`.
- **R2-C6:** the lab's 100 GiB disk is stated; step 1 fails with fl-a's figures if its budgets
  leave less than a chunk at soft or between soft and hard.
- **R2-C nits:** 116 is called a margin under 118. The ChangeIsr bound and its "56 sources"
  test are gone with the ledger writes.
- **R2-S nit:** the warning is logged each time a node's report turns invalid, per instance.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 4aacc8e34c72106dbf566c4952fa6241fab82377

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72c3 (incremental design-code), r72s3 (incremental design-security). The plan was
approved before this round (bbdb51d); these fixes come after the approval.

- **R3-S1 — High / R3-C2 — Low (overlap):** a left copy is released only by its volume's
  deletion, so a replaced disk or a hand deletion strands its charge; an interrupted creation's
  claim is invisible; and #13's approved plan already names the same copy `released`, refuses
  the move back and confirms deletion in a transaction already at 120 compares. Evidence:
  `service.go:870–903`, `flaminio-13/docs/issues/13/design.md:58–78`, design.md:52, 63–66, 105.
- **R3-C1 — Medium / R3-S3 — Low (one defect):** `InitCapacity` restored a blocked node against
  the snapshot's mark revision, so a confirmation between mark and snapshot was lost. Evidence:
  design.md "Initialization" step 4, contracts:40.
- **R3-S2 — Medium:** `--all` does not repair a tombstone an older binary rewrote while it is
  pending; `TestInitAllRepairs`, Compatibility, decisions and Q3 claimed it did. Evidence:
  `service.go:72–75, 635–661`, spec S6, design.md:108–109, 156, 213–214.
- **R3-C3 — Low:** a left copy makes a drained or retired node part of every later deletion;
  the administrator's confirmation is not documented or tested. Evidence: `service.go:105–113`,
  `auth.go:77`.
- **R3-C4 — Low:** the in-memory store enforces no 120-operation limit; the absent-ledger race,
  concurrent initialization, 32 failed batches and the claim-key overwrite are untested.
  Evidence: `meta/kv/etcd.go:91`.
- **R3-C5 — Low:** `meta up` would run `capacity-init` once per VM and from `chaos`, its output
  hidden; a move back to a left copy demanded a report though it charges nothing. Evidence:
  `scripts/lab.sh:1407`.
- **R3-C6 — Low / R3-S nit 2:** design, contracts and spec still called settled questions open or
  Proposed. Evidence: design.md:4, 215, 222; capacity-contracts.md:1, 4; spec.md:142.
- **R3-S4 — Low:** an interrupted, `ABORTED` or raced initialization leaves nodes marked, out of
  placement, and the CLI exits 0. Evidence: design.md:99–114, contracts:124–125.
- **R3-S5 — Low:** a node's figures are unbounded; Risks understates an inflated or a zero
  report. Evidence: spec S1, design.md:87–89, 208–210.
- **R3-S6 — Low:** a move back to a left copy skipped the physical-use guard. Evidence:
  design.md:63–66.
- **R3-S7 — Low:** a nil room map meant no limit, failing open. Evidence: design.md:85–86.
- **R3-C nits:** `CompletedMove`'s caller; lab step 3's "exactly R" depends on step 1's check.
- **R3-S nits:** "a running initialization rereads"; "leaves `Left`" read as keeping it.

Changes/resolution:
- **R3-S1, R3-C2, R3-S6:** #72's `left`, `CompletedMove` and free move back are cut. The
  moved-away copy is #13's released copy, charged until the node confirms deleting it. A move to
  a released node is refused by #13's S2 before any charge, so no move skips the guard. The
  release rule is stated once (`releaseCharge`). `ConfirmDeletion` applies it, and #13's
  `ConfirmRelease` must apply it in its own transaction, at 118 chunks at most. Spec S11 states
  the dependency on #13 by number and what holds meanwhile. A released copy deleted by hand or
  lost with a replaced disk is released when #13's sweep confirms it absent. Replicas still named
  on a replaced disk stay charged until moved off (Risks). `InitCapacity`'s answer names the
  volumes holding a claim, and followups says to delete an interrupted one.
- **R3-C1, R3-S3:** every write of `InitCapacity` compares the revision of this call's own mark
  (`TxnRev`). Any rewrite (a release, another call) leaves the node marked for a rerun.
  `TestInitRacingConfirm` covers a blocked node and an absent ledger.
- **R3-S2:** the plan now says it plainly: a tombstone rewritten by an older binary keeps its
  pending nodes blocked, with their ledgers unchanged; run `--all` again once it is gone.
  `TestInitAfterOldBinary` replaces `TestInitAllRepairs`. Compatibility, Q3, the contracts'
  upgrade order and decisions are corrected.
- **R3-C3:** an administrator's `ConfirmDeletion` releases the charge (tested). followups says a
  node gone for good keeps its charges, which do not matter, and its ids taken, as #13's S11 says.
- **R3-C4:** the limit tests run on both stores, the etcd one enforcing 120 (`etcd.go:91`).
  Added `TestCreateAt116Nodes`, `TestInit121Nodes`, `TestConcurrentInit`,
  `TestInitAbortedAfter32` and `TestClaimKeyOverwritten`.
- **R3-C5:** `meta up` for the whole cluster runs `capacity-init` once, after every instance,
  output shown, failure fatal; not for a single VM as `chaos` calls it. The free move back is
  gone with R3-S1.
- **R3-S4:** the response marks each node still being initialized; the CLI exits 1 naming them.
  Risks says that `--all` pauses creates and moves until its writes land, and that an
  interrupted run leaves its nodes marked until a rerun.
- **R3-S5:** Risks states both effects and their bound: one compromised `io` identity,
  availability only, never an acknowledgement. No bound on the figures in this step.
- **R3-S7:** a nil map has no room; the domain tests use `domain.Unlimited`; `TestPlaceRoom`
  covers a nil map refusing.
- **R3-C6, R3-S nit 2:**
  - spec, design and contracts carry "Settled at approval, 2026-11-13" or "Accepted with the
    plan, 2026-11-13" where they called a choice open or Proposed;
  - adr-proposal.md's status says it is binding for this plan's implementation from the approval
    on, though numbered and moved into docs/adr/ only with it;
  - decisions.md notes under Approvals that these fixes came after the approval.
- **Nits:** `CompletedMove` and `Left` are gone. The initialization wording now says a failed
  write leaves the node marked. Lab step 3 says step 1's U check makes "exactly R" hold.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — b13ee9cbcf5b2de4decfdade8ccb971b78befec3

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72c4 (incremental design-code), r72s4 (incremental design-security). The plan was
approved before round 3; these fixes come after the approval.

- **R4-1 — Medium / R4-S2 — Low (one defect):** a release between `InitCapacity`'s mark and
  write made a blocked node's restore fail, and the earlier ledger was lost, so the node stayed
  marked for good while its blocker stood; the CLI's "run it again" could not help. Evidence:
  design.md "Initialization" step 4, release rule; capacity-contracts.md:83, 127–128.
- **R4-2 — Low / R4-S1 — Medium (one item):** #13's approved plan says 1 to 119 chunks per
  `ConfirmRelease` and sweep batch; #72 needs 118, stated #13's bound two ways, and did not say
  how a ledger-only conflict is retried or how the ledger read relates to #13's read bound.
  Evidence: `flaminio-13/docs/issues/13/design.md:72, 142, 628, 674`, `decisions.md:248`,
  `spec.md:105` there; design.md:18, 81–83.
- **R4-3 — Low:** "Parallelizable? Yes" overstated: the Go domain and service units need #13's
  `Chunk.Released` and `CheckMove`. Evidence: design.md "Parallelizable?".
- **R4-S3 — Low:** with #13's release deletion off, nothing releases a hand-deleted or
  replaced-disk released copy; spec S11, decisions and the followups text said otherwise.
  Evidence: `flaminio-13/.../spec.md:48–49, 77–79, 84–90`.
- **R4-S4 — Low:** #13's fence-only slots were counted in slots used but never charged,
  contradicting the invariant; tombstone entries for fence holders were undefined. Evidence:
  `flaminio-13/.../spec.md:31–33, 68–69`, design.md:57–58.
- **R4-S5 — Low:** no test of a stale, replayed or non-releasing `ConfirmRelease`, or of one
  between Init's mark and write. Evidence: design.md Tests, S4/S11 and S6 rows.
- **R4-C nit:** the confirmer of `ConfirmDeletion` and the lab step 5's sweep stated loosely.
- **R4-S nits:** "an agent or administrator confirming early" after #13's S12; "replicas on a
  replaced disk stay charged until moved off" read as freed by the move.

Changes/resolution:
- **R4-1, R4-S2:** a mark now keeps the value it replaces (`prev`). A release while marked
  subtracts from `prev`. A blocked node is restored from the current mark's `prev`, rereading on
  a conflict. A rerun keeps an existing mark's `prev`. So no restore or rerun loses a release.
  A node whose mark keeps no value stays marked, reported as blocked by its blocker. The CLI
  exits 2 with one line per node still marked: run it again, or clear its blocker first. `meta
  up` warns on exit 2 and fails on exit 1. Tests: `TestInitRacingConfirm` (restored less the
  release), `TestInitBlockedWithoutPrev`, the CLI's exit-2 lines.
- **R4-2, R4-S1:** spec S11 is now the one statement of the change #72 needs in #13's approved
  plan, as a prerequisite #72 lands only with or after:
  - `ConfirmRelease` takes 1 to 118 chunks, and the sweep confirms in batches of 118;
  - the ledger compare and write by #72's release rule;
  - a ledger-only conflict retried as a moved header;
  - the ledger read is part of the service's handling of the sweep's call;
  - the places in #13's text, and an engine test confirming 119 released chunks in two batches.

  The design's #13 fact keeps 119 "as approved" and defers to S11. #13's plan is not edited.
- **R4-3:** Parallelizable is now "partly", naming the Go domain and service units and the tests
  that follow #13's rules.
- **R4-S3:** S11, decisions and the followups text say that while release deletion is off, a
  released copy deleted by hand or lost with a disk stays charged until its volume is deleted:
  hand deletion frees no capacity.
- **R4-S4:** a fence holder is charged one slot and no bytes, in `VolumeCharges`, tombstones and
  initialization. `ConfirmRelease` keeps one slot when it adds the node to `Fenced`. Risks notes
  the conservative case of a fence holder that still holds chunks. Tested in
  `TestDeleteReleasesOnConfirm` and `TestConfirmReleaseReleasesCharge`.
- **R4-S5:** `TestConfirmReleaseReleasesCharge` covers stale, replayed and non-releasing calls
  (ledger unchanged), a ledger-only conflict, both stores with etcd enforcing 120.
  `TestInitRacingConfirm` runs with `ConfirmDeletion` and with `ConfirmRelease`.
- **Nits:** `ConfirmDeletion`'s callers are the node's data plane and an administrator (#13's
  S12). Lab step 5 names the tombstone sweeps and says step 4's move releases nothing. The
  security risk names the node's `io` identity and administrators. The replaced-disk sentence
  says a moved-off replica becomes released and stays charged.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 7aae38e9b5400cbe744aabf5e7ce7014c292a59b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b5 (incremental, design-code and design-security). The plan was approved before
round 3; these fixes come after the approval.

- **R5-B1 — Medium:** a release on a mark without `prev` wrote nothing, so an initialization
  that read its snapshot before that release wrote a charge the node had already released.
  Evidence: design.md:79–80, 117–119; `meta/kv/kv.go:18–22`.
- **R5-B2 — Medium:** the blocked restore compared "the revision just read", not this call's
  mark, contradicting the contracts, the ADR and decisions, and letting one call overwrite
  another's mark. Evidence: design.md:119–121, capacity-contracts.md:40,
  adr-proposal.md:31–32, decisions.md:99.
- **R5-B3 — Low:** S4, the design and a test had the node's data plane call `ConfirmDeletion`,
  which needs #13's S12, while S11 depends only on S1–S5 and `auth.go:77–78` allows the agent.
  Evidence: spec.md:67, design.md:82–83, 194–195.
- **R5-B4 — Low:** a blocked node restored from `prev` made the CLI exit 0 with no line, although
  Q3's procedure needs another `--all`. Evidence: capacity-contracts.md:130–133, spec Q3.
- **R5-B5 — Low:** no initialization test had a fence holder. Evidence: design.md:167, 170.
- **R5 nits:** #13's `spec.md:105` listed as changing though the bound "already counts" the read;
  the outcome after the restore's 32nd try; whether `had_ledger` and `previous_*` count a mark's
  `prev`; `TestInitBlockedWithoutPrev` not asserting the rerun.
- **Cross-plan (#70, round 5):** this plan's S3 named only creates and moves as charging.

Changes/resolution:
- **R5-B1, R5-B2:** the contracts now state one rule, which spec S6, the design and the ADR
  point to:
  - a mark keeps `prev`;
  - every write `InitCapacity` makes, restores included, compares this call's own mark, with no
    retry;
  - every release on a mark rewrites it, `prev` less the charge or the same mark, so its
    revision moves.

  A release during a call therefore leaves the node marked, with the release in `prev`, for an
  exact rerun. `TestReleaseRuleByLedgerState` asserts the revision moves for a mark without
  `prev`. `TestInitRacingConfirm` runs on both stores with both kinds of release, for a node with
  a ledger, one without and a blocked one. `TestConcurrentInit` is restated.
- **R5-B3:** `ConfirmDeletion` keeps today's callers: whichever identity `auth.go` allows for the
  node (its agent today, its data plane after #13's S12) or an administrator, in S4, the design,
  the test and Risks. S11 does not depend on #13's S12.
- **R5-B4:** a restored blocked node now exits 2 with the line `capacity-init: <node> blocked by
  <id>: kept its earlier ledger; run it again with --all once <id> clears`; the CLI test covers
  it.
- **R5-B5:** `TestInitCapacity`'s fixture has a fence holder with no other copy and a tombstone
  pending on one, one slot and no bytes each, on both stores.
- **Nits:**
  - #13's `spec.md:105` is dropped from S11's list ("#13's spec is unchanged"), and the bound is
    said not to change;
  - the restore no longer retries, so there is no 32nd try;
  - `had_ledger` and `previous_*` count a mark's `prev`;
  - `TestInitBlockedWithoutPrev` confirms the node, reruns and asserts exact.
- **Cross-plan:** S3 adds once: a volume's growth (#70) charges its new copies, at soft, in the
  transaction that exposes them; whichever of #70 and #72 lands second adds the charge and its
  test. The intent points to it.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 559eef3311a396ed286a4c28b1fe664c90cd2d89

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b6 (incremental, design-code and design-security). The plan was approved before
round 3; these fixes come after the approval. The reviewer walked every interleaving of
`InitCapacity`, `ConfirmDeletion` and `ConfirmRelease` and found no release lost or counted twice.

- **R6-B1 — Low:** a blocked node whose restore lost its compare to a release got the "wait for
  its confirmations or repair it" line, though a plain rerun restores it. Evidence:
  capacity-contracts.md:48–49, 93, 140–141; design.md:116–118.
- **R6-B2 — Low:** the exit code depends on `blocked_by`, but no text said whether rows the call
  did not target carry it; a plain run could exit 2 for untouched nodes. Evidence:
  capacity-contracts.md:92, 137; spec.md:110–117; design.md:143–145, 182.
- **R6-B3 — Low:** spec S3's growth rule had no design side: no bullet, file or test; the intent
  still listed growth as open; the contracts' writers and transactions omitted it. Evidence:
  spec.md:60–61; design.md:53–84, 124–149, 163; intent.md:50; `#70 design.md:41–42, 74–77, 208`.
- **R6 nits:** `meta up` and the lab docs described exit 2 as "a node left marked" only;
  `TestInit121Nodes` lost "both stores"; the race test's assertion did not fit the node with no
  ledger, which has no `prev`.

Changes/resolution:
- **R6-B1:** the contracts' CLI lines now use `had_ledger`. A marked, blocked node that kept
  `prev`, whose restore raced a release, gets `capacity-init: <node> blocked by <id>: raced a
  release: run it again`. A marked, blocked node without one keeps the wait-or-repair line. The
  `marked` comment defers to the lines, and the S10 CLI test has the raced blocked node.
- **R6-B2:** `blocked_by` is set only for the call's targets, in the contracts (field comment and
  exit rule) and spec S6. A node the call did not touch never makes it exit 2, and the CLI test
  has such a row exiting 0.
- **R6-B3:** a design bullet says where growth charges the ledger: once #70 has landed, each
  step of its `ExpandVolume` (`meta/service/expand.go`) charges by #70's "Charges", with `Room` at
  soft and the room map, ledgers compared and written in the step's transaction, and deletion
  releasing by the release rule. Whichever of #70 and #72 lands second adds it and removes step
  1's refusal. Further changes:
  - Files touched lists `expand.go` and `expand_test.go` (#70's);
  - the S3 test row adds the shared `TestExpandChargesLedger` and `TestExpandRefusals` without
    step 1's refusal;
  - the contracts list growth among the ledger's writers and add its transaction row;
  - the intent moves growth from the open list into what this plan wants;
  - spec Tests covers growth once #70 has landed.
- **Nits:**
  - `meta up` warns on exit 2 for "a target still marked or blocked", and the docs line says
    the same;
  - `TestInit121Nodes` runs on both stores, etcd enforcing 120;
  - the race test asserts, for the node with no ledger, that the mark's revision moved without
    `prev` and that the rerun is exact (spec Tests likewise).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 4eadb5fa0867cb472102c3a8982fa9399af3fdda

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b7 (incremental design-code and design-security).

r72b7 confirmed every round-6 finding fixed (R6-B2 in the text; its service-level test is R7-1) and reported:

- **R7-1 — Low:** Nothing at the service level tests that a node the call did not target carries no `blocked_by`; only the CLI's test, against a fake server, does. Evidence: spec Tests; design.md S6 row (`TestInitCapacity`).
- **R7-N1 — Nit:** The growth bullet gives growth's 120-operation bound only by reference to #70. Evidence: design.md:85–89.

Changes/resolution (made by the orchestrator; a change after the approval): `TestInitCapacity` gains a non-`all` case with an untouched readable node pending on a legacy tombstone (empty `blocked_by`, `marked` false, ledger unchanged); the growth bullet says the step's chunk count shrinks to the longest prefix whose compares and writes fit 120 operations.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — eea3f2c0b1fbd731fe2e68224bb1e71da0e87f2b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b8 (incremental design-code and design-security).

r72b8 confirmed both round-7 findings fixed and reported nothing of Low or worse.

- **R8-N1 — Nit:** The growth bullet's "120 operations" should name both of #70's limits (120 compares and 120 writes) and that one chunk always fits. Evidence: design.md:85–89; #70 design "Charges".

Changes/resolution (one clause of design.md, made by the orchestrator; a change after the approval): "fits 120 compares and 120 writes, at least one chunk".

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 591097911a24d7424ba9a9b85bb9202f0f869657

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b9 (incremental design-code and design-security).

r72b9 confirmed the round-8 Nit fixed and reported nothing of Low or worse.

- **R9-N1 — Nit:** The clause says "writes" where #70's "Charges" says "operations" (the same bound). Evidence: design.md growth bullet; #70 design.md:86–89.

Changes/resolution (one clause of design.md, made by the orchestrator; a change after the approval): the clause now reads "120 operations and 120 compares (#70's terms), at least one chunk", word for word with #70.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — 5966eff693fd1216063ff5e8a58354f7385238de

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b10 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.

### Plan review round 11 — 89fb28e6876be0e0c176f66e0b6d8a33dec8d6a4

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b11 (incremental design-code and design-security).

Reviewed the orchestrator's alignment of #72 with #70's ordering prerequisite (`5966eff..89fb28e`; #72 had been clear at round 10). The changed sentences agree with #70's current text; nothing still describes a step-1 refusal. Two stale mentions remained:

- **R11-1 — Medium:** design.md:145–146 (Files) still said the growth charge comes "by whichever of #70 and #72 lands second". Evidence: design.md:145–146 against :89–91, spec S3 and #70's "Charges".
- **R11-2 — Low:** decisions.md:138–140 ("Growth (cross-plan with #70)") still said "whichever of #70 and #72 lands second adds it and its test", with no changed note. Evidence: decisions.md:138–140.

Changes/resolution (made by the orchestrator; a change after the approval): design.md's Files line says #70 adds the charge, landing with or after #72; decisions.md's two growth entries say the same, the older one noting what it replaced. No "whichever of #70 and #72" remains in #72's plan.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 12 — 186074fb9e04314cec4a6f735cc08ace734b3158

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r72b12 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
