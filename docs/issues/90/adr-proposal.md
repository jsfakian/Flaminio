# Replica membership belongs to a store incarnation

- **Status:** Proposed for issue #90; unapproved. Historical #61 acceptance is provenance only.
- **Date:** 2026-11-20
- **Scope:** Issue-local proposal; no active/global ADR number or acceptance is assigned.

## Context

[Issue #90](https://github.com/jsfakian/Flaminio/issues/90) fixes a shared safety gap: an empty
replacement disk inherits a reusable node name's earned ISR/source/lease role despite losing
blocks and durable fences. Existing superblock UUID is ignored by membership and session
bootstrap; simulation crash retains the original store. The root cause is independent of cloud
classification/claiming, #61 policy and #45's later recovery stages. Source #61's approvals at
`5696573a126c0abe26347d3c9700e557489613cf` do not approve the corrected protocols/APIs here.

## Decision proposed

Bind earned membership and cleanup to ReplicaRef(node,UUID), owned by #90, and a service-owned
retained store lifetime. Registration observes replacements without rewriting membership; old
source/ack/lease/session state never transfers by node alias. Authorize every new remote/local
Hello completely against current metadata before mutations, then persist its fence before
readiness. An immutable grant checks each role/operation; new moves require a new Hello.
Ordinary accepted I/O/heartbeats remain metadata-independent and add no locks. Initial
materialization has consumable exact-ref pending authority: durable creation/fence precedes
metadata earning, and a new session must obtain MEMBER before any earned credit.

Replace only after existing ISR removal permits it. Explicit current-epoch/version replacement
conservatively charges old/new refs and reconstructs all current data/holes/live writes, every
committed snapshot stage and committed/pinned root/receipt where those profiles exist. Recheck
the complete frontier at join; no dirty-only/current-only shortcut or automatic no-source
promotion. Old refs remain retained until exact authorized cleanup, never confirmed by a new
same-node disk merely because its name matches.

Use one composed identity/profile codec, byte/transaction envelope, bounded grant protocol and
fair control queues defined in [design](design.md) and its appendices. Maintenance enrollment
uses administrator-reviewed continuity evidence, durable bounded cursors and atomic activation;
uncertain legacy provenance becomes explicit unavailable quarantine. Proven FE0 ordinary/snapshot
volumes use explicit signed legacy-incarnation profiles preserving bytes/fences/history, without
a guessed positive epoch or destructive cleanup. Converted-item corrections invalidate verification
and require full rescan. No node-only authority fallback remains after activation. #90 supports one opened store/node and adds no on-disk format; #51/#58 own
history/root formats. Same-UUID rollback/cloning and malicious hardware remain outside scope.

## Options

- **Earned store refs (proposed):** express actual durable incarnation; requires new codec,
  bounded control authorization, retained cleanup/lifetime state and maintenance migration.
- **Node generation alone:** cannot establish disk continuity unless durably bound to the
  opened store; name/address/liveness is not that evidence.
- **Trust empty replacement and resync dirty regions:** loses untouched acknowledged data and
  fences, so cannot satisfy the invariant.

## Consequences

New bootstrap depends on available linearizable metadata; existing accepted sessions keep the
normal lease/fence model. Proven migration requires downtime and administrator evidence;
unknown provenance stays unavailable. Retained old cleanup charges can block capacity until
possession/deletion is proved. Minimum ISR or no authoritative source can block recovery;
no availability exception is accepted. #83 production physical clock admission means the
existing VM lab cannot qualify this feature; pre-#83 exploration remains explicitly limited.

Accepted ADR0001/0002/0004/0005/0006 remain consistent and are not superseded or edited. The
historical #61 issue-local acceptance is preserved in decisions.md; accepting a revised #90
design would be a distinct human decision. No global ADR index change is part of this draft.

## Confirmation

[Core proof](core-design.md) maps each new safety rule to an independent simulator assertion,
a caught named mutation and a bounded clean case; actual remote/local adapters verify the same
machine, delayed callbacks, fences and fast reads. Both-backend migration/grant/cleanup/capacity
races and Go/Rust wire/budget vectors prove contracts. Physical qualification is separately
authorized, never invented; no runtime/lab work or new approval is claimed by this proposal.
