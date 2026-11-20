# Issue #69: Read-only cluster view before the UI — design

[Specification](spec.md). Approved 2026-11-13; not implemented.
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
there are no proto field/method additions or on-disk changes.

Server rollout precedes issuing reader credentials. An old server/parser may refuse the new
role, which is a safe explicit error; do not fall back to administrator credentials.

## Collection and derivation (S2–S5)

Add Go package `control/status`: `Collect(ctx, Client, Limits) (View,error)` and pure `Derive`.
Client exposes only the four read methods, not the mutation-capable MetadataService interface.
Collect nodes and volume IDs, then GetVolume each and ListTombstones for each registered node.
Deduplicate tombstones by volume, preserving their recorded Nodes/Pending sets and count.
A NotFound volume is included in `omitted_volumes`; all other RPC errors fail the view. Use a
30 s total context deadline and at most four in-flight RPCs. No unbounded goroutine fan-out.

Proposed limits: 4096 registered nodes, 4096 volume IDs, 64 MiB collected/encoded response data
and existing per-RPC receive bound; reject over-limit results with ResourceExhausted rather
than truncating them. Validate protobuf lengths/counters with checked arithmetic. These are
bounded initial CLI limits, not claims of supported cluster scale. Exact constants are tested.
No successful output is printed until collection/derivation/encoding has finished within bounds.
Settled at approval, 2026-11-13: these limits as proposed (see decisions.md, Settled at approval).

Derive redundancy for each chunk from len(ISR) versus volume.Replicas. Reject contradictory
geometry/sets/counts as invalid data; do not silently invent unavailable/healthy states. A
moving chunk with RF current ISR members is not degraded solely because Replicas has RF+1.
Node holdings count chunk membership; served volumes derive from target names. A separate
`availability_hint` compares observed alive ISR members with MinIsr and marks unknown nodes
explicitly. Every displayed hint is observational and never an input to mutation decisions.

## CLI and output contract (S2, S4–S7)

Extend the existing `flaminio-meta` client with `status [--json]`, retaining existing global
server/cert/key/ca options; add `--trust-domain` validation for the reader and `--timeout` capped
to the collector maximum. Hostname verification and the configured CA remain enabled; verify
the server identity/trust domain through the existing pki helpers before trusting the response.
Require reader client identity for this command; do not silently borrow an admin key.

JSON top level: `schema_version:1`, `started_at`, `finished_at`, `snapshot:false`, `nodes`,
`volumes`, `tombstones`, `omitted_volumes`, `unsupported_fields`, `tombstone_coverage`.
The latter is `"registered_nodes_only"`, never `"complete"`; unsupported_fields includes disks,
physical_capacity and rebuild_progress until their producers exist. Timestamp strings are UTC
RFC3339Nano; order node/volume IDs and chunk indices deterministically. Reuse proto geometry
as decimal integer JSON fields; preserve explicit unknown availability rather than zero values.
Human output is derived from the same View and ends with the non-snapshot/coverage disclaimer.
Successful command exit is 0, errors 1 with stderr diagnostic and no stdout JSON. Escape all
untrusted identifiers; output is never a shell fragment or ANSI control sequence.

## Files touched

- `pkg/pki/{pki,pki_test}.go`, CA CLI: Reader URI, validation and issuance S1/S6.
- `dataplane/crates/flaminio-net/src/identity.rs`, session tests: parser agreement/refusal S1/S6.
- `meta/service/auth.go`, auth tests and proto caller comments: explicit allowlist S1/S6/S7.
- `control/status/{status,status_test}.go`: bounded collector/pure derivation S2–S5.
- `meta/cmd/flaminio-meta/main.go`, tests: status subcommand and output S2/S4–S7.
- lab PKI/status scripts and fake-VM tests: explicit reader identity and no mutation S7.

## Contracts

Reader URI and four-method allowlist above; existing node-scoped ListTombstones request shape
is unchanged. New public command/output/limits are fixed above. Collect never exports a token
or private key. No new metadata method, disk/frame/CRD/web contract. #71 may reuse `control/status`
with a separate bounded summary API later, not reinterpret a non-snapshot CLI view as authority.

## Tests

- S1: `TestReaderMethodAllowlist` across every generated method; real TLS calls deny create,
  attach, ISR change, move, drain, delete and confirm; unknown method/role refused.
- S1/S6: Go/Rust Reader URI and single-SAN vectors, wrong trust domain/CA, replica accept refusal;
  reader cannot gain node privileges by setting a request node field or attaching a token.
- S2/S3: `TestDeriveMoveIsNotDegraded`, mixed chunk health, invalid ISR, unknown node, holdings,
  target counts; no RPCs or mutations from Derive.
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
read access, not to tenants. A 64 MiB cap needs pre-allocation/protobuf receive bounds, not only
a check after an unbounded marshal. Credential issuance and URI extension require review.
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
