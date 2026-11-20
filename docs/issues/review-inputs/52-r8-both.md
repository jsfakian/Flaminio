# #52 plan review, round 8, focus design-code and design-security

Scope: `git diff ccc2082..c4b750a -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
the caps and rates (contracts.md:301-315), RegisterSnapshotSource's idempotence and source cap
(contracts.md:108-111, 150-152), the CLI's end rule and sessions (contracts.md:456-485),
`dataplane/crates/flaminio-metaclient/src/lib.rs:127-163` and the service's codes
(`meta/service/service.go`: `Unavailable` at 151, `Aborted` for contention at 218 and others).

## Earlier findings
- R7-C1: fixed. Every `Refused` (`retry`) now counts toward the 3 tries with no `Accepted`, whatever its cause. After the third the worker re-reads the volume and registers again, so a node cut off from the metadata service no longer stalls its chunk, and a closing grant reaches the end rule (contracts.md:456-460, 476-482). `closing_grant_aborted` covers it (design.md:313).
- R7-C2: fixed. `Unavailable`, `DeadlineExceeded`, `Cancelled` (lib.rs:146-153, and the synthesized `DeadlineExceeded` at 139-143) and `Aborted` are `retry`; only an answered refusal is `closed` (contracts.md:337-342). `validation_rate_bounded` lists the codes (design.md:309).
- Nit (design.md:231-232): fixed. A session lost past `Accepted` and the 3 tries before `Accepted` are now separate (design.md:232-235).

Also checked, and they hold:
- A re-registration after cap or rate refusals cannot get around the rate. The rate is keyed per (grant, node, chunk), not per token (contracts.md:309), so a re-registered token for the same pair is still throttled.
- Reselection cannot grow `sources` beyond what it could before. It moves only among ISR members, and a repeated pair is replaced in place (contracts.md:150-152), so the chunks x (RF + 2) cap is unaffected.
- The metadata load added by cap or rate reselection is bounded. Each worker has at most 4 reads in flight (contracts.md:438), and each reselection costs one GetVolume and one registration. decisions.md names this load as a risk (round 7, "Risks named").
- The service uses no code other than `Unavailable` and `Aborted` for its transient failures, so the `closed` remainder holds no transient code.

No finding of Low or worse.

## Nits
- contracts.md:476-479: say the 3-try count is kept per (chunk, node) and whether the 1 s to 10 s backoff restarts at reselection; as written, the 10 s ceiling looks unreachable.

Reviewed commit: f4915b9ad0630ae721ee37eb2473e24c9366fa3e
