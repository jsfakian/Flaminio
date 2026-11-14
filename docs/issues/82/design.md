# Issue #82: Evidence-based node failover — design

[Specification](spec.md). Settled choices, if any: `decisions.md`.
Approved 2026-11-13; not implemented. The first Proposed profile is [manual-lockout-v1](manual-lockout-contract.md).
Settled at approval, 2026-11-13: manual-lockout-v1 is accepted (see decisions.md, Settled at approval).

## The facts it starts from

- **Absence becomes authority.** `control/failover/failover.go:83–95` marks from NotReady,
  Grace and missing heartbeat; it removes marks on Ready plus heartbeat. Neither operation
  receives shutdown evidence. Existing `TestStep` expects both decisions.
- **Identity and ownership are incomplete.** `Node` at `failover.go:32` lacks UID and machine
  identity. `kube.go:90–107` omits UID/providerID; `toNode` treats annotation presence as
  ownership (`:146`). `SetOutOfService` (`:167–194`) re-reads by name, removes all same-key/
  effect taints, then conditionally patches its own fresh resourceVersion. It does not compare
  the incarnation selected by Step or refuse a foreign mark installed after listing.
- **The controller always starts.** `control/cmd/flaminio-csi/main.go:69–76` configures no
  evidence provider. `deploy/csi/rbac.yaml:121–130` permits only node get/list/patch, which
  usefully prevents direct pod/attachment deletion.
- **Existing storage safety is separate.** `Target::advance_chunk` in
  `dataplane/crates/flaminio-repl/src/target.rs:2195–2277` fences and reconciles before Ready.
  This design does not alter it or treat Kubernetes marks as permission for I/O.
- **The lab assumes automatic recovery.** `scripts/lab.sh:1142–1205` powers off a VM then
  waits for automatic mark and later unmark. Those waits need evidence integration before
  the harness can pass under the new policy. #43/#48/#49 consume the revised contract.

## Candidate and evidence boundary (S1–S4, S9)

Keep readiness, Grace, metadata heartbeat and another-Ready checks only as candidate filters.
Add `control/failover/evidence.go` with the internal contracts below. Nil authority is the
production default and always returns evidence-unavailable; no timeout-based fallback exists.
Do not ship a test authority as an executable option or trust a Node annotation as proof. Manual-lockout-v1 is the only
Proposed production authority; all other profile names refuse.
Settled at approval, 2026-11-13: manual-lockout-v1 is accepted as that authority (see decisions.md, Settled at approval).

Order each attempt: list candidate; fetch current Node; resolve exact identity; ask authority;
validate proof; invoke the conditional mutation with that identity, proof and operation ID.
The mutation re-reads Node and rechecks identity, ownership and proof applicability. A proof
must establish shutdown/fencing throughout the mutation and ensuing recovery, not merely a
historical power-state sample. An observation TTL alone cannot prevent a concurrent restart.
The appendix specifies how the first profile holds its lock and authorizes terminal retirement.
Provider errors, absent identity, expired/revoked proof or changed generation block mutation.

A transport timeout after PATCH is an unknown outcome. Re-read the Node before retry, locate
an identical operation record, and revalidate proof. A missing Node is not shutdown evidence;
never recreate it. A replacement UID or machine generation starts a new investigation.

The concrete first adapter, authenticated configuration, physical inventory, signed assertion,
restart interlock and terminal return protocol are in [manual-lockout-contract.md](manual-lockout-contract.md).
Its trusted physical operator maintains isolation until the exact old UID is retired; no
sampled-power, VM or unattended-return provider is supported by this draft.

## Conditional marks and cleanup (S3, S6–S7)

Replace `SetOutOfService(ctx, name, on)` with separate `Mark`/`Clear` requests below. Keep
merge-patch resourceVersion concurrency control, add expected UID and machine identity checks,
and compare the current taint and operation annotation before constructing any patch. Preserve
raw unrelated taints/annotations. Conflicts restart at observation, never retry an old patch.

A mark adds exactly `node.kubernetes.io/out-of-service=nodeshutdown:NoExecute` and an operation
record atomically. If any out-of-service taint already exists without the identical verified
operation record, report foreign/conflicting mark and do nothing. Annotation presence or a
matching timestamp never grants ownership. Multiple instances converge on the recorded winner.

`Clear` requires a matching operation and a verified-return permit from the configured return
policy. The first profile always refuses Clear and uses terminal old-UID retirement/fresh node
identity instead, as specified in the appendix. Ready plus heartbeat cannot supply a permit. Legacy
`flaminio.io/out-of-service` timestamp values are foreign/legacy marks; never adopt or remove
one automatically. Document operator inspection and separately authorized cleanup on upgrade.
If no return policy is configured, retain the mark and report cleanup-blocked. The return
policy must cover stale workloads/attachments and safe fence release before permitting removal.
No controller release or untaint is inferred from successful rescheduling alone.

## Scoped recovery and takeover (S5, S8)

No scoped partition recovery is implicitly enabled by this correction. Report scoped mode
unavailable; retain the existing RBAC without pod or VolumeAttachment mutation permissions.
If the human chooses to include it, this design needs an addendum defining the opt-in API,
mixed-storage eligibility and exact deletion preconditions before implementation. Until then,
partitions produce zero recovery mutations, including for Flaminio-only nodes. Document that
storage fencing cannot stop an old process's network or other external side effects.
Settled at approval, 2026-11-13: scoped recovery is not included (see decisions.md, Settled at approval).

Keep Rust/proto/wire/on-disk contracts unchanged. Kubernetes actions cannot bypass attach,
epoch issuance, all-effective-ISR fencing, lease waiting when needed, or reconciliation.

## Contracts

Proposed internal Go contracts; these are not new metadata RPCs or a provider wire protocol:
Settled at approval, 2026-11-13: these contracts are accepted with the plan (see decisions.md, Settled at approval).

- `Identity {Name, UID, ProviderID, MachineGeneration string}`; provider-specific generation
  mapping is mandatory; the first profile uses the appendix inventory. Names alone are insufficient.
- `ShutdownProof {Identity; Authority, Reference, FenceGeneration string}`; opaque nonsecret
  references identify a durable authority operation. No transferable bearer credential is stored.
- `ReturnPermit {Identity; OperationID, Reference string}`; authority/return policy validates
  this against the exact held fence and recovery operation before cleanup.
- `Authority.ResolveIdentity(ctx, Node) (Identity, error)` binds Node UID/providerID to the
  authority's machine generation; unresolved or ambiguous mappings fail closed.
- `Authority.VerifyShutdown(ctx, Identity) (ShutdownProof, error)` and
  `Authority.ValidateShutdown(ctx, ShutdownProof) error` must fail on uncertainty/revocation.
  `Authority.VerifyReturn(ctx, Identity, operationID) (ReturnPermit, error)` and
  `Authority.ValidateReturn(ctx, ReturnPermit) error` have the same fail-closed semantics.
- `Kube.Mark(ctx, MarkRequest) error`, where `MarkRequest` carries expected `Identity`, proof
  and operation ID; `Kube.Clear(ctx, ClearRequest) error`, carrying expected identity,
  operation ID and return permit. The adapter receives the authority for final validation.
- Keep owner annotation key `flaminio.io/out-of-service`; replace its timestamp value with
  versioned JSON `{version:1,nodeUID,providerID,machineGeneration,operationID,authority,
  evidenceRef,fenceGeneration}`. Encode these as JSON object fields with string values except
  integer `version`. Only exact parsed records qualify for owned operations. Record creation
  belongs in the same resourceVersion-conditional patch as the taint; no in-memory-only owner.
- Structured log message `failover outcome`, fields `node`, `nodeUID`, `operationID`,
  `evidenceRef`, `outcome`, `reason`. Outcomes: `candidate`, `evidence-unavailable`,
  `evidence-invalid`, `shutdown-verified`, `recovery-requested`, `cleanup-blocked`,
  `cleanup-completed`. Log no credentials or unfiltered provider response bodies.
- Existing CLI `failover --grace --interval` remains candidate timing only. Concrete `--failover-authority-config`, proof/config/crypto bounds, signer CLI and retirement
  procedure are specified in the appendix; unset authority
  starts in observe-only mode and logs `evidence-unavailable`. Unknown configuration is fatal.

## Files touched

- `control/failover/failover.go`, new `evidence.go`: candidate, proof and cleanup gates (S1–S9).
- `control/failover/kube.go`: UID/provider identity parsing, conditional operations (S3,S6,S7).
- `control/failover/failover_test.go`, new `evidence_test.go`: controller/API matrix (S1–S9).
- `control/cmd/flaminio-csi/main.go`, new `main_test.go`: safe startup/configuration (S2,S5,S9).
- `deploy/csi/driver.yaml`, `deploy/csi/rbac.yaml`: default/config comments and least
  privilege and the appendix's exact namespace-read permission (S2,S5,S9).
- `scripts/lab.sh`, `scripts/tools/failover-policy-test.sh` (new), `Makefile`: replace unsafe
  lab assumptions and register the script check in lint (S2–S9).
- `control/failover/manual_lockout.go`, tests and signer/config command wiring: appendix (S2–S4,S7,S9).

## Tests

| Rules | Concrete tests and level |
|---|---|
| S1 | `failover_test.go:TestPartitionNeverMarks` and updated `TestStep`: false/unknown Ready, missing heartbeat, deleted Node, very long Grace, another Ready node; zero mutation calls. |
| S2,S3 | `evidence_test.go:TestShutdownEvidenceRefusals`: absent provider, timeout/auth/error, unknown state, stale/revoked generation, wrong UID/provider/machine and proof invalidated between observation and patch. |
| S3,S4 | `failover_test.go:TestVerifiedShutdownRecovery`: valid held fence marks once; fake API/provider outages before mutation and lost PATCH response after success safely re-observe. |
| S5 | `failover_test.go:TestScopedRecoveryUnavailable`: opted-in, opted-out, other-driver and mixed-storage pod fixtures; fake API rejects every pod/attachment write; no mark without proof. |
| S6 | `failover_test.go:TestConditionalMarkOwnership`: httptest API race inserts foreign taint, replaces Node UID, changes owner annotation or returns 409; concurrent controllers preserve winner and unrelated taints. |
| S7 | `failover_test.go:TestCleanupRequiresReturnPermit`: Ready+heartbeat, legacy timestamp, foreign mark, restart, revoked permit, recreated machine and interrupted clear retain marks; matching permit clears only owned record. |
| S8 | Keep `flaminio-repl/src/sim/tests.rs` takeover, lease and reconciliation scenarios/mutations; controller tests assert recovery calls no data-plane admission shortcut. |
| S9 | `control/cmd/flaminio-csi/main_test.go:TestFailoverDefaultAuthority` and `evidence_test.go:TestOutcomeLogs`: default observe-only, unsupported configuration rejected, required outcomes and no secret fields. |
| S2–S9 | `scripts/tools/failover-policy-test.sh`: stub VM/API/authority commands; no unchecked power sample, timeout cleanup preserves foreign marks, restarts restored and unsupported provider fails before destructive action. |

The appendix adds real-signature/file/API and terminal-retirement cases for S2–S4/S7/S9.
Implementation checks: `make test race lint contracts sim hygiene`; no lab runs during drafting.
A new proof-based replication rule is not introduced; if implementation changes takeover rules,
it must add a simulator check and caught injected bug rather than relying on Go controller tests.

## Lab

The current VM `make csi-failover` must refuse manual-lockout-v1 before power or mutation
commands: VMs have no supported authority in this first profile. After separate authorization,
#49 can still test a live VM partition on mixed storage and require zero recovery mutations,
healing all rules afterwards. A separately authorized disposable physical-host qualification
uses the appendix's actual lockout, Kubernetes recovery and terminal cold-return procedure;
record data/workload/attachment checks and preserved disk/key inventory. Never unlock the old
installation in a cleanup trap, auto-delete foreign marks or format retained replica disks.
No lab success is claimed by this draft.

## Docs

Update architecture §11.2 and package comments to distinguish evidence-based node shutdown
from unavailable scoped recovery. Update `docs/lab.md`, `README.md`, follow-up status and
Makefile help for safe defaults and changed `csi-failover` prerequisites. Coordinate #43 operator
rollouts, #48 upgrade expectations and #49 scenarios. No performance-plan result is invented.

## Risks

- **Proposed first profile:** manual-lockout-v1 trusts an administrator's signed physical
  isolation assertion, with exact inventory/UID binding and the permanent held-lock lifecycle.
  It does not claim measured shutdown. Other providers remain unsupported.
  Settled at approval, 2026-11-13: manual-lockout-v1 is accepted (see decisions.md, Settled at approval).
- **Return trade-off:** terminal UID retirement, fresh NodeName/PKI and clean OS; preserve all
  replica disks/keys offline until #13/#14/#61 obligations are safely resolved. No automatic
  same-identity rejoin or Clear is supported. Human approval of this proposal remains absent.
  Settled at approval, 2026-11-13: the terminal-retirement return policy is accepted (see decisions.md, Settled at approval).
- **Scope:** scoped partition recovery remains unavailable. Default observe-only remains safe;
  verified shutdown is concretely supported only by the appendix's physically controlled profile.
- **Security:** node patch and fence authority credentials are powerful; unrelated workloads
  cannot mint evidence. Node administrators can already mutate nodes; annotation ownership
  protects concurrency and mistakes, not a compromised Kubernetes administrator.
- **Compatibility:** old marks survive upgrade for operator review; automatic recovery may
  stop until evidence integration. A fence must survive controller restart and API outages.

## ADRs

Consistent with ADR 0001/0002 (Go control plane, etcd metadata) and 0005's replica-confirmed
lease principle. ADR 0005's early-read wording conflicts with the stricter existing takeover
implementation; this issue preserves code and does not resolve that pre-existing contradiction.
Audit-branch ADR 0007 remains Proposed. [Local proposal](adr-proposal.md) records the
orchestration authority boundary only; it neither accepts nor numbers a global ADR.
Settled at approval, 2026-11-13: the local proposal is accepted with the plan; 0007 stays separate (see decisions.md, Settled at approval).

## Parallelizable?

No Rust work is planned. Go controller/API tests can proceed against these internal contracts;
manual authority/signature/config and API work join against the exact appendix contract;
physical qualification and approval remain separate from draft technical completeness.
Settled at approval, 2026-11-13: the plan is approved; physical qualification remains separate (see decisions.md, Settled at approval).
