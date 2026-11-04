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

**Round 2** (incremental, `git diff HEAD~1` of 62c8b5a, sonnet): clear. Both reviewers
confirm round 1 fixed; the code reviewer showed both late-ack tests fail without the
`sent` check. Nits noted: `keep_dropped_acks` switches off the clearing and the `sent` check
together; a comment on why `pump` returning when deposed leaves nothing queued.

Final state: the code loop ended clear after 2 rounds.

## Tests

All local checks pass on 62c8b5a: `make lint test race contracts` and `make sim` (20 000
seeds, 93 tests including the ignored simulator checks). `cargo test -p flaminio-repl`: 69
passed, 24 ignored (run by `make sim`).

| Rule | Result | Tests |
|---|---|---|
| S1 | pass | `digests_of_chunks_fenced_together_go_from_chunk_0`, fails with `Bugs::today_queue_order` |
| S2 | pass | `with_nothing_waited_requests_go_in_queue_order`, sim `todays_pump_gives_the_same_runs` |
| S3 | pass | the order tests; `the_order_tests_fail_with_todays_pump` |
| S4 | pass | `aborted_rounds_leave_the_queue`, `every_client_operation_is_tried_with_every_slot_taken` |
| S5 | pass | `client_starts_match_todays_pump`, `overlapping_client_operations_start_in_queue_order` |
| S6 | pass | `a_waited_chunk_takes_the_next_free_slot` and the fencing-order tests |
| S7 | pass | `waited_work_and_a_resync_alternate`, `resyncs_progress_while_a_member_flaps` |
| S8 | pass | `a_digest_sent_again_does_not_pass_a_waited_chunk` |
| S9 | pass | `deposition_fails_waiting_operations`, `deposition_ends_reads_waiting_to_go_again` |
| S10 | pass (lab, 2026-11-04) | `scripts/checks/reconcile.sh`, [results](../../benchmarks/2026-11-04-reconcile-order.md) |
| S11 | pass | `a_dropped_replicas_ack_does_not_count_after_it_rejoins`, the two late-ack tests, sim `catches_keep_dropped_acks` |

## Conformance

```
PASS

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 | yes | yes | target.rs:2227-2242 (`newly_fenced`), :1887 `queue_newly_fenced`; test `digests_of_chunks_fenced_together_go_from_chunk_0` (:3696, also fails with `Bugs::today_queue_order`) |
| S2 | yes | yes | target.rs:1909-1915 (`unready == 0` -> `pump_in_order`), :1961 (no waited chunk); `with_nothing_waited_requests_go_in_queue_order` (:3719); sim `todays_pump_gives_the_same_runs` (sim/tests.rs:316) |
| S3 | yes | yes | target.rs:2008-2056 (steps 1-4, `last_waited`, resync cap); tests :3763-3852, :3901, :3937, :3945, :3984, :4075 |
| S4 | yes | yes | target.rs:1916-1960 pass 1 (stale resync dropped), pass 2 tries every client op; `aborted_rounds_leave_the_queue` (:4083), `every_client_operation_is_tried_with_every_slot_taken` (:4120) |
| S5 | yes | yes | target.rs `client_view` assert around the selection; `client_starts_match_todays_pump` (:4575), `overlapping_client_operations_start_in_queue_order` (:4668), :4137, :4156, :4184, `a_failed_read_goes_again_before_later_overlapping_reads` (:3351) |
| S6 | yes | yes | follows from S3 selection; tests :4286, :4320, :4343, :4396, :4404 |
| S7 | yes | yes | `waited_work_and_a_resync_alternate`, `one_resync_item_in_flight_while_a_chunk_is_waited`, `resyncs_progress_while_a_member_flaps` |
| S8 | yes | yes | re-queue paths unchanged (front); `a_digest_sent_again_does_not_pass_a_waited_chunk` (:4444) |
| S9 | yes | yes | `depose` clears new state (:1123-1152); `deposition_fails_waiting_operations` (:4258); existing tests unchanged |
| S10 | partial | lab, pending | docs/replication-protocol.md §6 step 3 and §9 updated; followups.md §2 waits for the lab numbers |
| S11 | yes | yes | target.rs:2623-2634 (`adopt_replicas` clears `acked`/`sent`), :932-934 (`w.sent` check); tests :3503, :3563, :3569; sim `catches_keep_dropped_acks` (sim/tests.rs:475) |

Contract drift: none (no proto/RPC, socket, frame, on-disk or CLI change; flaminio-node untouched, as the design says; reconcile.sh `FIRST_CHUNK` matches design)
ADR conformance: consistent (the design lists no ADR, and none was added)
Scope drift: none
Docs: done (replication-protocol.md §6/§9); followups.md §2 and docs/benchmarks/ are lab, pending
Open review findings: none
```

## Lab

The design's "Measurement", 2026-11-04, on `vol1`, `NODE=fl-a`, without zero copy: done, every
run passed the check. Results: [docs/benchmarks/2026-11-04-reconcile-order.md](../../benchmarks/2026-11-04-reconcile-order.md).

| | main (e8b6d01) | branch (5833375) |
|---|---|---|
| First read, `FIRST_CHUNK=0` | 0.200–0.237 s (6 runs) | 0.125–0.168 s (3 runs) |
| First read, `FIRST_CHUNK=32` | 0.191–0.267 s (6 runs) | 0.138–0.155 s (3 runs) |
| Device ready | 0.093–0.163 s | 0.106–0.144 s |

- **Chunk 0 first** (expected well under today's 0.21–0.25 s): pass.
- **Chunk 32 first** (expected its own digests and what was in flight, not half the chunks):
  pass, 0.03–0.04 s after the device is ready (main: 0.08–0.12 s).
- Follow-ups §2 updated. Main has six runs: the first branch deploy did not rebuild (file
  times), so those three runs used main's binaries; the branch was then rerun with a verified
  rebuild.

## ADRs

None relied on beyond the protocol (`docs/replication-protocol.md`); none proposed.
