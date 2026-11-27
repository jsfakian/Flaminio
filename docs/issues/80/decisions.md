# Issue #80 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`dbf5cf7cc8bc87afc614d3c5ffce11bea843c303e7573a393d7409721732f792`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Implementation status is recorded in [report.md](report.md).

## Settled at approval

- **Geometry and metadata-budget policy** ([spec](spec.md), Open questions, first item;
  [ADR proposal](adr-proposal.md), Decision and Options; [design](design.md), Admission and
  metadata bounds and Risks, Open policy): the Proposed policy. Whole chunks only, power-of-two
  chunk sizes from 1 MiB through 1 GiB, at most 65,536 chunks, and the conservative admission
  bound `2048 + N*(288 + 264*(R+1)) <= 4,194,304`, giving effective N caps of
  5137/3881/3119/2607/2239 for RF 1 to 5. It accepts refusing some formerly accepted creates and
  stored volumes. Partial chunks, pagination and higher limits are deferred to a separate design.
  The [ADR proposal](adr-proposal.md) is accepted with the plan.
- **CSI compatibility contract** ([contracts](contracts.md), the whole draft contract;
  [design](design.md), CSI selection and retries): accepted as written. The administrator-only,
  read-only `CheckCreateVolume` decides CSI retries, with no scalar `GetVolume` fallback.
- **Migration of pre-existing unsupported volumes** ([spec](spec.md), Open questions, second
  item; [design](design.md), Risks; [ADR proposal](adr-proposal.md), Consequences): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): no migration path in this
  plan; a separate design defines one if it is needed (see below). Until one is defined, such volumes are refused before
  any target or replica mutation, with no automatic resize, rewrite or deletion (S8).

## Settled at the user's instruction of 2026-11-13

- **Migration of pre-existing unsupported volumes:** this plan defines none. Such volumes stay
  refused before any target or replica mutation, with no automatic resize, rewrite or deletion
  (S8), and a migration path, if one is needed, is a separate design, as higher limits are.
  Recommended by Claude, no recommendation in the plan: the refusal already keeps such a volume's
  data unchanged, any automatic migration would rewrite or drop data the plan cannot prove safe,
  and deferring it weakens no rule.

## Choices

- Review80-R1-C1: metadata alone evaluates full non-capacity identity and CSI range compatibility
  via read-only CheckCreateVolume. No scalar Get override; direct #76 creation stays strict.
- Review80-R1-C2: #86 owns bounded conservative uncertainty/Fenced framing, a required dependency
  before #80's full transport guarantee. No uncertainty truncation or silent reply-limit increase.
- Review80-R1-C3: both administrative and CSI quantities use checked arithmetic before RPC.
- Review80-R1-C4: validate every later view before runtime map/core mutation; higher epoch still
  deposes using safe current geometry. Growth requires its separate authorized transition.

## Implementation sequencing — 2026-11-27

#86 / PR #103 and #77 / PR #104 are merged. Resume #80 on
`946b4ea9deb5c069e51e33d6598739f19bceabbc`, restoring its separately preserved geometry
helpers and Rust preflight groundwork. The approved design permits independent Go and Rust
implementation against the shared geometry/formula/RPC contracts; integration and acceptance
join them before review and publication.

## Implementation clarifications

- **Positional ISR records:** ChunkMeta has no wire index. Preserve the existing request-order
  contract from #77: validate saved request indices as unique/in-range and require the exact
  reply count before positional association. Do not add a wire field or claim an independent
  reply-index check the existing message cannot perform.
- **Shared checked encoder:** #86 already introduced encode_checked, EncodeError and the
  Conn::send preflight. Extend that single entry point for Hello while preserving its Fenced
  checks; no parallel encoder or larger frame limit is introduced.

- **Recovery geometry checks:** before block/index narrowing or core recovery, classify
  unsupported scalar V/C, inconsistent V/C across held chunks and indices outside N as
  unsupported read-only state. Preserve stored bytes/fences and the existing explicit delete
  path. This applies S4/S8 to startup as well as incoming Hello.
- **Historical canonical identity versus current admission:** validate stored canonical
  identity with #76's original grammar. A formerly legal but currently unsupported geometry
  remains a well-formed identity and returns geometry FailedPrecondition, rather than DataLoss.
  Missing/unknown/null required identity fields or partial new-format identity are DataLoss;
  current supported geometry is checked separately by shared Admit. Only SizeBytes is replaced
  for full-struct range-compatibility equality; every non-capacity field remains compared.

- **Maximum-view interop fixture:** a test-only loopback server under
  `meta/testdata/geometry-interop/` seeds admitted RF-cap future-move views and serves the
  production metadata service with real mTLS/authorization and normal limits. A shared
  fixture builder feeds protobuf-size tests and the server; `scripts/dev/meta-interop.sh`
  starts it alongside the existing ordinary fixture. This proves the actual Rust/Go receive
  boundary without production CLI flags, runtime test hooks or larger-message overrides.
- **Shared vector contracts:** Go TestSharedVectors and Rust geometry::tests::shared_vectors
  consume the same independent `meta/testdata/geometry-v1.json` expected results. The new
  `make geometry-vectors` target is part of `make contracts`; expected values are not
  regenerated from either helper under test.

- **Normal-read parse compatibility:** bounded creation decoding is stricter than #77's
  normal header reader. Preserve bounded outer SyntaxError/UnmarshalTypeError through the
  existing Unavailable mapping, excluding exact/nested create_spec type paths. Keep strict
  identity and oversize errors DataLoss. The corruption-after-initial-read Check race proves
  identity DataLoss on both backends; outer syntax/type known-commit/ISR matrices preserve
  their statuses and exact authority assertions.

## Code/security review round 1

- **R1-R-C1 — legal move cardinality:** an R-member planned move requires its destination
  absent, while the R+1 temporary stage requires it present. Apply that equivalence in the
  shared engine membership validator before initial/refresh/ISR adoption. Preserve legal
  planned, prejoin and joined stages and higher-epoch deposition using current safe geometry.
  This enforces existing CheckMove/validMove shapes at the input boundary; it changes no
  replication algorithm, wire schema or metadata transition rule. The regression fails
  before the predicate correction and passes afterward without relaxing mutation checks.
