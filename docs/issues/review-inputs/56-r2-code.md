Reviewed b2b76b7e28215afc331cd6c5a07ee0350027a010 incrementally from 513b973. Reviewer: /root/remaining_plan_reviews/review56_r2_code.

R1-C1/C2/C4/C5/C6 fixed. R1-C3 has a concrete contract, with the checkpoint gap below.

- R2-C1 — Medium: lab-contracts.md:131 permits an earlier successful read to become a checkpoint after newer mandatory writes finish. Carrying its stale value can falsely pass lost writes. Require a dedicated drained checkpoint read with no intervening block operation before the cut. Fixture: read v0, ACK v1, reference old read, then incorrectly read v0; never Pass.
- R2-C2 — Low: lab-contracts.md:175 requires a JSON Patch annotation-absence test, which RFC6902 does not support. Observe absence in GET, bind by resourceVersion test and atomically add; handle absent annotations map. Fixtures: absent map and concurrent foreign annotation. RFC6902 section4.6: https://www.rfc-editor.org/rfc/rfc6902#section-4.6

Old-epoch Invoke after completed successor response is correctly no-effect, matching sim/mod.rs:1394. No Nits.
Dependencies: #49 de46aef; #77 bb8df5b; #82 18fb585; #83 67862ad. No files changed.
