# Report: #11

## Review rounds

### Code loop

**Round 1** (full change, code-reviewer and security-reviewer): 1 Medium, 1 Low, Nits.
- Medium (both reviewers): S11 incomplete. A late acknowledgement from a dropped replica
  still counted when its node kept its session through another chunk. Fixed: a write counts
  an acknowledgement only from a node it was sent to (`w.sent`). Tests
  `a_dropped_replicas_late_ack_does_not_count_after_the_drop` and `..._after_it_rejoins`.
- Low (code): the waited pump was O(N·W). Fixed: a map from chunk to wait rank and one
  bucketing pass, O(N).
- Nits fixed: `FIRST_CHUNK` validation in `scripts/checks/reconcile.sh`; `pump` queues newly
  fenced chunks itself instead of asserting it; `Bugs::today_queue_order` shows the S1 test
  fails on the old queue order.
- Nits noted: the `catches_keep_dropped_acks` simulator check tries one seed (settled in
  `decisions.md`); a replica that keeps answering digests with the wrong length keeps its
  chunk waited; `docs/followups.md` §2 is updated with the lab step.
