# Issue #71 plan review — round 1, security focus

Scope: `docs/issues/71/` (intent, spec, design, contracts, adr-proposal), checked against the
code in `/tmp/flaminio-plans-2026-11-12/71` and the dependency drafts it relies on (#69, #54,
#74, #77, #83). The cited code facts (demo exporter, store/target Stats, fast-read paths,
`logCalls`, the control socket) are correct.

### R1-S1 — High
**Conditional preconditions added as fields to existing metadata RPCs fail open.**
The design closes the stale-plan race only by server-side preconditions, added as new request
fields on existing methods (design.md:108–112, contracts.md:159–161). A metadata server that
does not know a proto3 field ignores it and runs the mutation unconditionally. The lab runs
three metadata instances, so a rolling upgrade or a newer CLI gives exactly that mix.
- `DeleteVolumeRequest` is `{id}` only (`api/proto/flaminio/meta/v1/meta.proto:163–165`); there
  is no capability RPC, so "no owner capability -> exits unsupported before mutation"
  (contracts.md:161) has no mechanism, and a capability read answered by one instance says
  nothing about the instance that serves the delete.
- A zero-valued precondition is indistinguishable from an absent one: legacy volumes have
  `FirstEpoch == 0` (`meta/domain/domain.go:44–47`).
- Result: `volume delete ID --plan --confirm` with a stale plan deletes a recreated volume of
  the same id, the case S8 exists to prevent.

Fix: carry preconditions on new methods (for example `DeleteVolumeIf`, `MoveReplicaIf`), which
an old server answers `Unimplemented`, each with its `auth.go` rule and the descriptor-driven
denial test; give the precondition explicit presence. Add an old-server fixture test that
asserts zero mutations.

### R1-S2 — High
**The node observer is not least-privilege, and the contract misstates what its certificate
allows.** This contradicts S5 ("node producers have separate least-privilege roles").
- It reads the root control socket (design.md:56–58). That socket also serves `attach`,
  `detach` and `delete`, and `delete` removes this node's replicas of any non-attached volume
  with no tombstone check (`dataplane/crates/flaminio-io/src/control.rs:101–102, 172–189`;
  `dataplane/crates/flaminio-node/src/engine.rs:696–703`).
- contracts.md:127 says "Its agent certificate authorizes only its telemetry scope". An
  `agent` identity may call RegisterNode, NodeHeartbeat, Attach (only-if-target),
  ListTombstones and ConfirmDeletion for its node (`meta/service/auth.go:58–78`).
- The reduction is deferred to #74 (design.md:59), but #74's draft keeps the existing control
  socket and does not mention the observer (`74/docs/issues/74/design.md:167`).
- The observer is new attack surface: it parses SMART and host data and opens network
  connections, so its compromise yields replica deletion and deletion confirmation.

Fix: serve `observe` (and #83's `clock-status`) on a separate read-only socket with its own
mode and group, which accepts no other command. Issue a distinct `node/<n>/observer` identity
that the collector admits for Push and that has no metadata mutation rule. Test that this
credential is denied every metadata mutation and that the socket refuses every other command.

### R1-S3 — Medium
**What a node may assert inside its own Push is not scoped or fairly budgeted.** "Node agent
may Push only its own node evidence" (contracts.md:73) binds `node_id`, not the entities.
- Disks: store UUIDs are not registered in metadata (`Node` has no disk field,
  `meta.proto:63–74`), so S2's "limited to registered operational entities" cannot be checked.
  A node can claim another node's store UUID; with `source_node?` optional (contracts.md:92)
  and "foreign owners block publication" (contracts.md:94), the first claimant wins.
- Volumes: a node can report I/O series for every volume id. At 96 series per pair
  (contracts.md:42), about 1,050 claimed volumes exhaust the 100,000 global budget
  (contracts.md:45). The whole view becomes incomplete and the health alerts, which require
  fresh and complete data, are suppressed cluster-wide. A daemon serves one volume at a time
  (`control.rs:95`), so an honest node needs one pair.
- `recovery_*` gauges are labelled `volume,kind` only (contracts.md:30–32). The authoritative
  producer is undefined, so any node can report `remaining=0` or `eligible=0` for a volume and
  suppress RecoveryStalled.

Fix: key Disk identity by (authenticated node, store UUID) with a per-node disk cap; admit a
(node, volume) sample only when metadata names that node as target or replica holder; define
the producer of each volume-level family; apply a per-producer series quota so overflow drops
only the offender. Add a test for each of the three abuses.

### R1-S4 — Medium
**Replay protection across producer restarts has no mechanism.** "New source instance resets
counters/freshness; old sessions/sequences cannot overwrite it" (contracts.md:70–71,
design.md:70), but `instance_id` is opaque hex with no order (contracts.md:54).
- A delayed retry from the previous instance, or the old pod during a rollout, looks like a
  newer instance: counters reset and freshness is restored from stale data.
- It is unsaid whether Push's `instance_id` is the daemon's or the observer's, and whether
  Push is unary or a stream ("stream/session bound", contracts.md:69).

Fix: have the collector remember a bounded set of superseded instance ids per producer
identity and refuse them, or assign an ordered generation on first contact. State which
process's instance is pushed and the RPC shape. Test: an old-instance Push after a
new-instance Push is refused and counted.

### R1-S5 — Medium
**The Events RBAC cannot work for cluster-scoped summaries without widening the reader.**
The summaries are cluster-scoped (design.md:67); Events are published and read only in
`flaminio-observability` (contracts.md:121–125).
- Kubernetes API validation requires an Event whose involved object has no namespace to be in
  `default` (or `kube-system` through `events.k8s.io`). This comes from the upstream validation
  rules; I could not confirm it in this repository, which does not vendor client-go.
- If so, the publisher cannot create these Events, and the fix by RBAC alone gives readers and
  the publisher Events access in `default`, beyond S5.

Fix: make the three kinds namespaced in `flaminio-observability` (reader and publisher become
Roles), or record Events regarding a namespaced anchor with the summary as `related`. Add an
envtest case for Event creation and for the reader's read of it.

### R1-S6 — Medium
**Destructive non-recovery verbs have no plan generator, so `--confirm` binds nothing.**
`disk claim`, `disk release` and `volume delete` take `--plan FILE --confirm SHA256`
(contracts.md:149–151), but only `recover plan` writes a plan or prints its digest
(contracts.md:152). The operator would write the file and hash it, which is not S8's "exact
confirmation of that plan" built from fresh revisions.

Fix: add owner-served plan commands for these verbs (for example `volume delete ID --plan-out`,
`disk plan`), or drop `--plan/--confirm` from them and rely on R1-S1's server preconditions.
Test that apply refuses a plan the owner did not issue.

### R1-S7 — Medium
**"Wrong cluster fails closed" has no cluster identity to compare.** Plans "bind cluster"
(design.md:107, contracts.md:174) and the S8 test names wrong-cluster refusal.
- The metadata service has no cluster identity; the only one in the code is the superblock's
  `cluster_uuid` (`dataplane/crates/flaminio-format/src/superblock.rs:28`). #54's
  `cluster_origin_uuid` is a proposal.
- The summary spec `{source_id,source_incarnation,source_node?}` and the JSON envelope carry
  none, while reads use the kubeconfig context and mutations use separately supplied metadata
  endpoints: the two can be different clusters holding the same volume id.

Fix: name the identity's source and owner (depend on #54's enrolment or define it here), put
it in the summary spec, the envelope and the server precondition, and test two clusters with
an identical volume id.

### R1-S8 — Medium
**The span export path is unspecified, so its trust boundary is undefined.** Spans sit in a
4096-record ring per producer and "OTLP uses mTLS" with a pinned peer (design.md:40–45, 64).
The `observe` schema and the Push message carry no span field (contracts.md:54–65), and the
design does not say which process exports (the Rust daemon or the observer), under which
identity, or to which endpoint (collector port 9444 or an external OTLP collector). The S3
tests cannot be written against that.

Fix: state the path, for example a bounded span array in the `observe` snapshot and a Push
field admitted under the own-node rule, with the collector as the only OTLP exporter to one
configured pinned endpoint.

### R1-S9 — Medium
**"Multiple collectors are safe idempotent publishers" (design.md:72) does not hold for
freshness.** Freshness is each collector's own receipt age (design.md:77–78), and a producer
pushes to one replica.
- The replica that receives nothing publishes Stale against the other's Fresh.
- If patches are ordered by source sequence instead, then when the receiving replica dies with
  the producer, the survivor holds an older sequence and can never mark the summary Stale,
  which S1 forbids ("stale evidence cannot report healthy live state").
- `/metrics` also differs per replica.

Fix: one active publisher chosen by a Kubernetes Lease (it elects a publisher, it fences no
storage), or have producers push to every collector. Add a two-collector test.

### R1-S10 — Medium
**Reader rights over recovery status contradict across three drafts, and the recover grammar
does not match #54.**
- contracts.md:153 gives `recover status` "reader-authorized progress"; design.md:156 says "Do
  not add recovery methods/rights to metadata Reader"; #54 grants `GetRestore` to "explicit
  Reader/Admin" (`flaminio-54/docs/issues/54/contracts.md`, "Proposed admin RPCs"); #69 S1
  allows the reader exactly four methods.
- #54 has no plan/apply/digest model (`BeginRestore` … `CompleteRestore` by operation id), and
  its restore runs from `flaminio-backup` with repository credentials and a node control
  socket, which a kubectl plugin should not hold. `--kind metadata|force` are absent in #54.

Fix: decide with #54 and #69 whether the reader allowlist gains one status method, and record
it in all three. Make the adapter mirror #54's begin/status/cancel by operation id, or have
#54 adopt plan/apply. Remove the unowned kinds from the grammar until an owner exists.

### R1-S11 — Low
**Deletion "only for matching retired summaries" (design.md:87–88) is not expressible in
RBAC.** contracts.md:123 grants delete on all summaries. "Foreign owners" (contracts.md:94) is
also undefined, since there is no ownerReference or publisher label.

Fix: describe the restriction as the publisher's UID/RV precondition, not a grant, and name
the field or label that marks a summary as the publisher's.

### R1-S12 — Low
**The scrape credential is over-broad and its admission rule is vague.** "Metric GET permits
explicit Reader/Admin" (contracts.md:75): a Prometheus scrape key is then a full metadata
reader, and "explicit" does not say whether any reader identity or a configured list of names
is admitted.

Fix: admit a configured allowlist of exact identities for `/metrics`, and test that an
unlisted reader is denied.

## Nits
- spec.md:3 links `decisions.md`, which does not exist in the folder.
- contracts.md:74 lets the `node/<n>/meta` identity Push, but the lab issues that certificate
  for server use only (`scripts/lab.sh:583`); say that a client-usage certificate is issued.

Reviewed commit: 76cf82f7ae03bab66a9410264470a6f3991715de
