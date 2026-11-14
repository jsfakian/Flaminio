# Issue #73: Tenant policy and destructive-operation audit — intent

[Issue #73](https://github.com/jsfakian/Flaminio/issues/73), ARCHITECTURE §15.3.
Approved 2026-11-13; not implemented. It covers the issue's first step, the metadata service's
authority; see "This step".

## Why

1. **Authentication is not tenant authorization.** `Authorize` treats every certificate with an
   Admin name as an administrator for create/delete/move/drain (`meta/service/auth.go:57,64–65`).
   It has no namespace, device-class quota or deletion-protection check. Node identities have
   narrow per-method rules, which must be preserved.
2. **CSI supplies no trusted tenant identity.** Its CreateVolume sends geometry and placement
   policy only (`control/csi/controller.go:231–236`); DeleteVolume supplies only the volume name
   (`:261–269`). A namespace string added by a caller would not establish ownership.
3. **Kubernetes admission is not the last authority.** Direct authenticated metadata RPCs bypass
   it. CRDs/operator remain #43 planned work. Existing `deploy/csi/rbac.yaml` grants sidecars
   their operational rights, not a product admin/tenant policy layer.
4. **Current logs are not a complete durable audit.** `logConfirmation` logs identity, peer,
   serial and revision for a newly confirmed tombstone (`meta/service/service.go:675–694`).
   It is diagnostic slog, with no append-only store, retention or export recovery.
5. **Quota and lifecycle commits must agree.** A charge, an owner, a protection state and an
   audit event that are not written with the volume's own transactions can disagree with it
   after a crash, a lost reply or a concurrent request.

## This step

The issue lists five steps and depends on #43 and #50–#53, which have no code and, for #52 and
#53, no plan. Its last line allows the base to start first. This plan is that base: **the
metadata service's authority**, for the operations that exist today.

- **Issue step 3, whole:** deletion protection enforced in the service that deletes, and the
  explicitly authorized override.
- **Issue step 4, for today's destructive operations** (volume deletion, forced confirmation,
  policy, owner and pruning changes): append-only events, their persistence, reads for export,
  pruning only against a receiver's signature, failure behaviour, and what they must never hold.
- **Issue step 2, the service half:** RF=1 and unencrypted restrictions by namespace, quotas per
  device class and required protection, checked on every request path and safe under
  concurrent requests. The CSI controller supplies the verified owner.
- **Issue step 5, the service half:** the role and policy matrix by direct RPC, refusals included.

What stays open, and keeps the issue open after this step:

- **Issue step 1:** the Kubernetes RBAC split and the admin resources. They need #43's CRDs.
- **Issue step 2, the Kubernetes half:** the admission webhook or ValidatingAdmissionPolicy and a
  policy resource. They need #43's installer, and a Kubernetes client and test fixture this
  repository does not have. The service's check stays the authority when they arrive.
- **Key operations, snapshots and backups:** their audit events, protection checks and quota
  belong to #50–#53, which add those operations; growth is charged with #70's growth
  step (spec S4).
- **A receiver that signs receipts, an exporter daemon and a SIEM forwarder:** this step ships
  audit reads, receipt-checked pruning and their CLI.
- **Durable records of refused attempts:** a refusal is logged here, not stored. Storing it
  needs a bounded design that no refused caller can use to fill the store.
- **Activating the lab cluster,** and the CSI manifest changes activation needs.
- **A certificate of its own for the failover controller,** bound to no role: today it mounts the
  CSI controller's (`deploy/csi/driver.yaml:208,220`).
- **Tenant QoS ceilings:** not provided by this step, nor planned in it; #75 cannot rely on
  them.

From #72 this step assumes only that a device class is a pool name `domain.ValidID` accepts and
that a creation request will carry one; until it does, every volume's class is `default`.

Against "Done when": after this step, forbidden namespace combinations, over-quota requests and
protected deletions are refused under concurrent requests on every path to the metadata service;
permitted PVC workflows work once tenancy is activated; volume deletions have durable,
attributable records; an export interruption loses no record and changes no authorization.
Early refusal at admission, the RBAC split and key-operation records are not yet there.

## What we want

1. An explicit role for each caller of the metadata service, replacing the Admin name, with the
   override and audit reading separately credentialed.
2. Owners bound to namespace and PVC UIDs that the CSI controller read from Kubernetes; forged
   owners and direct-RPC bypasses refused. Protected volumes stay protected through namespace
   deletion, failed provisioning and every API path.
3. Destructive operations recorded durably with their mutation; records read for export and
   pruned only once a receiver has signed for them (a full configuration chain excepted:
   contracts, "Self-prune"), and without secrets. A refusal costs the
   store nothing.
4. Allowed and forbidden paths verified on both metadata backends.

## Out of scope

- Replacing tenant PVC/snapshot APIs, the replication capability/ISR rules, encryption/key
  algorithms, snapshot/backup mechanics, or #72 physical capacity admission.
- Protecting records from a malicious etcd/root administrator without an independent export
  authority; proposing policy cannot create such a guarantee. No shared-lab commands now.
