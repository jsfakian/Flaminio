# Issue #77: Consistent volume reads across replacement — specification

[Intent](intent.md) · [Design](design.md). Approved 2026-11-13; not implemented.

## Terms

- **Header revision:** monotonically increasing KV modification revision for one volume header.
- **Coherent view:** header and complete chunk prefix that existed together at one read point.
- **Incarnation:** one create-to-delete lifetime, identified by the volume's first epoch.
- **Attach revision:** the exact header revision written by one successful conditional Attach.
- **Contention:** the header changed while assembling the response; not proof of disk corruption.

## The invariant

A successful volume/attach response, and every ISR response including Applied=false, never
combines header authority from one incarnation or attach with chunks acquired under another. Read validation cannot authorize any metadata write
or replace epoch/ISR fencing.

## Behaviour

**S1. Coherent volume view.** Every successful GetVolume response has a read point where its
header and complete ordered chunk set coexisted. Same-count replacement is not an exception.

**S2. Read validation.** Detect header changes while reading the chunk prefix and retry within
a bounded context-aware budget. Publication uses an unchanged header revision, not just name,
size, epoch equality or chunk count. A Creating/Deleting header is not a published volume.

**S3. Error classification.** A missing/replaced header yields NotFound or bounded contention
according to the retry outcome. Diagnose malformed/missing chunk records as corruption only
against a validated unchanged published header. Never return a partially decoded view as success.

**S4. Attach response.** A committed Attach has a known attach revision. Its returned header,
chunk view and signed token must refer to that authority. If a subsequent header change prevents
that response, refuse explicitly; do not overwrite an independently newer view with an older
header or pretend the earlier Attach never committed. Missing/Creating/Deleting pinned headers
are committed/superseded Aborted. Other postcommit errors preserve their status and known-commit
context, including cancellation; no response code is proof that the committed epoch rolled back.

**S5. Concurrent chunk changes.** Every ChangeIsr response describes its header and named chunks
at one snapshot or successful conditional-write point, including refusal/no-write responses.
Ordinary conditional ISR/move updates may proceed while a reader gathers a coherent chunk prefix. Do not add serialization that freezes normal I/O or
requires a steady-state data-path metadata round trip. All KV backends obey the same contract.

**S6. Compatibility.** Existing published volumes, including first_epoch=0 legacy headers,
remain readable. Keep wire shape, caller authorization and epoch issuance semantics unchanged.
New checks do not resurrect a deleted volume, bypass tombstones or clear Creating flags.

**S7. Recovery and cancellation.** Retrying reads is bounded and honors cancellation/deadlines.
Lost attach responses may leave a committed epoch; later recovery uses existing fencing and
reattach rules, without rolling the epoch back or silently retrying a committed write inside
the read helper.

## Tests

- S1/S2: deterministic header-read hooks interleave same-count delete/confirm/recreate with RF,
  geometry and first-epoch changes; returned state is coherent or refused, on memory and etcd.
- S3: removed/malformed chunk during changing header versus stable corruption; no partial success.
- S4: newer Attach, deletion and recreation between attach commit/header/chunk read and token
  creation; token/view authority match or refusal. A lost commit reply never decrements epochs.
- S5/S6: rejected ISR responses across replacement/multi-chunk updates; atomic named-read backend
  contract, concurrent ISR/move updates, legacy headers, authorization, vectors and interop.
- S7: repeatedly changing headers exhaust budget; cancellation ends retries and no goroutine leaks.

## Open questions

None for the proposed revision-validation approach. Review must verify the KV List operation
is an atomic prefix snapshot for every backend; future pagination must preserve that guarantee.
