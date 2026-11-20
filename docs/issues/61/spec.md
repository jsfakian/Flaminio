# Issue #61: Instance-store identity slice — composition specification

[intent](intent.md) · [design](design.md) · [decisions](decisions.md).

Historical approval applies to the source slice; this moved-reference revision is unapproved.

## Terms

- **Identity plan:** issue #90 and its `docs/issues/90/` intent/spec/design/appendices.
- **Original slice:** #61's approved disk-identity half of step1, before cloud recovery policy.
- **Acceptance evidence:** #90's actual model, adapter and authorized environment results.

## The invariant

There is one store-identity authority, in #90. This wrapper neither relaxes nor duplicates it.
Moving or reviewing documents cannot claim the original runtime/cloud outcome has shipped.

## Behaviour

**S1. Delegation.** All store identity, generation, membership, bootstrap, replacement, cleanup,
wire, migration and resource rules live in #90. Consumers use that contract rather than a local
#61 ReplicaRef, protocol version or fence rule.

**S2. Acceptance.** The original identity outcomes are checked through this mapping:

| Original #61 outcomes | Owning evidence in #90 |
|---|---|
| 1–4,7: record earned identity, reject aliases, full resync, retain fences, reformat | identity/core/protocol/store/metadata adapter checks and caught mutants |
| 5: honest unavailable/lost-store diagnostics | no-authoritative-source refusal and status checks |
| 6: executable safety proof | store-loss simulator scenarios and per-rule caught mutants |
| 8: live/detached replacement data proof | authorized adapter/environment tests; current VM clock limits remain explicit |
| 9: accurate documentation | implemented identity scope, migration and unsupported-case documentation |

Missing implementation or evidence remains missing; a clear draft cannot satisfy these runtime rows.

**S3. Dependencies.** The tracker links #90 from #61 and #45's later store-loss/recovery steps.
The current clear #45 plan is not expanded by this link. Remaining #61 cloud/claim/placement and
rebuild stages stay open; this slice promises no new platform implementation.

**S4. History.** Preserve #61's original approvals/move decision as history. Substantive #90
revisions and this wrapper require their own approval; no approval is transferred automatically.

## Tests

S1–S4: document/reference consistency and fresh composition review against #90's exact reviewed
plan/report. Runtime acceptance uses #90's named future checks once implemented; it is not rerun
or duplicated under this wrapper. No cloud, VM, disk or runtime test is authorized by drafting.

## Open questions

None for this composition-only slice. Later cloud/rebuild choices remain outside this slice.
