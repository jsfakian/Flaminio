# Issue #77 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`e560e6e7ad72ca0c06d99647fad4ae074a06f36e0f238fd095f8ba42cdabf410`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Implementation status is recorded in [report.md](report.md).

## Settled at approval

- None to settle: the [spec](spec.md)'s Open questions section records none for the
  revision-validation approach. Its note that every backend's List must stay one atomic prefix
  snapshot is a requirement the [design](design.md) adopts (Contracts), not a question.

## Choices

- Review77-R1-C1: use a bounded internal atomic ReadSet for header+named ISR chunks rather than
  separately acquired refusal views. Successful conditional-write replies linearize at CAS.
- Review77-R1-C2: pinned missing/hidden/superseded header is committed-superseded Aborted;
  other postcommit errors preserve their code and include exact known-commit context.
- Review77-R1-C3: concurrent Attach tests count actual backend-applied header increments,
  not successful API returns, because a committed Attach may have an unavailable response.
- Review77-R2-C1: ReadSet retains repeated keys/response positions; duplicate-index refusal
  remains CheckIsrChange policy after coherent acquisition, preserving existing RPC behavior.

## Implementation sequencing — 2026-11-27

#80's accepted compatibility contract explicitly uses #77's coherent read. The prerequisite
was absent on main after #86 merged, so implement and merge #77 independently before #80.
Keep the two issues' runtime changes separate; this changes sequencing, not either contract.

## Implementation clarification — internal KV receive limits

The design's phrase "bounded response under the existing metadata receive cap" conflated
the external metadata RPC budget with the internal etcd client. Source inspection during
implementation found that this KV client's receive setting is unset and the installed etcd
client defaults to MaxInt32. ReadSet caps requested positions at 65 and inherits that existing
client limit; #77 does not introduce a 4 MiB internal-KV byte guarantee or change ordinary
prefix List behavior. External full-volume representation/admission and explicit metadata
client budgets remain #80's work. Preserve existing KV transport configuration rather than
impose a backend-wide cap that could make currently readable volumes inaccessible. This is
a correction to the description of the existing limit, not acceptance of a new allocation
policy; the review must still examine allocations introduced by ReadSet.

- **Header parse status:** baseline source verification confirms unchanged errStatus maps raw
  header JSON errors to Unavailable. Preserve that explicit existing error classification;
  the new validated-prefix rule classifies stable chunk corruption as Internal. New tests
  distinguish the two rather than changing a pre-existing header status under this issue.

## Code review round 1 — commit accounting

- **R1-C1:** use one actual-header-history observer in concurrent, superseded and ambiguous
  Attach tests. Record direct Mem applications below the injected retry, and replay native
  etcd header PUT ModRevisions from the known starting revision through the known final
  revision, under a bounded context. Deduplicate applied revisions; do not infer count from
  API success, outer TxnRev results or the final epoch. Existing kvtest.Start exposes the
  client, so this needs only service_test.go and introduces no production hook or API.
  Restoring the old observer in an isolated overlay fails on both backends (count 1 versus
  epoch delta 2); the shared corrected observer and focused race checks pass.
