# Issue #61 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **Q1, enrollment evidence for clusters older than store identity** (`spec.md`, Open questions;
  `design.md`, Legacy activation; `adr-proposal.md`, Consequences and open choices): as proposed.
  Under a maintenance barrier, an administrator reviews the inventory (opened disk UUIDs, volumes,
  first epochs, placement, fences, authoritative data) and `enroll-store` binds only known,
  unchanged original stores to their exact existing memberships; ambiguous, missing or replaced
  legacy disks stay quarantined. A bare UUID observed today is not taken as proof of history.
- **Q2, the same-node replacement and full-resync protocol and its compatibility envelope**
  (`spec.md`, Open questions; `design.md`, Replacement and full resync and Files and contracts;
  `adr-proposal.md`, Consequences and open choices): as proposed. `ReplicaRef` and protocol v2
  (`flaminio-repl/2`) shared with #72's multi-disk plan as one identity authority, the proposed
  shared proto, `AuthorizeReplicaSession` and `ReplaceReplicaStore`, full-chunk resync before the
  new store joins, and a documented maintenance migration with no legacy mode after activation.
- **The ADR's decision** (`adr-proposal.md`, Decision proposed for review and Options;
  `design.md`, Lab, docs, risks and ADRs): accepted as proposed (earned store refs), with the
  plan.
- **Moving this root-cause slice to a dedicated issue** (`spec.md`, Open questions; `intent.md`,
  opening paragraphs): as the intent recommends. The slice belongs in an issue of its own, which
  #45's later steps and #61 depend on, and moves there unchanged; that tracker action is not
  taken here.


## Approved tracker move carried out — 2026-11-20

The previously approved move is now [issue #90](https://github.com/jsfakian/Flaminio/issues/90).
Its source provenance is this folder at `5696573a126c0abe26347d3c9700e557489613cf`; original
approvals/settlements above are preserved as history. #90 owns all technical identity contracts.
This folder remains only the original-slice composition/acceptance wrapper. No cloud
classification or other new platform stage is substituted for the approved slice.

This reference revision and all substantive #90 changes are **unapproved**. Original #61 R1
findings follow the moved authority and are not resolved merely by deleting technical text.
#61 waits for a full fresh #90 review/address loop to clear, then receives a fresh review of
its own composition diff. Implementation/evidence is separate; #61 stays open for later work.
Current #45 technical plans remain unchanged; its GitHub body links the later dependency.
