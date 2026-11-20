# Issue #69: Read-only cluster view before the UI — design

[Specification](spec.md). Original design approved 2026-11-13; not implemented.
The R1 corrections linked below are **Proposed, unapproved**; historical approval is retained.
For the existing intent's identity/command slice only.

The facts it starts from:
- Go Identity currently has Node/Component/Admin; ParseID accepts node/admin URI shapes
  (`pkg/pki/pki.go:193–245`). Rust mirrors them in `flaminio-net/src/identity.rs`.
- Authorize grants three reads to any authenticated identity; ListTombstones is admin or own
  node agent (`meta/service/auth.go`). The latter requires a valid node ID (`service.go:584`).
- Metadata CLI prints individual RPC results; no combined view, metrics endpoint or CRD summary
  exists. `Volume.Replicas` gives desired RF while a move temporarily adds a replica.
- `GetVolume` needs #77 coherent reads. No existing method supplies a cluster-wide snapshot or
  tombstones for unknown/unregistered nodes. The collector must expose those limitations.

## Reader identity and authorization (S1, S6)

Propose URI `spiffe://<trust-domain>/flaminio/reader/<name>` with existing segment validation.
Add `Reader string` to Go Identity and a Reader variant to Rust Identity; formatting/parsing
must produce exactly one identity shape. Update CA issuance tools to recognize it, never by
trusting CSR SANs. Existing node/admin encodings remain byte-for-byte unchanged.
Settled at approval, 2026-11-13: this URI as proposed (see decisions.md, Settled at approval).

In Authorize, branch on Reader before the existing method switch: allow only ListNodes,
ListVolumes, GetVolume and ListTombstones; everything else denies. Retain node-scoped request
validation in ListTombstones and no broader node rights. Build a descriptor-driven denial test
so new methods are not accidentally granted. Replica session accept continues requiring
`node_with("io")`: a reader cannot present itself as a target, even with a copied token.
No token is signed for a reader. Update proto caller comments and regenerate/check contracts;
the original four-method policy is historical. The proposed #91-owned page-read allowlist
extension and #69-owned tombstone inspection mode are limited as described below; no on-disk
change is introduced by #69.

Server rollout precedes issuing reader credentials. An old server/parser may refuse the new
role, which is a safe explicit error; do not fall back to administrator credentials.

## Collection and derivation (S2–S5)

Add Go package `control/status`: `Collect(ctx, Client, Limits) (View,error)` and pure `Derive`.
The local [view contract](view-contract.md) is normative for strict enumeration, legacy/exact
identity adapters, coherent full/paged acquisition, nested schema1 fields and pure reductions.
#91 alone owns the additive strict ListVolumes service contract and Reader page allowlist;
#69 consumes them through a whoever-lands-second hook, without changing that dependency.
ListVolumes always sends require_complete=true and accepts complete=true; old-server default
false, damaged candidate headers and RPC errors fail visibly. It supplies no guard candidate
ID and adds no admission bracketing, history proof or eviction authority.

Collect nodes and strict volume IDs, then each coherent full/paged volume. A named read's
NotFound is an identified `omitted_volumes` entry; other errors discard the whole attempt.
Collect tombstones for every registered node using the [inspection contract](tombstone-inspection-contract.md),
including historical UUIDs, every bounded page and empty nonfinal pages. Never reconstruct
unobserved full Nodes/Pending sets or deduplicate by volume name alone.

Retain the approved maximum4096 nodes,4096 volume IDs,30s total and four in-flight calls;
add32768 application RPC invocations (including failed/page calls; retry details in the
security contract), so cursor churn cannot cause
unbounded work. Full RPC receive remains4MiB; identity pages512KiB; inspection pages32KiB.
The64MiB application-data/encoding cap is enforced before allocation by the status-local
[security contract](security-contract.md), including all live reservations and bounded output.
No stdout is published before successful collection, derivation and bounded encoding.
Original limits were settled2026-11-13; the concrete enforcement and attempt/page adapters
are Proposed corrections. No automatic cross-attempt restart or partial view is published.

Derive uses committed metadata redundancy separately from earned initial copies and observed
exact-store availability. Pending INITIAL blocks the availability hint even when planned ISR
is full. A live replacement UUID never inherits an earned old UUID's liveness. One pure
reducer defines chunk→volume aggregation, node observational totals and omission coverage;
#71 reuses that reducer's committed-ISR under-replicated meaning. None grants I/O authority.

## CLI and output contract (S2, S4–S7)

Extend the existing `flaminio-meta` client with `status [--json]`, retaining existing global
server/cert/key/ca options; add `--trust-domain` validation for the reader and `--timeout` capped
to the collector maximum. Hostname verification and the configured CA remain enabled; verify
the server identity/trust domain through the existing pki helpers before trusting the response.
Require reader client identity for this command; do not silently borrow an admin key.

The exact metadata-server peer configuration and before-response TLS predicate are in the
[security contract](security-contract.md); use its required `--metadata-node` and `--trust-domain`
configuration. Stock CA, hostname and ServerAuth checks remain enabled; no admin fallback.

JSON schema1 top-level and nested fields, canonical ordering, unavailable fields and both
human/JSON reductions are fixed in [view-contract.md](view-contract.md). The top-level
`snapshot:false` and `tombstone_coverage.scope="registered_nodes_only"` remain explicit.
Timestamps are UTC RFC3339Nano. Human output is derived from the same View and ends with
non-snapshot/coverage disclaimers. Successful exit is0, errors1 with stderr diagnostic and
empty stdout; untrusted identifiers are escaped, never shell fragments or terminal controls.

## Files touched

- `pkg/pki/{pki,pki_test}.go`, CA CLI: Reader URI, validation and issuance S1/S6.
- `dataplane/crates/flaminio-net/src/identity.rs`, session tests: parser agreement/refusal S1/S6.
- `meta/service/{auth,service}.go`, tests, `meta/kv`: allowlist/shared read hooks and bounded inspection S1/S4–S7.
- `api/proto`, generated `api/gen`, Rust `flaminio-metaclient`: additive inspection/shared ABI compatibility.
- `control/status/{status,status_test}.go`: bounded collector/pure derivation S2–S5.
- `meta/cmd/flaminio-meta/main.go`, tests: status subcommand and output S2/S4–S7.
- lab PKI/status scripts and fake-VM tests: explicit reader identity and no mutation S7.

## Contracts

Reader URI is the historically approved URI. Proposed normative local contracts are
[view-contract](view-contract.md), [tombstone inspection](tombstone-inspection-contract.md),
and [security](security-contract.md). #91 owns strict-list protobuf fields/service validation
and the sole Reader ReadIdentityVolumePage allowlist addition; #90 owns identity-v4 and its
page ABI. #69 owns only its consumers and additive read-only history-inspection mode.
Regenerate Go/Rust bindings with `make proto`, check `make contracts` and method authorization
when implemented. No new writer, on-disk/frame/CRD/web contract or token exposure. #71 may
reuse pure derivation, never reinterpret this non-snapshot CLI view as mutation authority.

## Tests

- S1: `TestReaderMethodAllowlist` across every generated method; real TLS calls deny create,
  attach, ISR change, move, drain, delete and confirm; unknown method/role refused.
- S1/S6: Go/Rust Reader URI and single-SAN vectors, wrong trust domain/CA, replica accept refusal;
  reader cannot gain node privileges by setting a request node field or attaching a token.
- S2–S5: the exact adapter/reducer and service-to-status fixtures in both local contracts cover
  empty legacy projections, replacement UUID, INITIAL, full/paged equivalence, strict damaged
  enumeration, all inspection pages and omission coverage; Derive performs no RPC or mutation.
- S4/S5: `TestCollectNotSnapshot`, concurrent deletion/recreation, registry-limited coverage,
  cap/deadline/cancel cases; oversized protobuf or JSON never yields partial success/stdout.
- S6/S7: old-server refusal, no admin fallback, output JSON escaping/control characters,
  CLI exact flags, credential mode checks and fake-script calls recording no mutations.
- Run `make lint test race contracts` and relevant Rust session/parser tests through Makefile;
  no new replication safety rule, so no new ISR mutation is introduced.

## Lab

Later explicit authorization only: issue a reader certificate with existing PKI workflow,
run the command before/after a data-node stop/return, verify timestamps/hints and deny every
mutation. Do not stop a node serving unrelated workloads. Restore node state and remove only
the temporary reader credentials. No lab command runs during drafting.

## Docs

Update CLI usage/Makefile help if a status target is added, docs/lab.md and #69 followups.
The existing intent's claims that no issue tracks observability/CLI/CR summaries are historical:
#71 now owns them. Dashboard/Kubernetes-user authorization remains #69's later owned stage,
not implemented by this command; the whole #69 issue stays open.
Settled 2026-11-13 (user's instruction): dashboard authorization and toolchain are decided by
#69's later dashboard stage, the toolchain in an ADR, none in this step (see decisions.md).

## Risks

Reader output exposes admin operational information; issue/distribute its credentials as admin
read access, not to tenants. The security contract enforces64MiB application credits before
typed allocation and bounded encoding; stock-library private heap is outside that accounting. Credential issuance and URI
extension require review.
Settled at approval, 2026-11-13: the URI extension and credential issuance as proposed (see
decisions.md, Settled at approval).
Registry-only enumeration cannot guarantee tombstone completeness. No liveness-derived label
can authorize a fence/repair, and no UI badge can strengthen the service's state guarantees.

## ADRs

Rely on ADR0001/0002 and current authorization/identity boundaries. The Reader URI proposal is
issue-local `adr-proposal.md`, not an accepted new identity rule. #77 consistency is a prerequisite
for the stronger per-volume response claim; this command still advertises no aggregate snapshot.
Settled at approval, 2026-11-13: the Reader URI ADR is accepted with the plan (see decisions.md,
Settled at approval).

## Parallelizable?

Yes after URI/allowlist/output contracts are agreed: Go identity/collector and Rust parser
refusal can be built independently and joined by shared vectors and real TLS integration tests.
Settled at approval, 2026-11-13: the URI, allowlist and output contracts as proposed (see
decisions.md, Settled at approval).
