# Tenant authority and destructive-operation audit live in the metadata service

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when
  implemented. It binds this plan's implementation from the approval on, before it is numbered
  and moved.
- **Date:** 2026-11-12
- **Scope:** Issue-local; no ADR number or acceptance is assigned.
  Settled at approval, 2026-11-13: accepted with the plan; it stays issue-local until implemented
  (see decisions.md, Settled at approval).

## Context

[Issue #73](https://github.com/jsfakian/Flaminio/issues/73) requires policy beyond per-method
mTLS and an append-only, attributable audit of destructive operations (ARCHITECTURE §15.3).
Any certificate with an Admin name may create, delete, move and drain today
(`meta/service/auth.go:57,64–65`); several metadata instances share one store, and an older
binary rewrites volume headers through its own struct (`meta/service/service.go:539–541`).
Kubernetes admission does not cover direct metadata RPCs, and #43 and #50–#53 do not exist yet.
This step covers the service's half ([intent](intent.md), "This step").

## Decision proposed for review

Settled at approval, 2026-11-13: accepted as below (see decisions.md, Settled at approval); the
changes of review rounds 3 to 11 came after the approval.

- **Durable, one-way activation by records in the store.** Their presence switches every
  instance of this step from the Admin-name rule to roles; insecure instances refuse to start
  on an active store, and a record missing afterwards fails role-gated calls closed while
  failover keeps serving.
- **Roles bound to exact SPIFFE IDs in one record,** with the receiver key, read by every
  instance on each role-gated call, so instances cannot disagree. A `security-admin` writes it
  and cannot drop its own binding; the override, audit reading and security administration are
  each bound alone. Once active, Attach is decided from the certificate alone: the CSI
  controller's identity by name, and agents restarting themselves. Node, agent and target rules
  are unchanged; none of these reads a record.
- **Owner, policy and quota records apart from the volume header,** written in the claim and in
  the header's deletion, so a writer that does not know them cannot strip them and a charge
  lives exactly as long as its volume.
- **Destructive operations' and configuration changes' events hash-chained in the metadata
  store,** each committed in its mutation's transaction: one chain per namespace for its tenant
  deletions, budgeted by its policy within a fixed store-wide allocation and retired with its
  policy once pruned; one configuration chain no tenant writes, with a fixed overdraft for the
  security administrator's revocations and key changes; and 16 system chains for the rest. No
  tenant fills another's or an administrator's chain, and the total is bounded. Every event
  counts against its chain's budget, with one reserved slot per admitted deletion. Pruning needs
  an independent receiver's Ed25519 signature over the chain's hash, and records it, except a
  full configuration chain, which the security administrator may prune with a key it holds
  (Consequences). Refusals
  are logged at a bounded rate per caller and never stored.

## Options

- **Metadata authority (proposed):** covers every request path and concurrent quota, but
  creation depends on a policy record and deletion on audit space.
- **Webhook or ResourceQuota alone:** early feedback, but blind to direct RPCs; cannot meet the
  invariant alone. Left for the Kubernetes half.
- **A per-process policy flag:** no durable mode, so instances disagree and an older or
  misconfigured instance keeps the Admin-name rule (review R1-B1).
- **Per-instance role and key files:** they can disagree, and a digest check between them adds
  a mechanism the store's own record avoids (review R2-S2, R2-S3).
- **One shared audit budget:** one namespace's churn would refuse every other namespace's
  deletions and the administrator's writes (review R2-S1).
- **Synchronous SIEM gate:** couples deletion to SIEM availability; a store-held chain with
  receipt-gated pruning is preferred.
- **Durable refusal records:** any authenticated caller could fill the store (review R1-B2).

## Consequences

- An older binary cannot be stopped by the store after activation; it is unsupported, and what
  it leaves is reported (spec Q4). Settled 2026-11-13 (user's instruction): that risk is
  accepted as stated (see decisions.md).
  Settled at approval, 2026-11-13: unsupported, and reported (see decisions.md, Settled at
  approval).
- With no receiver, nothing is pruned, and the budgets bound the audited operations an active
  store can make (spec Q2). Their total is fixed, and must stay well below etcd's space quota,
  with compaction and defragmentation run.
- Role separation holds only between certificates the CA issues to different people (#16).
  Before activation every administrator name is trusted, as today.
- Once active, manual and system-volume attaches need the CSI controller's certificate, which
  also carries `csi-controller`: a least-privilege cost stated after approval. Settled
  2026-11-13 (user's instruction): accepted (see decisions.md).
- With a receiver key whose private half it holds, the security administrator alone can prune a full
  configuration chain, erasing configuration events no receiver has received; the prune event keeps
  that key's fingerprint. A residual stated after approval. Settled 2026-11-13 (user's instruction):
  accepted (see decisions.md).
- With the last key's private half lost after the overdraft is spent, no chain can be pruned and the
  configuration chain refuses every write, until an etcd-level write of the authorization record's
  `receiver_key` (runbook): a residual stated after approval, in round 8.
- etcd or root access can rewrite history no receiver has signed; the chain is not called
  immutable against that adversary.
- Accepted ADRs 0001, 0002, 0005 and 0006 are consistent: `kv.KV` is unchanged and no new
  dependency is added.

## Confirmation

The [design](design.md)'s tests: the role table by direct RPC on an inactive and an active
store; quota, protection and override interleavings on both backends; every deletion
transaction failed and retried; chain tampering; forged, stale and replayed prunes; a full
chain with Attach and ChangeIsr still committing; a refusal flood writing nothing. No runtime
work or verification is claimed by this draft.
