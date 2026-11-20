# 0016. Creation retains an immutable admitted plan and cooperative progress

- **Status:** Accepted
- **Date:** 2026-11-13
- **Issue:** [#76](https://github.com/jsfakian/Flaminio/issues/76)

## Context

Interrupted volume creates could not be resumed: placement lived only in the caller's memory,
and a crash after any successful claim left a hidden Creating header with no recoverable
operation or full create identity. Retries returned AlreadyExists while GetVolume returned
NotFound. etcd's transparent retry of an uncertain transaction can surface a committed claim as
a failed compare. Full-prefix discovery and Put-containing GC fail under inventory pressure and
etcd NOSPACE. See `docs/issues/76/{intent,spec,design,adr-proposal}.md`.

## Decision

- Persist a canonical create identity and a complete immutable placement plan in private
  `create-roots/` / `create-plans/` keys before admitting the name.
- Admit the sealed plan and Creating header atomically under name and tombstone absence.
  Without [#78](https://github.com/jsfakian/Flaminio/issues/78), omit the placement-admission gate
  and `admission_generation` field: creation does not serialize against drains.
- Progress (cleanup, copy, finalize) compares both the claim revision and the root revision.
  Identical callers and a periodic recovery worker cooperate without a lease.
- Finalization atomically makes the volume visible and sets `FirstEpoch` to the original claim
  revision. Headers retain `create_operation` and `create_spec` after visibility.
- Private GC is delete-only (root deletion fences, then orphan plans under root absence) so it
  remains executable under etcd NOSPACE. Sixteen permanent `create-birth/<shard>` keys prevent
  resurrecting a deleted operation id, including after a transparent backend retry.
- Under old-writer exclusion, a recognized Creating header at epoch 0 with an empty target may
  be CAS-removed without a new tombstone; unknown shapes keep conservative deletion.
- Discovery uses bounded keys-only `ScanKeys` and bounded `GetBounded` value reads.

## Options

- **Immutable staged plan (chosen):** straightforward replay; temporary duplicate chunk metadata.
- **Compact deterministic plan inputs:** needs a stable placement-replay contract the code lacks.
- **Rollback every interrupted operation:** delayed creators still need fencing; replanning can
  defeat earlier drain reservations.
- **Single-writer lease:** does not remove lost-response or immutable-plan fencing needs.

## Consequences

- Peak chunk metadata may approach twice the initial plan size per operation while staged.
- Old writers must be excluded (maintenance barrier); rolling mixed-version safety is not claimed.
- Legacy visible headers keep scalar-only identity matching as a documented exception.
- Delete-only GC does not clear etcd's NOSPACE alarm; operators still compact/defrag/disarm.
- Q1–Q3 from the specification are settled as in `docs/issues/76/decisions.md`.

## Confirmation

Deterministic schedules in `meta/testdata/create-recovery.json` and
`dataplane/crates/flaminio-repl/src/sim/create_recovery.rs` assert `NoRootResurrection` and
`NoUnprovedTombstoneBypass`, including `missing_birth_compare` and
`weakened_unpublished_classifier` counterexamples. Go service tests cover resume, deletion
classification and CSI AlreadyExists preservation.
