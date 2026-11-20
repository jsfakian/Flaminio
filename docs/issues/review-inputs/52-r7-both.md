# #52 plan review, round 7, focus design-code and design-security

Scope: `git diff eb95fa0..62cbbfc -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
S4/S7 (spec.md:60-64, 82-87), the resume section (design.md:178-188), lab steps 4-5
(design.md:393-397), RegisterSnapshotSource's idempotence and source cap (contracts.md:150-152),
the batch revalidation rule (contracts.md:52, design.md:110), and
`dataplane/crates/flaminio-metaclient/src/lib.rs:136-157`.

## Earlier findings
- R6-C1: fixed. Only the registration end rule aborts. Store, interrupt and lost-root failures exit 1 with the grant active, so S7, lab step 4, the failure matrix and `store_fault_resumes` now agree.
- R6-C2: partly fixed. Pre-`Accepted` failures, the idle close and `Unavailable` are classified. A `retry` has no bound (R7-C1), and the transient set is incomplete (R7-C2).
- Nit (`--compare-source` "a live reader grant of its own"): fixed.

Also checked, and they hold:
- A closing or closed grant reaches the end rule with no message matched. A `Refused` (`closed`), a session past `Accepted` lost or barrier-closed, and 3 pre-`Accepted` failures each lead to a registration. That registration is refused (`FailedPrecondition`, `NotFound`, the generation compare), and "any other refusal" then runs the end rule.
- Re-registering the same pair does not grow `sources` (contracts.md:150-152), so the 3-try cycle cannot exhaust the source cap.

### R7-C1 — Medium
**A source that keeps answering `Refused` (`retry`) is retried forever, with no revalidation or reselection. The new transient-validation `retry` makes this realistic, and a closing grant then never reaches the end rule.**
- Evidence:
  - A `retry` is "retried with the same token after a jittered backoff ... registering nothing" (contracts.md:472-474). Only a connection ending before `Accepted` with no `Refused` counts toward the 3 tries and the reselection (contracts.md:474-478).
  - This round added "a ValidateSnapshotRead answering `Unavailable` or not sent with the node's metadata client down" to `retry` (contracts.md:337-339).
  - Take a source node that the worker can reach but that is cut off from the metadata service. Every Hello it gets answers `retry`, for as long as the partition lasts.
  - Meanwhile a failover can take that node out of the ISR. Revalidation runs only "around bounded read batches" (contracts.md:52; design.md:110), and a Hello retry loop starts no batch. The chunk stalls, against S4's "reselect/revalidate" and "a failover does not stop such reads" (spec.md:62-64).
  - If that node holds the last source the worker needs and its grant is closing, no registration follows. The run neither ends nor closes its grant. That breaks the "always reaches" claim of contracts.md:455-457 and decisions.md (round 6, "Only an unusable grant aborts").
- Fix:
  - Count a `Refused` (`retry`) for a transient validation failure toward the same 3-in-a-row rule as a pre-`Accepted` failure: re-read the volume, then register another ISR member (the same one if it is the only one).
  - Caps, rate, bucket and outstanding-call `retry`s keep costing no registration.
  - Add the case to `closing_grant_aborted`: a node answering `retry` for its validation, then another member registered, and on a closing grant, `aborted`.

### R7-C2 — Low
**Only `Unavailable` and "the metadata client down" make a ValidateSnapshotRead failure `retry`. The node's own metadata client also reports timeouts and partitions as `DeadlineExceeded` or `Cancelled`, and the service reports contention as `Aborted`. Each of these falls under "ValidateSnapshotRead's refusal", that is `closed`, and so costs a registration per Hello.**
- Evidence:
  - contracts.md:337-340.
  - `flaminio-metaclient/src/lib.rs:139-143` synthesizes `DeadlineExceeded` on its own timeout, and lib.rs:146-153 treats `Unavailable`, `DeadlineExceeded` and `Cancelled` as an endpoint that did not answer.
  - The CLI's own transient set is `Unavailable`, `Aborted`, `DeadlineExceeded` (contracts.md:446, 462-463).
- Fix:
  - `retry` on every code the metaclient treats as no answer (`Unavailable`, `DeadlineExceeded`, `Cancelled`), plus `Aborted` and the client down. Only an answered refusal is `closed`.
  - Add `DeadlineExceeded` to the `validation_rate_bounded` case (design.md:307).

## Nits
- design.md:231-232: "loses (its node down; the CLI after its tries toward that node)" reads as if the CLI tried before re-registering a session past `Accepted`, which it re-registers at once (contracts.md:469-471). Scope the tries to "a connection lost before `Accepted`".

Reviewed commit: ccc2082ef7a5c2a5bab2aee6f044fa2b1af87757
