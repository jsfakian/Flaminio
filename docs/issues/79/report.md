# Issue #79: plan review report

## Review rounds

### Design R1 — reviewed 2868b319b8d24e934af916f9d9cc00a9464a8f1d

Reviewers: **review79_code_r1** (design-code) and **review79_sec_r1** (design-security),
fresh independent reviews. Nine distinct findings after merging overlap; code independently
confirmed the device-reuse and detach races as High while security rated device reuse Critical.
The findings below record the supplied reviewer evidence, not a claim that runtime tests ran.

| ID | Severity | Finding/evidence at reviewed revision | Change addressing it |
|---|---|---|---|
| R1-S1 | Critical | Design83–90 checked only new stage path; active raw stage A then filesystem B could mkfs unrecognized raw data. | Scan all volume/device records/effects before probe; retain incarnation access/format profile beyond path retirement; reject raw/fs/other-stage conflict with zero probe/format. |
| R1-S2 | Critical (code High) | Design39–42/83–87 learned diskseq from a path opened after V could die and W reuse it. | #15 shared authoritative device-info/open/device-info binds expected FirstEpoch/boot/instance/generation/rdev/diskseq from daemon-held fd; pin expected object, fail closed on old profile. |
| R1-S3 | High | Design118–121/144–146 retained detach V by name; replacement between client status and daemon execution could be detached. | #15 conditional detach compares entire expected lifetime under registry/entry locks before routing captured generation; stale replacement untouched; no legacy retry fallback. |
| R1-S4 | Medium | Native unmount on a held mount-root FD was not a Linux6.6 successful ordinary cleanup procedure. | Close all mounted-root refs; trusted pinned-parent/basename umount2 with UMOUNT_NOFOLLOW under serialized cooperative namespace ownership, no lazy/force; busy/unknown refuses and real success is required. |
| R1-S5 | Medium | Crash after unlink before record removal allowed identical dev/inode/type ABA and foreign-leaf deletion on retry. | Unlink only with continuous original created-leaf fd witness; after interruption preserve any existing leaf, even identical inode. Test same-number/new-lifetime replacement. |
| R1-S6 | Medium | No durable format-start checkpoint: pre-signature mkfs crash could look blank and trigger another format. | Fsync profile format-start before tool and finish after success; uncertain started marker survives cleanup, always manual maintenance instead of reformat. Test killed tool before signature. |
| R1-S7 | Medium | Go ID guard missed direct Rust control.rs82–103 daemon dispatch. | Require Rust grammar/arity validation before registry/engine for every volume-bearing command, shared Go/Rust vectors and direct-socket rejection tests. |
| R1-S8 | Medium | Effective readonly considered filesystem only; reader-only raw block plus Readonly=false could bypass unsupported-RO gate. | Derive effective readonly for both access types and map complete access-mode/Readonly/flag matrix to fail-closed block gate. |
| R1-C3 | Medium | tmpfs/loop bind tests did not exercise shipped blkid/mkfs.ext4/mount inherited-FD operands and native full cleanup. | Built CSI-image disposable ext4-loop lifecycle with exact image/kernel/tool versions, actual FD probe/format/stage/publish/writes/native unpublish/unstage/loop cleanup and error cases. |

Shared root cause fixed once in **#15 commit 8af7aeb** (parent 48b0ae1):
`docs/issues/15/device-lifetime-contract.md`, spec/design consistency and historical-decision
amendment. That is an unimplemented, unapproved dependency; #79's baseline does not already
supply it. Next review must inspect that cross-issue delta as well as #79's R1 diff.

R1 edits revise spec/design/Proposed ADR coherently and record draft choices in decisions.md.
Documentation validation: diff whitespace check, line-budget check and review of the shared
protocol/spec/design references. No source implementation, runtime/kernel/CSI-image/real mount
or lab tests were run; their test cases are future implementation requirements.

**Status:** addressed in draft, awaiting fresh incremental correctness/security review. This is
not an all-clear or approval. Raw-block readonly mechanism/support, shipped-kernel/tool behavior
and cooperative namespace ownership remain explicit release gates; no risk is accepted here.

### Design R2 — reviewed 31bab4a2f5c3146790fec524e9368be0b540aa1e

Actual reviewers: **review79_code_r2** (incremental design-code) and **review79_sec_r2**
(incremental design-security), dependency #15 reviewed at8af7aeb. Durable supplied input:
`docs/issues/review-inputs/79-round-2.json` in the main checkout. Three distinct pending findings:

| ID | Severity | Supplied finding | Draft correction |
|---|---|---|---|
| Review79-R2-C1 | High | design54–61/71–74/89–90: absent local profile/FirstEpoch cannot prove first initialization across nodes/state loss. | Permanent per-incarnation use authority created atomically with explicit new-volume profile; private initialization admission, one-time Started CAS, auth/model-mutant tests and exact #76/#80 integration in initialization-contract.md. Unknown/raw/import/legacy never grants format. |
| Review79-R2-C2 | Medium | design114–123: ordinary pathname umount depends on unspecified namespace cooperation. | kubelet-cleanup-contract.md pins verified v1.35.0 executor/wildcard conflicts, block reference removal and post-RPC parent cleanup; exact paths, deployment admission/RBAC/profile checks and successful cleanup/concurrent/deadline tests replace the vague gate. |
| Review79-R2-S3 | Medium | design54–57/89–90: device() can attach before reservation, losing durable cleanup intent. | Fsync reserved_unbound operation before every node attach/init mutation, fsync epoch then op-bound daemon attach and #15 tuple bind; explicit absent/starting/lost reply/fsync/crash handling, no volume-name adoption. |

Updated intent/spec/design/Proposed ADR and decisions coherently; two exact appendices carry the
new contracts. #15 remains unchanged. Read code: create/Attach and per-method auth, CSI controller
creation/placement, daemon dispatch and engine attach entry; official Kubernetes v1.35.0 source
links and the inspected functions are recorded in the cleanup appendix. Source inspection proves
that tagged ordering, not arbitrary vendor kubelet/runtime behavior.

Validation: documentation diff whitespace, line budgets, local links and cross-contract inspection.
No source changes, runtime/model/admission/image/kernel tests or lab execution. Planned tests are
requirements, not successful results. **Status: addressed in draft; fresh correctness/security review
required.** No all-clear/approval or accepted risk; raw-block readonly remains a separate open gate.

### Design R3 — reviewed 4164f6390074a643fe2232aa9a3a8caf18d975ea

Actual fresh reviewers: **r79c3** (design-code) and **r79s3** (design-security). Both confirmed
all three R2 findings resolved. Reviewed dependencies: #15 at8af7aeb, #76 at9642649, #80 at8a92832.
The three new findings and their supplied evidence are distinct from that prior resolution:

| ID | Severity | Supplied finding/evidence | Draft correction |
|---|---|---|---|
| Review79-R3-C1 | High | initialization-contract163–165 required no public init attachment without an explicit caught gate-bypass mutant/independent assertion. | Named bypass_public_init_admission mutant, independent issuance/start/exposure history oracle and exact service/CSI/daemon replay; ordinary credentials during Started violate immediately even if downstream checks refuse. |
| Review79-R3-C2 | Medium | initialization-contract143 required authority_revision locally but no RPC supplied it. | Remove the unused local field; retain actual RPC operation/epoch and process nonce, server-internal revision CAS unchanged. |
| Review79-R3-S3 | High | initialization-contract71–75/133–136 allowed absent Reserved attach at the old epoch after daemon restart, without durable ran_epochs/local replica proof. | Required Proposed #83 handoff5a2fcd8 binds epoch/FLCT/purpose to daemon nonce; first Reserve persists nonce after new-epoch CAS, same-nonce recovery only; changed nonce refuses/manual recovery. Same-daemon plugin restart still recovers exact operation. |

Updated spec/design/initialization appendix/Proposed ADR/choices. #83's exact FLCH134, field tags
and purpose rules are composed without changing FLCT/FLCI; #15 lifetime ABI remains unchanged.
No other issue files changed. Validation: whitespace, spec/design budgets, local/dependency links
and cross-contract field/nonce/retry inspection. No implementation/runtime/model/lab tests ran.
**Status: addressed in draft, awaiting fresh correctness/security review.** No approval/all-clear;
raw-block readonly and implementation/conformance remain open.

### Plan review round 4 — 52f7a966fa29cbea967d9252880636a328bd0b26

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r79c4 (incremental design-code), r79s4 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed all three R3 findings resolved: independent public-init issuance/exposure oracle/caught gate-bypass mutant, removed unsourced local authority_revision retaining server CAS, and mandatory #83 signed process handoff/same-daemon nonce binding with changed-daemon old-epoch refusal. Checked affected schemas/retries/credentials and dependencies; zero comments including Nits. No implementation/runtime/lab tests or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.
