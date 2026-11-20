# Issue #55 — report

## Review rounds

### Plan review round 1 — cd584f4ca92178b174cf1a70d522e87bed5dfcdf

Scope: correctness against code and security of the compact-budget measurement plan;
no runtime implementation. The saved reviewer had both design-code and design-security
focuses; its identity was not recorded. Input: `docs/issues/review-inputs/55-r1-both.md`
in the audit checkout. Source checked against local `origin/main` at `a1cc2421ad5a`.

Findings: 1 High, 7 Medium, 9 Low, 5 Nits. Fixes prepared on 2026-11-19:

- **R1-C1 — High:** fl-c could receive the second replica outside the measured inventory.
  Require the two-data-node topology and verify every owned chunk's placement/full ISR;
  compute each node's slope from its own allocation. Add a fl-c refusal fixture.
- **R1-C2 — Medium:** the budget verdict did not name its memory metric.
  Use the service/container cgroup's maximum sampled memory.current, including shmem,
  cache and kernel charges; keep PSS/RSS as diagnostics. Test opposing PSS/cgroup verdicts.
- **R1-C3 — Medium:** disktool reads checkpointed bitmaps and pool pairs without replay.
  Accept allocation evidence only with zero replay batches; otherwise reopen without
  formatting and inspect once more. Keep pool pairs distinct from derived map-page counts.
- **R1-C4 — Medium:** map growth was mixed with heap retained after writes.
  Compare fresh-open/no-device PSS at both populations; report post-load retention separately.
- **R1-C5 — Medium:** expected members and stable identities were unspecified.
  Add the per-form/per-host member table and UID/cgroup ownership sources. Thread/probe
  churn is distinct from daemon replacement; aggregate CPU includes exited children.
- **R1-C6 — Medium:** excluding all unrelated replicas contradicted preserving existing data.
  Keep fixed, recorded unattached replicas; refuse other active consumers and allocations
  that change during the matrix. Never delete pre-existing data to prepare a run.
- **R1-C7 — Medium:** architecture wording instructions contradicted the intent.
  Keep ARCHITECTURE/accepted ADR wording as required; update the results note/README roadmap
  and remove the unsupported claim that another audit already corrected the wording.
- **R1-C8 — Low:** zero-copy staging was described as additional queue memory.
  Correct it to a shared memfd mapping replacing the anonymous queue buffers.
- **R1-C9 — Low:** tick-derived process CPU obscured small idle usage.
  Specify schedstat diagnostics and cgroup usage_usec totals, with identity/coverage rules;
  retain stat ticks as a cross-check.
- **R1-C10 — Low:** the warm-up job was unspecified.
  Name read-only fio settings within the written extent and require qualified queue coverage.
- **R1-C11 — Low:** sampler invocation, Python checks and observation costs were infeasible.
  Stream Python over vssh, specify unittest invocation through make lint, limit k3s to endpoint
  cgroup totals, and move control-socket state checks outside the sample window.
- **R1-C12 — Low:** mutation authorization and interrupted cleanup were unspecified.
  Add CONFIRM=matrix, explicit exclusivity checks and MODE=cleanup using the lifecycle
  manifest; persist operation intent/acknowledgement and preserve ambiguous state.
- **R1-S1 — Medium:** inherited FORMAT=1 could reformat a node.
  Pin FORMAT=0 and all recorded I/O settings, clear inherited attachment state and avoid the
  formatting deletion helper. Test hostile inherited FORMAT/ZERO_COPY/ATTACH values.
- **R1-S2 — Low:** capture metadata reads defaulted to admin.
  Require AS=io for read-only metadata calls and assert no administrator use in capture.
- **R1-S3 — Low:** raw ExecStart/metadata responses could leak token/key-path data.
  Filter nonsecret flags before persistence and store only validated metadata fields;
  tokens stay transient. Add secret/token/key-path filtering fixtures.
- **R1-S4 — Low:** tenant pod names could impersonate product members.
  Select flaminio-system controllers and follow owner UIDs; test a tenant namespace decoy.
- **R1-S5 — Low:** cleanup did not bind deletion to the owned volume incarnation.
  Validate a unique cbudget id and record create/acknowledged attach epochs. Refuse mismatches,
  missing ids or ambiguous results. GetVolume has no first_epoch and DeleteVolume accepts
  only an id, so the epoch check is explicitly conditional on lab exclusivity, not atomic.

All five Nits receive small wording/reference fixes: add the performance-plan budget section,
restore missing spaces, correct the startup-log lines, name tracefs/loss checks and state that
the operator belongs to #43. No Nit is left intentionally unresolved.

The approved measurement-only scope remains. Existing approvals of 2026-11-13 are preserved;
the changes above are recorded as made after approval. No new approval or accepted risk is
inferred. The exclusive-admin check-then-delete race is named, not accepted; uncertain ownership
must leave data untouched. No code, lab measurement or runtime test result is claimed.

Validation: document whitespace, changed prose width and relative links only. The sampler,
analysis and fake-lab tests above are requirements for implementation, not tests run here.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made
after approval.

### Plan review round 2 — ec52405ff1ca92fe8338989981cd416b9cbdccb0

Scope: incremental `cd584f4..ec52405` under this issue, excluding report.md; correctness and
security against local origin/main. Fresh reviewers: `/root/review55_r2_code` and
`/root/review55_r2_security`. Full inputs are retained as `55-r2-code.md` and
`55-r2-security.md` under the audit checkout's docs/issues/review-inputs.

Findings: 1 Medium and 1 Nit from correctness; security is clear with zero comments. Earlier
findings are resolved except the pod execution of R1-C3/C4, covered by the finding below.

- **R2-C1 — Medium:** deleting a #12 DaemonSet pod permits immediate replacement, so it cannot
  provide the required offline inspection window. The correction defines a matrix-only,
  per-node scheduling hold using the existing dataplane selector label, recorded before
  mutation, with pod/store absence checked before inspection. Recovery-open temporarily
  restores/reapplies the hold; fresh-open capture follows exact state restoration. Lifecycle
  cleanup restores the saved selector and deployment on interruption. Planned fakes cover
  absence of a successor during inspection and restoration after interruption/failure.
- **R2-N1 — Nit:** decisions.md referenced the removed "Proposed budget table" paragraph.
  Point the reference at the current per-node target paragraph.

The measurement-only scope and existing approvals remain. These changes are made after
approval; no new runtime behavior, lab result or accepted risk is claimed. The held-stopped
procedure preserves #12's deployment form and changes only the recorded selector on the
owned data node, with ambiguity preserved for recovery.

Validation: document whitespace, changed prose width and relative links only. All sampler,
matrix and interruption tests remain planned implementation checks.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made
after approval.

### Plan review round 3 — 47b183ebbabf2d1629644cff33cb2c0eaa033b4f

Scope: incremental `ec52405..47b183e` under this issue, excluding report.md. Fresh reviewers:
`/root/review55_r3_code` (design-code) and `/root/review55_r3_security` (design-security).
Full inputs are retained as `55-r3-code.md` and `55-r3-security.md` under the audit checkout's
docs/issues/review-inputs.

Both reviewers report zero comments including Nits. R2-C1 and R2-N1 are fixed: the recorded
per-node selector hold prevents replacement during inspection, requires pod/process/store
absence, bounds recovery-open/re-hold and restores the exact saved selector through cleanup;
the budget reference names the current paragraph. No revision fallout was found.

Validation: issue-document whitespace, changed prose width, relative links and review-ledger
consistency checks pass. The reviews are static plan/codebase checks. No runtime implementation,
test suite, lab measurement, push or GitHub write was performed.

Status: clear. The plan was approved on 2026-11-13; the fixes of rounds 1–2 were made after
approval. No new approval or accepted risk is inferred. Delete-by-id still depends on declared
exclusive administration; the lack of an atomic incarnation precondition remains explicitly
named in decisions.md. The measurement-only slice leaves the rest of issue #55 open.
