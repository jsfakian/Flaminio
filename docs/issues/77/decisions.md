# Issue #77 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

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
