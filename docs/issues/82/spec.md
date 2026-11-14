# Issue #82: Evidence-based node failover — specification

Approved 2026-11-13; not implemented. [Intent](intent.md). Settled choices, if recorded: `decisions.md`.
The later design is `design.md`.

## Terms

- **Candidate:** a node whose observed readiness/liveness justifies investigating recovery.
- **Node incarnation:** the Kubernetes Node UID and the machine/provider identity needed to
  distinguish a recreated Node, replaced instance, or restarted machine from earlier evidence.
- **Shutdown evidence:** an authenticated observation or completed fencing operation whose
  contract establishes that this exact machine incarnation cannot run its old workloads.
- **Node-wide recovery:** adding the out-of-service taint which can force-delete pods and
  detach volumes across all storage drivers on that node.
- **Scoped recovery:** a separately enabled recovery path acting only on selected, explicitly
  opted-in Flaminio workloads, without asserting node shutdown.
- **Owned mark:** an out-of-service action attributed to a specific controller operation and
  incarnation, distinguishable from marks installed by an administrator or other controller.

## The invariant

A live partition alone never authorizes node-wide force detach. Every node-wide recovery
mutation must have applicable shutdown evidence and target the incarnation that evidence
names. Flaminio's epochs and leases remain the authority for its block I/O; neither those
mechanisms nor orchestration marks establish that another driver's storage or a process's
external side effects have been fenced.

## Behaviour

**S1. Candidate detection is not authority.** NotReady/Unknown, missing heartbeat, elapsed
Grace, a deleted Node object, another Ready node, or any combination may select candidates.
None alone or together satisfies shutdown evidence, regardless of elapsed time.

**S2. Evidence gates node-wide recovery.** Before every node-wide recovery attempt, the
controller obtains shutdown evidence from an explicitly configured trusted authority and
validates its machine/incarnation binding and validity contract. With no configured authority,
an unsupported provider, evidence error, unknown power state, stale result or identity mismatch,
it refuses the node-wide action and reports why. The safe default never falls back to S1.

**S3. Evidence survives neither ambiguity nor replacement.** Cached results cannot authorize
an action after the observed Node/machine incarnation changed or the evidence validity ended.
Evidence must be applicable at the conditional mutation, not merely when a candidate was first
listed. An unknown outcome is re-observed before retry; the controller never invents success.

**S4. Successful shutdown remains recoverable.** Valid applicable evidence allows recovery
under the configured policy. Failures to reach Kubernetes or the evidence authority preserve
existing storage safety and result in retryable diagnostics; they do not force-detach on a
weaker signal. Kubernetes API outage does not imply a successful eviction or reschedule.

**S5. Scope is explicit.** A scoped partition-recovery feature is disabled unless explicitly
configured and opted into by the affected workloads. It may not install a node-wide shutdown
taint, evict opted-out/unrelated workloads, or remove their VolumeAttachments. Mixed-storage
pods are refused unless the selected design proves all affected attachments meet its contract.
If scoped recovery is not implemented, report it unavailable rather than silently using the
node-wide path. Document that opt-in does not fence arbitrary process side effects.

**S6. Marks have ownership and conditional updates.** Repeated steps and multiple controller
instances are idempotent. Mutations compare the Node incarnation and current resource version,
retain unrelated taints/annotations, and record which operation owns the change. A changed
Node or conflicting mark is re-read and re-evaluated. The controller never adopts or removes
another actor's mark merely because its taint key/effect match.

**S7. Recovery cleanup is evidence-based.** Remove an owned mark only for its matching
incarnation after the defined recovery checks establish that returning workloads/attachments
are safe. Ready plus heartbeat alone does not erase foreign marks or transfer ownership.
Missing evidence, changed identity or interrupted cleanup leaves a visible recoverable state;
restart cannot turn those cases into an unconditional untaint.

**S8. Storage takeover is unchanged.** Every new Flaminio target still follows the implemented
fence/wait/reconcile gate and all-ISR durability rules. Shutdown evidence and taints never skip
that gate. No assertion is made about the safety of another driver's volume from these rules.

**S9. Operator-visible outcomes.** Status/logs distinguish candidate, evidence unavailable,
evidence invalid, shutdown verified, recovery requested, and cleanup blocked/completed.
Report node/incarnation and the nonsecret evidence/operation reference. Do not log credentials.
Document safe defaults, supported evidence providers, validity limits, scoped-recovery policy
and the procedure for manual marks already present during upgrade.

## Tests

- **S1–S3:** unit controller matrix with live partitions, expired heartbeat, arbitrarily long
  Grace, deleted/recreated Node, stale provider responses, replaced machine identity, provider
  timeout/auth failure and absent provider. None may request node-wide recovery.
- **S2–S4:** verified shutdown with matching identity permits recovery; loss of API/provider
  before mutation or after an unknown result retries safely. Use fake authorities and API.
- **S5:** opted-in/opted-out pods, other CSI drivers, mixed-storage pods and unsupported scoped
  mode. Assert no unrelated pod/attachment mutation and no shutdown taint in scoped mode.
- **S6–S7:** API conflict/restart/concurrent-controller cases preserve foreign taints, identity
  and operation ownership; stale cleanup cannot untaint a recreated or differently owned Node.
- **S8:** existing protocol simulator, fencing/lease and Rust/Go interoperability checks pass;
  integration tests never permit I/O solely because a mark or provider result exists.
- **S9:** logs/status and deployment defaults expose blocked evidence without secrets.
- Run `make test race lint contracts sim` as appropriate to changed layers. Under #49, later
  explicitly authorized lab tests distinguish actual power-off from a live network partition
  on a mixed-storage node and verify data. Drafting this plan runs no shared-lab commands.

## Open questions

- Proposed first provider/policy: [manual-lockout-v1](manual-lockout-contract.md), trusted signed
  physical lockout until terminal old-UID retirement; no automatic same-incarnation return.
  Fresh node name/PKI and clean OS are required, preserving replica disks/keys and outstanding
  #13/#14/#61 obligations. This is a concrete draft choice, not human acceptance.
  Settled at approval, 2026-11-13: manual-lockout-v1 is accepted as the first provider and return policy (see decisions.md, Settled at approval).
- Scoped partition recovery remains unavailable; no implicit opt-in is introduced.
  Settled at approval, 2026-11-13: scoped recovery is not included and stays unavailable (see decisions.md, Settled at approval).
- Proposed ADR 0007 records related orchestration limits. It is not accepted; any actual
  contradiction with an accepted ADR requires explicit supersession through the ADR process.
  Settled at approval, 2026-11-13: ADR 0007 stays context, not authority, for #82 (see decisions.md, Settled at approval).
