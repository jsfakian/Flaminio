# Issue #56 — report

## Review rounds

### Plan review round 1 — 513b97374f66c70e3627c50051be50abf84bccd6

Scope: full intent/spec/design/decisions plan against source and current dependency plans.
Reviewers: /root/remaining_plan_reviews/review56_r1_code (design-code and spec correctness), /root/remaining_plan_reviews/review56_r1_security (design-security and spec security).

- **R1-C1 — High:** VM power-off was accepted as node recovery authority contrary to current held physical-fence profile. Evidence: spec.md S11/Open questions; design.md Lab history and faults; #82 design36–51.
- **R1-C2 — Medium:** Exploratory VM path did not distinguish enforced physical-only #83 startup refusal. Evidence: design.md Lab history and faults; #83 design21–42.
- **R1-C3 — Medium:** Manager causal histories had no versioned executable contract compatible with overlap or powered-off endpoints. Evidence: design.md Lab history and faults/Contracts; #49 FLRUN001.
- **R1-C4 — Medium:** Simulator skipped probes and post-heal assertions could not demonstrate fault-phase contract; metadata Attach bypassed modeled authority. Evidence: design.md Simulator and mutation coverage; sim/mod.rs483–530,1070–1104,1332–1367.
- **R1-C5 — Medium:** Metadata revision observations assumed an interface #77 does not expose. Evidence: spec.md S10; design.md observation section; #77 no protobuf shape change.
- **R1-C6 — Low:** Intent promised unconditional removal of faults despite management loss. Evidence: intent.md What we want4; spec.md S9.
- **R1-S1 — High:** Sampled power-off and subsequent VM restart did not preserve #82 held restart interlock. Evidence: design.md95–105,141–143; #82 design36–51.
- **R1-S2 — Medium:** Unqualified VM exploration supplied no admitted #83 execution path. Evidence: design.md67–70; #83 design19–42.
- **R1-S3 — Medium:** Contextual envelopes could not establish #49 checker boundaries or safe optional-write deadlines. Evidence: design.md72–86,119–122; #49 FLRUN001.
- **R1-S4 — Medium:** Privileged independent healer lacked strict bounded recovery-record validation. Evidence: design.md93–100; #49 ledger validation.

No Nits. The fencing, clock and history findings overlap between review scopes; each reported finding remains recorded. Full reports: audit checkout `docs/issues/review-inputs/56-r1-code.md` and `56-r1-security.md`.

Dependency snapshots: #44 `670c275595bc0ada1c0511313bf8ed5533a22797`, #49 `de46aeff4d5e2874bd9501a899a088ef3e42812a`, #55 `5b140f05a4c9d40ecf8332114283b2d1a3860770`, #77 `bb8df5b057e1e26f6e73e1319c9afc49a3c72994`, #81 `15b0c35d137a324a257cb845e2f9b2531cd27745`, #82 `18fb58576f2d5a8cbf31463619c652fbf475a5bc`, #83 `67862ad667ac3121b453c9227cef1e8ec5248138`.

Resolution: all findings addressed by the planner/designer. Current #82/#83 profiles now block unsupported VM recovery/admission; isolated legacy exploration conditionally holds the unsafe controller. FLEDGE01 defines coordinator/helper envelopes, optional-write deadlines and bounded checker slicing without changing #49 formats. The strict typed healer preserves foreign state; model-only probes assert actual fault phases; #77 observations use epoch/isr_ver/ISR. Intent cleanup no longer promises recovery after management loss. Historical 2026-11-13 approvals remain; these revisions are explicitly unapproved. No runtime, lab or dependency edits.

Validation: issue-document relative links and git diff --check pass. All simulator, helper and script tests remain planned implementation checks.

Status: needs_incremental_review.

### Plan review round 2 — b2b76b7e28215afc331cd6c5a07ee0350027a010

Scope: incremental `513b973..b2b76b7`, excluding report, and affected dependency paths.
Reviewers: /root/remaining_plan_reviews/review56_r2_code (incremental design-code), /root/remaining_plan_reviews/review56_r2_security (incremental design-security).

- **R2-C1 — Medium:** Checkpoint could carry an earlier read value after a newer acknowledged write. Evidence: lab-contracts.md:131.
- **R2-C2 — Low:** JSON Patch has no annotation-absence test operation. Evidence: lab-contracts.md:175; RFC6902 section4.6.
- **R2-S1 — Medium:** Independent healer lacked persisted remote helper invocation ownership. Evidence: lab-contracts.md:43–51,153–185.

Both reviewers confirm the earlier fencing/clock/model/view/cleanup roots resolved. The concrete history contract has one checkpoint refinement. No Nits. Full inputs: audit checkout `docs/issues/review-inputs/56-r2-code.md` and `56-r2-security.md`. Dependency snapshots remain #49 de46aef, #77 bb8df5b, #82 18fb585 and #83 67862ad.

Resolution: the planner/designer requires a newly issued gated/drained checkpoint read with no intervening block invocation, binds observed annotation absence by resourceVersion with absent-map handling, and adds a persisted helper action/private startup claim plus conditional invocation-safe termination. Specific ordering, crash and foreign-state fixtures cover all three corrections. No runtime/lab/dependency changes; historical approvals remain separate and revisions unapproved.

Validation: issue-document relative links and git diff --check pass; all regression fixtures remain future implementation tests.

Status: needs_incremental_review.

### Plan review round 3 — e22ebd7e1181dfafab41a48760bd0518657657be

Scope: incremental `b2b76b7..e22ebd7`, excluding report, and affected paths.
Reviewers: /root/remaining_plan_reviews/review56_r3_code (incremental design-code), /root/remaining_plan_reviews/review56_r3_security (incremental design-security).

Both confirm R2-C1, R2-C2 and R2-S1 fixed. Security reports zero comments including Nits.

- **R3-C1 — Medium:** conditional hold conflict retry could reuse stale restoration replicas/generation/spec after an ownerless update. Evidence: lab-contracts.md:184.

Resolution: any conflict now aborts before injection without retry; foreign scale/spec updates are preserved and stale snapshots cannot be rebound at a newer resourceVersion. A specific ownerless replica/spec conflict fixture is required. Historical approvals preserved; revision unapproved.

Validation: document whitespace and relative links; future tests remain planned, no runtime/lab action. Full inputs retained as audit `review-inputs/56-r3-code.md` and `56-r3-security.md`.

Status: needs_incremental_review.

### Plan review round 4 — b43ee508e00949524907309e931b5d954f442918

Scope: incremental `e22ebd7..b43ee50`, excluding report, and affected hold paths.
Reviewers: /root/remaining_plan_reviews/review56_r4_code (incremental design-code), /root/remaining_plan_reviews/review56_r4_security (incremental design-security).

Both confirm R3-C1 fixed and report **zero comments including Nits**. Every conditional hold conflict aborts before retry/fault injection; foreign updates survive and no stale restoration snapshot is reused. Matching ownerless scale/spec fixtures are specified. Full inputs: audit checkout `review-inputs/56-r4-code.md` and `56-r4-security.md`.

Status: clear. Technical snapshot `b43ee508e00949524907309e931b5d954f442918`. All earlier findings were resolved; no runtime, simulator or script tests were run. Whitespace and relative links pass. Historical 2026-11-13 approvals remain and subsequent revisions remain explicitly unapproved. No dependency plan changed or new issue filed. #56 remains open for the later topology/install/visibility increments and actual evidence; current VM admission/recovery and unresolved mutation coverage limitations stay explicit.
