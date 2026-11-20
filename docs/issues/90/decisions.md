# Issue #90 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`c8126d6e64c1c2d9f9be994e55656aa6689949e8a63165129adb2f03ec1bad54`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet.

## Current status

The dedicated tracker move was authorized by the user; #90 exists. Spec and design of the
published clear snapshot are approved 2026-11-20 (see Approvals). That approval is
implementation permission for this issue's plan; accepting any ADR remains a separate
ship-spec step. No #61 review round is relabeled as a #90 review.

## Historical source approvals — provenance only

At #61 source commit `5696573a126c0abe26347d3c9700e557489613cf`:

- **Spec:** Ioannis Sfakianakis approved on 2026-11-13.
- **Design:** Ioannis Sfakianakis approved on 2026-11-13, including its issue-local ADR.
- **Enrollment policy Q1:** administrator-reviewed evidence under maintenance binds only known
  unchanged original stores; ambiguous/missing/replaced legacy provenance remains quarantined.
- **Source Q2:** same-node replacement uses full reconstruction and earned store refs; the
  source claimed a shared #72 ReplicaRef/protocol-v2 ABI. The actual dependency plans do not
  contain that shared ABI, and it conflicts with #51/#58 bytes. That historical wording is
  recorded here, not accepted as the corrected #90 contract.
- **Tracker split:** the shared store-identity root cause belongs in a dedicated issue used by
  #45 recovery and #61 cloud policy. The authorized move created #90; #61 remains composition.

## Proposed corrections after source review

Actual predecessor reviews are `/tmp/flaminio-61-r1-code.json` and
`/tmp/flaminio-61-r1-security.json`, reviewing the source hash above. They are findings to fix,
not #90 approvals or #90 review rounds. Dependency snapshots remain read-only; parent report
records actual hashes and the fresh #90 review will verify them again.

- **C1/SEC06/SEC08:** #90 owns ReplicaRef and the composed identity wire profile, exact cleanup
  schemas and byte budgets. Existing #51/#58 schemas remain read-only; second-to-land code
  applies the explicit profile/vector hooks. No claimed #72 ABI or deferred budget calculation.
- **C2/SEC01/03/05:** immutable bounded per-session role grants, current metadata Check,
  durable-fence callbacks and new Hello for added operations; full current/history/root frontier
  rechecked at join. These are concrete mechanisms in core-design.md, not future release gates.
- **C3/SEC02/07:** exact ref/lifetime cleanup auth and all gate/mode comparisons determine the
  recomputed transaction caps in contracts/budgets, including producers, validators and models.
- **C4/SEC09:** one service-owned lifetime survives forget/UUID ABA, binds #52 source proof and
  #72 observations/admission; replacement conservatively charges old and new copies.
- **C5/SEC04:** pure bootstrap/replacement state/events/actions, independent per-rule simulator
  checks and named caught mutants, real remote/local adapters, bounded grants and fair reserved
  metadata control capacity. Resource deadlines do not create epoch/fence authority.
- **C6:** migration.md defines durable administrator-only inventory/enroll/verify/activate APIs,
  provenance/quarantine, bounded CAS batches and restart/lost-reply behavior. Policy history
  does not imply these newly specified mechanisms were approved.
- **SEC10:** qualified physical fixtures are required for production under #83; separately
  authorized pre-#83 VM reformat is exploratory and cannot be claimed as production evidence.

No force promotion, below-minimum exception, cloud classifier/claim policy, same-UUID rollback
claim or active ADR modification is introduced. Future feature hooks apply only to implemented
profiles; ordinary identity-only operation does not wait for unrelated future mechanisms.

## Proposed fixes from actual #90 R1 at 98390ae

The first full #90 code/security reviews read commit
`98390ae141d287ec9e8e7443bac6389bd3ade9f7`; their actual findings are recorded by the parent,
not new approvals. These corrections remain unapproved.

- **R1-C1 / SEC90-R1-01:** durable per-chunk/ref initial pending authority is distinct from
  retained create_operation. Actual create/fence completion precedes a bounded metadata earning
  CAS; new Hello obtains MEMBER and consumed INITIAL never recreates an absent earned chunk.
  Separate independent simulator checks catch early earning and consumed-initial replay.
- **R1-C5:** proven FE0 ordinary/snapshot volumes retain their existing data/fences/history,
  use a persisted immutable legacy LID and explicitly versioned signed token/profiles, and skip
  destructive prior-incarnation cleanup. No arbitrary positive epoch rebase or old token fallback.
  Positive-only root/export modes on FE0 explicitly quarantine/refuse, not silently break I/O.
- **SEC90-R1-03:** an administrator can correct a converted item by original/current hash and
  revision/evidence CAS, preserving charges and provenance receipts. Correction/changed observation
  invalidates verification generation and requires a complete rescan; READY is immutable.
- **R1-C2 / SEC90-R1-02:** migration consumes bounded tombstone root/ref-row items and bounded
  legacy source projections defined by contracts.md, never a whole unbounded union.
- **R1-C2 / SEC90-R1-02 (storage/list):** schema3 tombstone roots, exact-ref charge rows and indexes have explicit value/key/transaction caps. Build and legacy conversion stream bounded units; list and sweep bound examined keys, calls, results and bytes. No volume-wide exact-ref array or whole-prefix response remains.
- **R1-C3:** retain #76 proof-scoped delete-only unpublished rollback, private-root revocation and orphan collection under NOSPACE. Exact mode/header/root/key revisions prove these deletes cannot change live authority; all live ref/ledger mutations remain gated. Independent model/mutant and isolated real-etcd alarm cases test the exception.
- **R1-C4:** #45 remains authoritative for own-IO/admin-only heartbeat, atomic node-state plus TTL operations and lease registration by committed revision. #90 adds exact UUID/generation and mode comparisons without reviving agent heartbeats or a single-put replacement API.

## Proposed fix from actual #90 R2 at c734bac

- **SEC90-R2-01:** [local-target-contract.md](local-target-contract.md) owns the missing
  whoever-lands-second FE0/LID integration with the canonical #15/#79/#83 technical snapshots.
  Existing FLCH134 signs the current process nonce and exact FLCTv3 hash, preserving new-epoch
  issuance/ran_epochs. Paged AttachIdentityVolume adds nonce12/handoff5 without reusing its
  existing revision fields. Versioned device-info/detach/status and schema2 normal intent compare
  exact LID alongside every existing lifetime field. Normal/paged CSI acceptance, stale LID/
  handoff, restart and lost-reply tests are explicit; legacy fresh filesystem initialization
  remains refused. This is an unapproved draft correction, not dependency/ADR acceptance.
