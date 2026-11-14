# Issue #76: Interrupted volume creation — intent

[Issue #76](https://github.com/jsfakian/Flaminio/issues/76), audit finding F01.
Approved 2026-11-13; not implemented.

## Why

1. **The durable claim cannot be resumed.** `createVolume` first writes a header with
   `Creating=true`, publishes chunks in separate transactions, then makes the header visible
   (`meta/service/service.go:409–473`). Placement exists only in that caller's memory. A crash
   after any successful transaction leaves no recoverable operation/spec identity.
2. **Retries cannot repair it.** A taken name returns AlreadyExists (`:420–433`), while
   `readHeader` and `ListVolumes` hide the creating header (`:275–289`, `:848–862`). CSI handles
   AlreadyExists by GetVolume, which returns NotFound (`control/csi/controller.go:252–261`).
   Audit `TestAuditInterruptedCreateRetry` reproduces the permanent provisioning failure;
   existing `TestCreateAfterAnInterruptedCreationStartsAboveItsClaim` tests deletion instead.
3. **A failed response is not a failed commit.** The etcd backend retries an uncertain transaction
   internally (`meta/kv/etcd.go:15–49`, `:109–140`). A committed claim can therefore surface as
   a failed compare. Cleanup or a fresh claim based only on that response would be unsafe.
4. **Recovery must retain admission and fencing.** Batches compare the original claim revision;
   deleting marks invalidate it before listing replicas. #78 adds placement admission generations:
   resumption must retain the admitted plan and generation, including across drain marking.
5. **Recovery must remain executable under pressure.** Full-prefix discovery allocates before
   any worker budget. Pinned etcd 3.7.3 rejects every Put-containing transaction under NOSPACE,
   including mixed delete/progress updates. Private reclamation needs bounded keys-only discovery
   and delete-only fencing/cleanup; an admitted creator is not abandoned to make space.

## What we want

1. Persist canonical create identity and the entire immutable placement before admitting a claim.
   Identical requests and recovery workers finish the same operation after any crash or lost reply.
2. Publish through bounded, conditional progress transitions; conflicting requests cannot change
   the plan, and delayed helpers cannot damage a completed/deleted/recreated volume.
3. Reconcile abandoned work without a client returning. Keep incomplete creations unattachable;
   preserve tombstones and revision-based first epochs. Migrate old hidden claims through a
   separately fenced rollback, since their original placement/spec identity is unavailable.
   For recognized never-published claims, no Attach/token was authorized: use that proof to avoid
   unnecessary replica inventory/new tombstones. Do not infer the proof for unknown shapes.
4. Prove the protocol on memory and etcd, with explicit transaction failure schedules and a
   deterministic recovery model. No lab run is needed to establish this metadata defect.

## Out of scope

- Capacity reservations (#72), placement admission/drain serialization (#78), read-snapshot
  consistency (#77), disk cleanup redesign, metadata disaster recovery and volume resizing.
- New CSI parameters, a caller-supplied operation token, mixed-version writer safety, or changes
  to replication durability, capabilities, wire frames or the on-disk format.
