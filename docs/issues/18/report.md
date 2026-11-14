# Issue #18 — report

## Review rounds

### Plan review round 1 — 1287453

Scope: correctness against code and security of the plan (approved 2026-11-13; these fixes are
made after approval); no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High:** #13's hand-off is missing: `choose` does not exclude the chunk's released
  nodes, and a chunk only releasing nodes could take would count as blocked and end the drain,
  where #13 says it waits. Evidence: flaminio-13 decisions.md:235-236, design.md:792-799;
  #18 design.md:76-83, :92-93, :185-186, :193.
- **R1-C2 — High:** the drain takes any `FailedPrecondition` from `MoveReplica` as a collision,
  but #72 and #78 answer the same code for other causes; S15 and the intent keep accepting
  draining and dead destinations, which #78 and #72 refuse. Evidence: #18 design.md:84-85,
  :124-126; 72/design.md:66-68, :105-108; 78/spec.md:49-53, 78/adr-proposal.md:42-43,
  78/design.md:95-97; #18 spec.md:99-100, intent.md:65-67.
- **R1-C3 — Medium:** `Volume` fields 9 and 10 collide with #79's. Evidence: #18
  design.md:115-117, :173; 79/initialization-contract.md:45-46; flaminio-17 design.md:530.
- **R1-C4 — Medium:** #18's drain does not compose with #17's `Drainer`, `VolumeState` and the
  removal of `Unattached`. Evidence: flaminio-17 design.md:424-433, :469-473; #18
  design.md:86-89.
- **R1-C5 — Low:** an unknown stored word stops the whole drain step, against S11; no test.
  Evidence: #18 design.md:77-79, spec.md:81-83.
- **R1-C6 — Low:** S1's "not the level a `bestEffort` placement fell back to" has no test.
  Evidence: spec.md:34-36; design.md:136-144.
- **R1-C7 — Nit:** "volumes waiting for a target … keep it going" invites an `Unattached`
  condition in `Run`. Evidence: design.md:93-94; `TestDrain`'s `v2`.
- **R1-S1 — High:** under `strict`, an unlabelled node counts as a zone of its own, so a drain
  sends replicas to a new node before its labels arrive, and the spread breaks for good; a
  staying replica without a readable record fails open the same way. Evidence:
  `domainKey` (meta/domain/domain.go:322-331); engine.rs:585-593; control/csi/topology.go:21-51;
  drain.go:76, :157-162; service.go:197-200.
- **R1-S2 — Medium:** a metadata-service rollback strips the level and policy from every volume
  attached during it, silently and for good. Evidence: ARCHITECTURE.md:916-918, :940;
  service.go:723-725; decisions.md Q5.
- **R1-S3 — Medium:** as R1-C2, from the security side: a capacity or drain-mark refusal is
  reported as a domain refusal and retried until the time-out; `RESOURCE_EXHAUSTED` fails the
  step. Evidence: 72/capacity-contracts.md:115, 72/design.md:105-108; 78/design.md:53-54.
- **R1-S4 — Medium:** as R1-C1: without #13's exclusion the drain runs to its time-out; with a
  naive one a waiting chunk ends the drain. Evidence: flaminio-13 decisions.md:235-236,
  design.md:792-795.
- **R1-S5 — Medium:** as R1-C3. Evidence: 79/initialization-contract.md:45-46; flaminio-13
  design.md:928-929.
- **R1-S6 — Low:** as R1-C5. Evidence: design.md:40-42, :77-79.
- **R1-S7 — Low:** the Risks entry understates who may set a node's labels, now that labels gate
  every strict move. Evidence: auth.go:71-72; scripts/lab.sh:1004; engine.rs:585-593.
- **Nit (security) — the compare count:** "at most 8 keys (5 replicas)" is 7 at most. Evidence:
  design.md facts.
- **Nit (security) — quoting:** the refusal's label value and ids are not said to be quoted; a
  label is a free string a node sets. Evidence: service.go:197-200; design.md "The refusal".

Changes/resolution:
- **R1-C1, R1-S4 (one root cause):** the drain section gains #13's order, due in whichever of
  #13 and #18 lands second: a chunk at #13's bound waits; eligible nodes exclude the chunk's
  replicas and `GetReleased()`; the domain filter runs on what is left; with no node left, a
  chunk a releasing candidate would pass waits, with `only nodes releasing it are left (N)` in
  `Progress.Held`, not blocked; otherwise blocked. Spec Terms (Eligible, Blocked chunk) and
  Tests; two drain tests (a releasing node least loaded; the only allowed node releasing). Lab:
  `DELETE_RELEASED=1` once #13 lands; deletions by #13's sweeps, not `vol-confirm`.
- **R1-C2, R1-S3 (one root cause):** the refusal keeps `FailedPrecondition` and its message
  starts with the fixed text `failure domain collision: `, which the drain tells it by (the
  simplest safe option: #72 and #78 identify their refusals by stated texts; #13's `reason`
  rides on `applied=false`, which S7 excludes). A collision blocks that chunk only; any other
  `FailedPrecondition` and `ResourceExhausted` follow #72's drop-and-pick-again within #18's
  rules, and with none left the chunk is blocked, not a failed step. `Run` checks `BlockedError`
  (with `Remaining > 0`) before #78's `CheckDrain`. S15 and the intent: a draining destination
  is refused once #78 lands, a dead one or one without a report once #72 lands; #18 adds
  neither. Contracts name the other causes; the combined compare count is stated (9 at most, 2
  operations, under 120). Test: an interceptor's `FailedPrecondition` without the text, and a
  `ResourceExhausted`, are not counted refused.
- **R1-C3, R1-S5 (one root cause):** `failure_domain = 18`, `failure_domain_policy = 19`
  (Contracts, the Targets line); `pb.rs` tags 18 and 19 should it ever name them.
- **R1-C4:** a cross-plan paragraph, whichever of #17 and #18 lands second: `choose` in #17's
  seam; a volume with no move pending and a blocked chunk is `waiting`, why `no destination
  (LEVEL/POLICY)`; `BlockedError` after `Drainer.Step`, before `CheckDrain`; `String()` puts its
  part right after the counts. Spec S12 and a test.
- **R1-C5, R1-S6 (one root cause):** an unknown stored word blocks that volume's chunks, shown
  with `%q`; the rest move and `BlockedError` names it. Spec S11, S12; a drain test.
- **R1-C6:** a `placement_test.go` case, a `zone`/`bestEffort` volume placed at `rack` reports
  `zone`, `bestEffort` after create, get and attach.
- **R1-C7:** `Run`'s text says a volume waiting for a target keeps it going only through a
  pending move; S13 is worded likewise (a clarification).
- **R1-S1:** under `strict` at `rack` or `zone`, for a chunk with a staying replica, a
  destination without the label or with an empty one is refused, and so is any move beside a
  staying replica without the label, a record or a parsable record (`domain.Strict`, used by the
  service and the drain). One-replica chunks and `bestEffort` are unchanged. Spec Terms
  (Unknown at a level), the invariant, S5, S7, S8, S10; tests for each case; the lab gives fl-c
  the zone `lab-c` for step 3.
- **R1-S2:** Q5 changed after approval: no rollback of the metadata service below this version
  once a `rack` or `zone` volume exists, stated in `docs/ARCHITECTURE.md` §18 and
  `docs/lab.md`.
- **R1-S7:** Risks and decisions.md state every label writer (administrators, the CSI
  controller's certificate included; the node's `agent`; its `io`), the same trust creation
  already has; refusing label changes from `io` is left to its own issue (not filed).
- **Nits:** the facts say 7 compares at most; the refusal formats every id and label value with
  `%q`, with a test of an escaped label.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 339601b

Scope: correctness against code and security of the plan (approved 2026-11-13; these fixes are
made after approval), incremental over round 1's fixes; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-B1 — Medium:** creation still gives an unlabelled node a `strict` replica, so the docs
  overstate R1-S1's fix; and a chunk with a replica on an unknown node has unmovable other
  replicas, which nothing states (drains, #45's rebuild, a node clearing its own label).
  Evidence: domain.go:322-331, :343-351, :405-417; control/csi/topology.go:21-27;
  service.go:197-200; design.md:300-301; decisions.md:91-93; 45/spec.md:68-69, :86.
- **R2-C1 — Medium:** the drain's rule for an unknown staying replica has no test, and the
  invariant test cannot catch a drain that counts the service's refusal as refused (it would
  run to its time-out). Evidence: design.md:92-94, :99-100, :237-239.
- **R2-C2 — Low:** no test proves the drain recognises the text the service actually sends;
  every drain test fakes it. Evidence: design.md S5–S7 tests, :248-251.
- **R2-C3 — Low:** a chunk blocked by #72's rule is reported as a domain block, and the last
  refusal has nowhere to go. Evidence: design.md:109-116, :135-142, :150-151;
  72/spec.md:123-125.
- **R2-C4 — Low:** with #13, the lab's step 4 hand move can get `applied=false` and exit 0, by
  timing. Evidence: 13/design.md:94-98; main.go:426-431; design.md:283-285.
- **R2-S1 — Low:** Q5's no-rollback rule is only documented, yet decisions.md says it prevents the
  drop; the operator cannot easily check when it applies; ARCHITECTURE §19 still promises
  rollback before finalize. Evidence: decisions.md:100-104; service.go:723-725;
  drain.go:51-56; ARCHITECTURE.md:940.
- **R2-N1 — Nit:** the invariant lacks "for a chunk with a staying replica". Evidence:
  spec.md:29-31, :55-60.
- **R2-N2 — Nit:** the refusal message could repeat the prefix or put it last, depending on how
  `Strict` wraps. Evidence: design.md:67-78.

Changes/resolution:
- **R2-B1:** creation fails closed like moves (the coordinator's decision): under `strict` at
  `rack` or `zone`, `Place` leaves out nodes without the label for volumes of two replicas or
  more, and a short topology is refused as today, counting the nodes left out (spec S5, S15;
  intent What we want 2 and Out of scope; design Creation, Contracts; tests
  `TestPlaceStrictLeavesOutUnlabelled` and a service test). One-replica volumes are unchanged,
  as for moves, so the docs' claim is stated for chunks of two replicas or more. No chunk of a
  recorded `rack`/`zone` volume predates the rule; one whose node later loses its label or record
  blocks moves, drains and #45's rebuild of its other replicas, named in Risks with the remedy
  (label the node, or move its replica first). `BlockedVolume.Why` names the reason (`replica "x"
  has no zone label`), in `String()` and `BlockedError`; `Strict` names an unknown staying
  replica first. Hand-off to #45's S15.
- **R2-C1:** a drain test: a node of the chunk set to the empty zone, and again with its record
  deleted; no request, blocked and not refused, `Run` returns `BlockedError`.
- **R2-C2:** the drain matches the prefix `domain.ErrCollision.Error() + ": "`; a drain test
  relabels the destination through the real service just before `MoveReplica`.
- **R2-C3:** the last refusal is the blocked volume's `Why`, in `String()` and `BlockedError`;
  the existing test asserts it.
- **R2-C4:** once #13 lands, the lab waits at most 60 s for `metactl releases fl-c` to be empty
  before step 4, else exits 1 naming fl-c; step 4's expected stderr carries the reason.
- **R2-S1:** decisions.md calls the rule an operating rule nothing enforces; ARCHITECTURE §18 and
  `docs/lab.md` give the check (`volumes`, `get` each, `jq` on `failureDomain`), the silent and
  permanent loss of a rollback anyway, and recreation as the only remedy; §19 treats creating
  the first `rack`/`zone` volume as this release's finalize; Risks names the loss, not accepted.
  No mechanism added: none exists cheaply.
- **R2-N1:** the invariant reads "a move of a chunk with a staying replica".
- **R2-N2:** `Strict` returns a bare reason; only the service writes
  `fmt.Errorf("%w: volume %q chunk %d: %s", domain.ErrCollision, …)`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — c6c7373

Scope: correctness against code and security of the plan (approved 2026-11-13; these fixes are
made after approval), incremental over round 2's fixes; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R3-C1 — Low:** the cases for a blocked chunk's `Why` overlap; in the order written they
  contradict the design's own tests, and a chunk whose nodes earlier refusals took out reads
  `no eligible node`. Evidence: design.md:127-132, :141, :302, :316; 72/spec.md:123-125.
- **R3-C2 — Low:** after R2-B1 no replica sits on an unknown node, so the property test's
  "beside" clause cannot fail; the generator does not fit the creation rule; the S5–S7 case of
  a staying replica without the label cannot be built through `RegisterNode`. Evidence:
  design.md:282-288, :97-101, :270; service.go:197-199.
- **R3-C3 — Low:** the creation suffix says "zone label" at `rack` too, and lives inside
  `ErrPlacement`, so #72's room refusal never names the unlabelled nodes. Evidence:
  design.md:99-100, :233, :72-73, :102-103, :253-256; 72/capacity-contracts.md:113-114;
  72/spec.md:99-100.
- **R3-B1 — Low:** the creation change is missing from the `CreateVolume` proto comments,
  ARCHITECTURE §8.2, follow-ups §4 step 2, and Risks' Compatibility (strict classes that relied
  on unlabelled nodes start refusing). Evidence: design.md:210-219, :421-422;
  meta.proto:139-140; ARCHITECTURE.md:442; controller.go:248-249.
- **R3-S1 — Low:** the lost-label block is not listed as named, not accepted, as the rollback
  loss is. Evidence: design.md:414-420, :449-451; decisions.md:139.
- **R3-N1 — Nit:** the `BlockedError` example quotes #72's node id, which #72's contract does
  not. Evidence: design.md:188; 72/capacity-contracts.md:115.

Changes/resolution:
- **R3-C1:** one stated precedence, the first that applies: the chunk's own refusal (the last);
  unknown words; no candidate (`no eligible node`); every candidate taken out by this step's
  refusals (the step's last such refusal); `Strict`'s reason for the least loaded candidate left.
  The tests name their case; a case-4 test is added. Spec S12 matches.
- **R3-C2:** the property test creates volumes only where enough labelled domains exist, checks
  that none then sits on an unknown node, clears and restores labels between moves, checks
  distinct domains among labelled replicas, and must fail with `skipCollisionCheck` and with a
  new test-only `skipStayingCheck`. The S5–S7 cases write the store for an absent label or a
  deleted or unparsable record.
- **R3-C3:** `domain.LeftOut` gives `; N nodes without a zone label left out` (`rack label` at
  `rack`), asserted at both levels; `createVolume` appends it last to any placement refusal, so
  with #72 it follows #72's suffixes on either create error. Hand-off to #72.
- **R3-B1:** the `failure_domain_policy` and `prefer_node` comments, ARCHITECTURE §8.2's row,
  follow-ups §4 step 2 and a `docs/lab.md` upgrade note are in the Docs list (spec S16); Risks'
  Compatibility names the refusals after the upgrade.
- **R3-S1:** Risks' "Named, not accepted" lists the lost-label block; decisions.md's R2-B1 entry
  says so.
- **R3-N1:** the example reads `node d has no current capacity report`: a refusal's message is
  used verbatim; #18's own reasons keep `%q`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 4048b5d

Scope: correctness against code and security of the plan (approved 2026-11-13; these fixes are
made after approval), incremental over round 3's fixes; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R4-1 — Low:** reason 4 repeats the step's last refusal, whose message names another chunk
  (`volume "v1" chunk 7`), so a later volume's `Why` names v1; reason 1 shows the refusal even
  when the chunk's remaining nodes were never taken out and `Strict` refuses them. Evidence:
  design.md:139-150, ~135-141.
- **R4-2 — Low:** the property test clears "a random node's label" without saying which, so at
  `rack` a cleared zone (or at `zone` a cleared rack) changes nothing; the injected-bug check has
  no stated budget or guarantee that the unknown-node case is reached. Evidence: design.md
  property test.
- **R4-3 — Low:** what `createVolume` tests before appending `LeftOut`, and that it re-wraps with
  `%w`, is unstated; `LeftOut` repeats `Place`'s predicate and may drift; the suffix also lands
  on refusals unrelated to labels (more replicas than live nodes). Evidence: design.md:105, the
  #72 bullet; service.go:143.
- **R4-N1 — Nit:** "clearing and restoring never put two labelled replicas in one domain" is
  stated without its reason (an unknown staying replica blocks the other replicas' moves).
  Evidence: design.md and decisions.md, the property test.

Changes/resolution:
- **R4-1:** reason 1 applies only when `Strict` refuses no node still left to the chunk;
  otherwise reason 5 gives `Strict`'s reason. Reason 4 reads `every candidate refused earlier in
  this step: <node>: <message>; …`, the chunk's own candidates and why each went. Tests: a
  refused chunk whose other candidate `Strict` refuses (reason 5); the reason-4 test asserts
  the exact text for a later chunk of the same and of a later volume. Spec S12 matches.
- **R4-2:** the test clears only the label at a volume's level, on a node holding one of its
  replicas; budget: fixed seeds 1–64, each a forced step and 40 random steps; the forced step
  clears a chunk's first replica's node at its level and moves its second replica by hand to a
  labelled node in an unused domain (guaranteed by the generator), which both injected switches
  accept on every seed.
- **R4-3:** one unexported predicate, `leftOut`, serves `Place`'s filter and `LeftOut`, with a
  table test; `LeftOut` is non-empty only for the refusal too few labelled domains cause;
  `createVolume` appends it only on `errors.Is(err, domain.ErrPlacement)`, re-wrapping with
  `%w`. A refusal for too few live nodes and #72's room refusal get no suffix; the #72 hand-off
  is corrected.
- **R4-N1:** the reason is written beside the claim (while a node is cleared, S5 refuses every
  move of its chunks' other replicas and every move to it), with a note that it rests on S5.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — b71f1c6

Scope: correctness against code and security of the plan (approved 2026-11-13; these fixes are
made after approval), incremental over round 4's fixes; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R5-C1 — Medium:** "caught on every seed" is not guaranteed: volumes are created only when the
  topology allows, a filter, so a seed can admit no volume; the forced step then does nothing and
  the injected bugs pass on that seed, against spec Tests and round 4's entry. Evidence: design
  Tests (the invariant); spec Tests.
- **R5-C2 — Low:** `TestLeftOutPredicate`'s "`Place` leaves out exactly the nodes `LeftOut`
  counts" cannot be asserted: `LeftOut` is empty whenever `Place` succeeds, and `Place` returns
  only the placement. Evidence: design Tests.

Changes/resolution:
- **R5-C1:** the generator gives three nodes distinct labelled zones and racks first, so each
  seed's first volume (`strict`, 2 replicas, at the seed's level) has replicas + 1 labelled
  domains and is always created; the other nodes are random. The test asserts on every seed that
  the forced step ran on that volume's chunk 0 and was refused, and, under each injected switch,
  accepted with the "beside" clause failing.
- **R5-C2:** three assertions per row: `leftOut` per node against the table; `LeftOut` non-empty
  exactly when `Place` returns `ErrPlacement` with some node left out, at least `replicas` nodes
  in all and fewer than `replicas` domains among those kept; its count equal to the nodes
  `leftOut` names.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 1e5ec2d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- R5-C1 and R5-C2 fixed.
- **R6-B1 — Low:** "the third of the first three nodes is always one" is wrong: the chunk may hold
  it. Evidence: design.md:350.
- **R6-B2 — Low:** the forced step asserted only "refused"; another refusal or a no-op passes.
  Evidence: design.md:352.

Changes/resolution: R6-B1: at least one of the first three is outside the chunk. R6-B2: the step
asserts `FailedPrecondition` with the `ErrCollision` prefix naming the cleared replica's node.
Recorded in decisions.md, round 6, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — d333ca864431b68125007881476fdfdc8149217a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
