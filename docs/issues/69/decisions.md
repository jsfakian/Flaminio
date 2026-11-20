# Issue #69 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The reader identity URI and API extension** (`spec.md`, Open questions; `design.md`, Reader
  identity and authorization, Risks and ADRs; `adr-proposal.md`, Decision proposed): as
  proposed. `spiffe://<trust-domain>/flaminio/reader/<name>`, parsed identically in Go and Rust;
  exactly ListNodes, ListVolumes, GetVolume and node-scoped ListTombstones allowed, every other
  and future method denied; replica sessions refuse it; reader credentials are issued only after
  server support, treated as admin read access, with no admin fallback.
- **The initial CLI and resource limits** (`spec.md`, Open questions; `design.md`, Collection
  and derivation, "Proposed limits", and CLI and output contract): as proposed.
  `flaminio-meta status [--json]` with the reader identity; 4096 registered nodes, 4096 volume
  IDs, 64 MiB of collected and encoded data, a 30 s total deadline and four RPCs in flight,
  refused with ResourceExhausted rather than truncated; the `schema_version` 1 JSON contract.
- **The ADR's decision** (`adr-proposal.md`, Decision proposed; `design.md`, ADRs): accepted as
  proposed, with the plan.
- **Dashboard authorization and toolchain** (`spec.md`, Open questions; `design.md`, Docs):
  no recommendation in the plan. Settled 2026-11-13 (user's instruction): decided by #69's later
  dashboard stage within this plan's rules, the toolchain in an ADR; none in this step (see
  Settled at the user's instruction of 2026-11-13, below).

## Settled at the user's instruction of 2026-11-13

- **Dashboard authorization and toolchain:** neither is chosen in this step, which serves no
  page. #69's later dashboard stage decides both in its own plan, after #43's operator can hold
  the reader identity, within this plan's rules: the page's server holds only the reader
  identity, never an administrator's; browsers authenticate through Kubernetes, as the intent
  states; and the view is admin read access, never given to tenants. The web toolchain is chosen
  in an ADR beside ADR 0001, which names only Go and Rust. Recommended by Claude, no
  recommendation in the plan: nothing in this step would use either choice, and a new toolchain
  needs an ADR.

## Proposed R1 corrections after approval (2026-11-20)

These choices address the fresh full review of3d53c0b61924923d51b9568ec6f1fa56a95e8518.
They are **Proposed, unapproved**, not retroactively covered by the approval/settled entries
above. No feature is implemented and no future fixture below is an executed result.

- **R1-C1 (High):** [view-contract](view-contract.md) consumes #90 identity-v4, exact
  incarnation/refs and coherent full/paged reads; committed ISR, planned INITIAL and earned
  copies are distinct. Availability uses current observed UUID/G, never node-name aliasing.
  #90 remains identity/page owner; #91 remains sole Reader page-allowlist owner.
- **R1-C2 (High):** [tombstone inspection](tombstone-inspection-contract.md) adds a bounded
  Reader-only/admin read mode over #90's existing node/UUID index, so registered nodes expose
  retained historical UUID rows. Exact row keys preserve incarnation/operation/ref; cursor
  completion, empty pages and unknown global totals are explicit. No possession/confirmation
  rights or unobserved full Nodes/Pending reconstruction; validated legacy observations stay separate.
- **R1-C3 (Medium):** status always requests/requires #91's strict complete ListVolumes.
  That issue alone owns the service ABI/validation; malformed headers and old-server default
  false fail visibly. #69 preserves allowed named NotFound omissions and snapshot:false,
  without guard bracketing, admission, candidate history or eviction rules.
- **R1-C4 (Medium):** the local [view contract](view-contract.md) fixes nested schema1,
  validated inputs, exact alive/dead/unknown thresholds, chunk→volume aggregation and
  node observed-count coverage. #71's committed-ISR under-replicated meaning is reused.
  Omitted IDs remain identified and node totals become observed lower bounds.
- **SEC69-R1-01 (Medium):** [security-contract](security-contract.md) requires configured
  --metadata-node and an exact node/<configured-node>/meta URI in VerifyConnection, alongside
  verified CA/hostname/ServerAuth and trust domain, before accepting a status RPC result.
- **SEC69-R1-02 (Medium):** that security contract fixes status-local preflight/charged
  construction, concurrent collection/derivation credits and bounded before-publication
  encoding. The64MiB cap is finite application/work accounting, not whole stock-library
  heap ownership; no gRPC fork, cgroup or guard restart lifecycle is imported.

Dependency snapshots read: #90 c9231b243dceb196a0eccb615055887c9879bb73 (technical-clear
331f337d3b847ff2d4faabc572f4ceae74837cd7), #91 3d9e0fe95c6f62bd3044842aeb612734ffd1e512
(technical-clear f6220b5eefbb0092979327465d2bd9d3aa58039d), #71
35ad29f01e447354df04cf6295b260c4c5cc460c and #77 bb8df5b057e1e26f6e73e1319c9afc49a3c72994.
Their contracts remain proposed and read-only here; the last implementation to land installs
the named local consumer hooks. No authority is forked or dependency reopened.

The appended follow-on proposal in [adr-proposal](adr-proposal.md) preserves the accepted
Reader URI decision and records the proposed shared read-only composition separately. The
whole #69 stays open for its later dashboard stage; no dashboard/browser/toolchain/CRD/metrics
or admission-guard work is included in these corrections.
