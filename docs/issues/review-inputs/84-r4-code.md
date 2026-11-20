## Earlier findings
Round 3 code findings (R3-C1..C4, N1): addressed by the cut of cleanup; the commands named (metactl volumes/tombstones/nodes/create/attach, node.sh attach, make vol-delete with VOL from env via lab.conf, node-up with FORMAT/QUEUES/ZERO_COPY) exist and behave as assumed. Design, spec S7 and kernel-check.md agree.

### R4-C1 — Low
Two fixtures need elapsed time (60 s nodes-alive wait; "300 s have passed before step 4") but the stub list in kernel-check.md "Script tests" has no clock: stubs are sudo, dd, blkdiscard, blockdev, findmnt, timeout, taskset. Without a stubbed sleep/date (or SECONDS), the fixtures take real minutes in make lint or cannot fail deterministically. Fix: add stubs for sleep and date (or a fake-clock variable honoured under FLAMINIO_SCRIPT_TEST=1) to the list.

### R4-C2 — Nit
kernel-check.md says the check "keeps no state of its own" while the Result section writes $LOCAL_STATE_DIR/block-contracts/<run>.json. Say "no state any run reads".

Reviewed commit: 87a60b3e84e95a7170708ed27b11fa24872ef11f
