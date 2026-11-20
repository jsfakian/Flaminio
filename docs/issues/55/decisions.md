# Issue #55 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`0ef85980ff8d38508f633f3899f938d2db130ece4af08921a3a9507a1cd15c8d`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Plan review round 1 (2026-11-19) fixes, made after approval: see below.
Plan review round 2 (2026-11-19) fixes, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **The data-plane target this measurement checks:** as proposed, at most 1.0 core and 512 MiB
  resident per node, in the defined idle and population states; it is a target to measure
  against, not a cap or a product guarantee. [spec.md](spec.md), S1; [design.md](design.md),
  Scope and inventory (the per-node data-plane target paragraph).
- **The final compact-mode scope and budget, and the product-set total target:** decided after
  the data; no recommendation in the plan. Settled 2026-11-13 (user's instruction): see below.
  [spec.md](spec.md), Open questions;
  [design.md](design.md), Scope and inventory.
- **Measurement permissions and observer tooling:** as the design proposes, wakeups come only
  from a qualified `sched_wakeup` or perf trace where permitted, and are otherwise recorded as
  unknown with the reason; no eBPF agent or kernel security change is installed silently, and
  the observer's cost is measured and kept out of the product's. [spec.md](spec.md), Open
  questions; [design.md](design.md), Sampling and metrics.
- **Map-allocation evidence:** as the design proposes, offline disktool inspection only after an
  explicitly authorized stop of the owned service, then restored (or a qualified read-only
  diagnostic if #71 provides one); without it the allocation is an estimate or unknown and no
  measured map slope is claimed. [spec.md](spec.md), Open questions; [design.md](design.md),
  States and map slope.

## Settled at the user's instruction of 2026-11-13

- **The final compact-mode scope and budget, and the product-set total target:** the plan's own
  procedure for gathering the evidence: the measurement runs first (S1–S7, `make
  compact-budget`, the dated report and its budget table), and the scope, budget and product-set
  target are decided from that report in a later step; no number is set now. Until then the only
  target is the per-node data-plane one settled at approval, and product-set totals are recorded
  as observations. Recommended by Claude, no recommendation in the plan: it is evidence to
  gather, and the plan forbids inventing footprint numbers before the data.


## Choices — plan review round 1, 2026-11-19 (after approval)

- **R1-C1/C6:** keep pre-existing unattached idle replicas as the recorded fixed baseline;
  refuse active consumers/attachments/campaigns and any extra live data node (fl-c included).
  Require every test chunk's replicas/full ISR to be {fl-a, fl-b}; derive each node's slope
  from its own qualified allocation. This replaces the impossible requirement to have no
  unrelated replicas while preserving existing data. S2/S4/S5 are clarified accordingly.
- **R1-C2:** the budget verdict uses maximum sampled memory.current and window-average
  cgroup CPU across repetitions. The unit's service cgroup includes children; the pod's
  dataplane-container cgroup includes its exec probe. File, shmem and kernel charges count;
  PSS/RSS are separate diagnostics. This makes the approved target checkable without claiming
  that such a sample proves a 512 MiB allocation limit works.
- **R1-C3/C4:** disktool reports checkpointed data bits/pool pairs, not map-page counts. Only
  zero journal replay and valid slots qualify allocation evidence. A bounded recovery-open,
  stop and inspect retry may qualify it; otherwise it stays estimate/unknown. Fresh-open,
  no-device PSS at both populations is the primary slope; post-load memory is separate.
- **R1-C5/C9/C11:** enumerate the expected members by host/deployment and controller UID.
  The agent is a one-shot/probe, not a daemon; #12's clear plan uses direct-mode pods, while
  #74's helper, if present, belongs to its unit cgroup. Thread/probe churn is not a daemon
  restart. Cgroup cpu.stat is authoritative including exited tasks; schedstat attributes only
  observed thread lifetimes, not a falsely complete process total. Use a Python SSH runner,
  explicit Python tests and bounded k3s endpoint totals; query status outside sample windows.
- **R1-C7:** edit the eventual results note and README roadmap to distinguish this evidence
  slice, and add the performance-plan budget section. Keep ARCHITECTURE/accepted ADR wording
  untouched as intent requires; no root-audit wording correction is assumed.
- **R1-C8:** zero-copy memfd staging replaces anonymous queue buffers; it is not added to them.
- **R1-C10:** warm-up is read-only, within the written extent, sized by io_buf/depth with one
  pinned job per configured queue. Record the job and any unproven queue coverage; warm-up
  alone proves neither full staging residency nor map-memory size.
- **R1-C12/S1:** require CONFIRM=matrix before mutation, allow cleanup of a prior lifecycle
  manifest, and persist operation intent/results. Force FORMAT=0 and pinned original settings
  on every unit restart; clear inherited attachment/token settings. Pod restarts follow #12
  without switching deployment form. No destructive generic all-node/delete-with-format path.
- **R1-S2/S3/S4:** capture reads metadata as io, without admin. Allowlist flag values before
  artifact logging and never persist raw metactl/ExecStart, capability tokens or key paths.
  Bind inventory to flaminio-system controller UIDs, not tenant-controlled pod-name prefixes.
- **R1-S5:** create_epoch is the create response's volume.epoch; GetVolume exposes current
  epoch only, so the recommended direct first_epoch comparison is unavailable. Persist
  expected_epoch from each acknowledged owned attach and require exact current-epoch match
  before destructive cleanup. A recreated id has a higher epoch; missing ids, mismatches or
  unknown operation results stop automation. A saved delete acknowledgement permits only
  bounded tombstone polling through existing CSI sweeps; absent id without that acknowledgement
  requires manual recovery. No direct by-name replica deletion or new metadata API is introduced.
- **R1-S5/C12 risk — named, not accepted:** DeleteVolume accepts id only; a get followed by
  delete has no atomic incarnation precondition. The harness requires exclusive operator use
  and refuses if that precondition is absent; it supplies no lock against a concurrent admin.
  Ambiguous state is preserved for manual recovery rather than claimed safe or silently retried.
- **N1–N5:** add (rather than update) a budget section, fix spacing/startup-log references,
  name tracefs and its lost-event checks, and state the absence of an operator in this slice.

These are changes made after the recorded approval, not new approval, implementation,
measurements or accepted risks. No dependency plan is changed.


## Choices — plan review round 2, 2026-11-19 (after approval)

- **R2-C1:** pod deletion alone leaves the DaemonSet free to reopen the store. For offline
  inspection, save and remove only the node's existing dataplane selector label, with node/
  controller identities and conditional API patches. Wait for both pod and host process/store
  absence before inspection, keeping the hold through its end. If replay is needed, restore
  the saved label for one acknowledged recovery-open, then re-hold before reinspection.
  Restore exactly the saved label/deployment for fresh-open capture and cleanup/error paths.
  Node/template/label changes and missing stop evidence refuse inspection or blind mutation;
  independently owned label restoration remains possible when volume ownership is ambiguous.
  This preserves #12's pod deployment form, without changing that clear plan or narrowing S4.
- **R2-N1:** the settled budget entry now references the current per-node target paragraph.

These changes are made after the recorded approval. No new approval, implementation,
measurement or accepted risk is asserted.
