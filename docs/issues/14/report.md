# Issue #14 — report

## Review rounds

### Plan review round 1 — 24877eb

Scope: correctness against code and security of the plan (approved 2026-11-13; step 2 shipped
in pull request #31, #30 merged after it); no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — Medium:** the plan's account of a wrong confirmation, and the shipped usage text,
  predate #30: a re-created volume no longer adopts or is deposed by leftovers. Evidence:
  intent.md Why 5-6, design.md facts (`u64::MAX`), main.go:25-30 and 214-218, against
  service.go:458-462, replica/mod.rs:193-211 and 813-822 (fb99440).
- **R1-C2 — Low:** #14's plan contradicts #13's: the sweep goes inside `flaminio-io node`, and
  #13 removes `vol-confirm`, `confirm.sh` and `confirm`; no unit can join the no-sweep check.
  Evidence: design.md:139, intent.md:66-67, decisions.md:22; #13 intent.md:4-5, spec.md S12-S13.
- **R1-C3 — Low:** S7.8 claims the confirmation finds the volume still pending; only the read
  before it does. Evidence: main.go:478-494, service.go:631-633 and 652-669; docs/lab.md:243.
- **R1-C4 — Low:** "RF 1 on fl-b never does [stop]" is false when `$VOL` has a replica on fl-c.
  Evidence: lab.sh:2155-2158 (`write_start`); followups.md §14 lab result; docs/lab.md:226-227.
- **R1-C5 — Low:** design statements differ from the shipped code: `confirm.sh`'s path
  overrides, the k3s check, `confirm`'s re-checks, `data_vms`, the wait's retries, `cmd_vol
  detach`. Evidence: confirm.sh:21-26, 41-50, 105; lab.sh:1819, 1787-1789, 721-725.
- **R1-C6 — Low:** the intent cites "spec, S5" for the log line (S4) and "spec S13" five times
  (the spec ends at S10). Evidence: intent.md:70, 74, 78, 83, 85, 89.
- **Nit (code):** S4 states no exception to "logs, once"; a lost answer to the last
  confirmation's write leaves no line either. Evidence: spec.md:51-53, service.go:631-633.
- **R1-S1 — Medium:** the hazard model is pre-#30, and the remaining hazard is named nowhere:
  every delete of the name ignores the fence, so one that runs after the id is freed wipes the
  re-created volume's replicas. Evidence: intent.md Why 6, spec.md invariant and S2,
  decisions.md:30-31, design.md:15-17, main.go usage and TestUsage; replica/mod.rs:813-821.
- **R1-S2 — Low:** the harness's deletes ignore fences and can run after the sweeps free the id;
  `delete_volume`'s re-check sees only a finished creation; the invariant overstates. Evidence:
  lab.sh:1733-1766, service.go:286-288, spec.md:18-21, decisions.md:70.
- **R1-S3 — Low:** the design's no-sweep check is weaker than the code's (`systemctl show`, both
  inactive and disabled or masked) and omits `confirm`'s sweep re-check. Evidence: design.md:136-
  139 and 150-152, confirm.sh:41-50 and 105, node-scripts-test.sh:231-237.
- **Nits (security):** the intent's "spec S13" and "spec, S5"; design.md:18 (`node.sh delete`
  now prints `not running`, exit 3) and design.md:125-127 (overrides only under
  `FLAMINIO_SCRIPT_TEST=1`). Evidence: node.sh:94-128, confirm.sh:21-26.

Changes/resolution:
- R1-C1, R1-S1: intent Why 5-6, the spec's invariant and S2's usage text restated for the code
  after #30 (the fence-blind wipe first; leftovers until the first session; old tokens; an old
  metadata service's volume deposed). Design facts corrected (`Store::delete_volume(uuid,
  None)`, #30's first epoch). The decisions keep their choices with #30's reason; the
  incarnation follow-up names the delete bounded below the tombstone's revision. The shipped
  main.go doc comment and usage, and TestUsage, are listed in the design's new "The next step".
- R1-S2: the invariant restated (one deleter and confirmer per node, plus the harness while no
  other lab command runs); S5 and the design's `delete_volume` step 4 name the residual; listed
  in decisions under "Risks named after approval", not accepted.
- R1-C2: the node daemon, `flaminio-io node` (intent, decisions); design step 3 says
  `vol-confirm`, `confirm.sh` and `confirm` go with #13's sweep (#13, S13).
- R1-C3: S7.8 and design step 7 say "the read just before the call"; the read-to-call window
  listed under "Risks named after approval"; docs/lab.md:243 listed in the next step.
- R1-C4: S6 corrected; docs/lab.md:226-227 listed in the next step.
- R1-C5, R1-S3, the security nits: the design's `confirm.sh`, no-sweep check, step 7, step 1,
  wait, callers and facts match the code; the test list matches.
- R1-C6: the intent cites S4, and decisions and follow-ups §14.
- Nit (code): S4 and the design state the exception; the accepted risk too.
- All recorded in decisions.md as changes made after approval, round 1.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — de8d800

Scope: correctness against code and security of the plan (approved 2026-11-13; step 2 shipped
in pull request #31); incremental over round 1's changes (plan files as at fb13b26); no runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-B1 — Low:** the harness's named residual was stated wider than the code allows: a
  creation alone gives a node nothing to delete, and the node where the new volume is attached
  refuses the delete, which stops the deletion. Evidence: replica/mod.rs:177, 253, 285 (chunks
  only through `hello`); lab.sh:702-704; service.go:286-288; engine.rs:696-698; lab.sh:1758-1767.
- **R2-B2 — Low:** the two risks were unaccepted in decisions.md but stated as settled in the
  spec and design, and the operator precondition the invariant relies on ("no other lab command
  while a deletion runs") was documented nowhere. Evidence: spec.md:27-28, 78-80, 112-115;
  design.md:260-262; docs/lab.md:214-223.
- **R2-C1 — Low:** "The next step" left out the shipped follow-ups text that R1-C2 corrected in
  the plan. Evidence: docs/followups.md:26 and 281-282; #13 spec.md:81, design.md:560-561 and
  822-824.
- **R2-N1 — Nit:** wrong lab.md line numbers in the next step. Evidence: docs/lab.md:246 (step 7)
  and 248-252 (the wrong-confirmation paragraph).
- **R2-N2 — Nit:** "not since #30" (S2) and "would close the wipe" (decisions) left out #30's own
  residual cases. Evidence: docs/issues/30/spec.md:86-87 (S8) and S9.

Changes/resolution:
- R2-B1: the residual now reads as the reviewer words it (created and attached; first session
  reaching a replica node between the re-check and its delete, seconds; replicas lost on the
  nodes it is not attached on; `make vol-create` then `make vol-attach`, or a second campaign),
  in decisions.md, spec S5, design step 4 and Risks; the invariant notes that an attached node
  refuses the delete.
- R2-B2: both risks recorded as accepted under the user's instruction of 2026-11-13 (continue
  unattended, choose the recommendations), as the coordinating session applied it, with the
  reason (lab harness only, seconds, #13's S13 removes the per-node deletes). The spec (the
  invariant, S5, S7.8) and design (step 4, step 7, Risks) say "accepted (decisions)". S10 and the
  next step add "no other lab command while a deletion runs" to docs/lab.md; the follow-ups'
  lab-wide lock item widens to cover it.
- R2-C1: the next step's docs/followups.md bullet adds §14 step 1 (the node daemon,
  `flaminio-io node`, #13; `vol-confirm`, `confirm.sh` and `flaminio-meta confirm` go with it)
  and row 26's "When" column (#13); both go with #13 if #13 lands first.
- R2-N1: the next step cites docs/lab.md:246 and 248-252.
- R2-N2: S2, the next step's usage text and follow-ups item, and decisions' incarnation
  follow-up say "outside #30's S8-S9 cases".
- All recorded in decisions.md as changes made after approval, round 2.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 5dfe0230471a67cfee487a9ae3bb8ecfb9af56fd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
