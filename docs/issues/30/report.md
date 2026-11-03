# Issue #30: a volume created again starts above every earlier epoch — report

The pipeline's record for #30: [intent](intent.md), [spec](spec.md), [design](design.md),
[decisions](decisions.md). Loop rules: `.claude/skills/ship-issue/review-loop.md`.

## Review rounds

The spec and design loops ran before this pipeline's rules (commits 7548371, f8def04).

**Code loop, round 1** (whole change, fb99440 with main merged; code and security reviewers):
0 Critical, 0 High, 0 Medium, 5 Low, 8 Nits.
- **Low, both reviewers:** `Store::delete_volume(_, u64::MAX)` skipped a chunk fenced at
  `u64::MAX`, so `delete VOLUME` reported success and left it. Now `Option<u64>`, `None` for
  every chunk (D13); store and replica regression tests.
- **Low:** an earlier volume's version 2 token at a Hello was untested: a replica test now
  shows it `Stale`, dropping nothing.
- **Low:** the drop has no simulator check: D10 says why, and which tests stand in.
- **Low:** `docs/lab.md`, `docs/replication-protocol.md` and `docs/followups.md` disagreed
  with the change: updated.
- **Low:** S9 untested: D11 says why.
- **Nits fixed:** `lab.sh` checks the first epoch is a number before arithmetic; the
  `unverified` feature's comment; an engine case for a token that does not decode; R10 for a
  drop failing after its first batch. **Not changed:** skipping the drop for a volume already
  fenced above its first epoch is not sound (D12); the design's cost claim is corrected
  instead. Encoding each delete record twice to size the batches is left as it is.
