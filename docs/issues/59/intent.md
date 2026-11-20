# Issue #59: WAN-tolerant backups — intent

[Issue #59](https://github.com/jsfakian/Flaminio/issues/59), README roadmap phase 4,
ARCHITECTURE §1.1, §1.2 and §14.1. This change is the issue's step 2 (a bandwidth cap and time
windows), with what #52 leaves of step 1 for a slow link. The review revision qualifies its
"Done when" with #53's fixed commit deadline and the declared local packet boundary.
Steps 3 (a long outage) and 4 (fleets) build on it: issue #59 stays open after this change. It
sits wholly on the first steps of [#52](https://github.com/jsfakian/Flaminio/issues/52)
(backups), [#53](https://github.com/jsfakian/Flaminio/issues/53) (locks) and
[#51](https://github.com/jsfakian/Flaminio/issues/51) (snapshots), planned and not built:
assumed done as their intents plan them.

## Why

An edge site's link is slow, shared with the site's own work, and sometimes gone for days
(§1.1: every external dependency "sometimes unreachable"). The code has no backup at all (#52's
intent, Why 1). What follows is what #52's plan leaves for such a link, with one example: a
10 Mbit/s uplink, a cap of 5 Mbit/s, a window from 22:00 to 06:00.

1. **#52 already resumes; step 1 is mostly its outcome 9.** The store away or the worker
   killed, a rerun completes "without uploading again the blocks that arrived". What that
   leaves:
   - **The block is the atom.** S3 cannot resume one `PUT`, and a multipart part is at least
     5 MiB (AWS's limit): a 4 MiB block cannot be sent in parts. A cut loses the blocks in
     flight, 4 MiB each. A block takes 3.4 s at 10 Mbit/s, 6.7 s at 5, 34 s at 1 Mbit/s, and
     longer when several share the cap. A link that never holds that long sends nothing.
   - **Someone must run it again.** The worker is an administrator's command, and schedules
     wait for #51's snapshot deletion (#52, Why 7). Nothing waits for the link to come back.
   - **Timeouts are whatever the object-store client has.** One that bounds a whole request
     fails every block that a cap makes slow.
   - **What a resume reads first** (the base manifest, which blocks are there) crosses the same
     link. Nobody has counted it.
2. **Nothing limits what a backup takes.** No cap or window is implemented. #52 now plans a
   Rust worker with an aws-sdk-s3 adapter; #58's reviewed plan leaves WAN policy integration
   separate. The former open language/client choice was settled in #52, not reopened here.
3. **A limit trades the recovery point, and nothing shows the trade.** The example carries
   16.8 GiB a night (4 291 blocks); uncapped and all day, 100 GiB.
   - A first backup of 200 GiB of written data that does not compress takes 12 nights. Until
     it ends the site has no backup: an interrupted one is not listed (#52, outcome 9).
   - A volume that changes more than a night carries falls a little further behind each day.
   - The snapshot stays until the backup ends, and holds space: 64 KiB copied at the first
     write to each cluster (#51, Why 3).
4. **The amplification #52 found matters most here.** The unit of change is the 64 KiB
   cluster, the unit of upload the 4 MiB block (#52, Why 2): between 1 and 64 times more is
   sent than changed.
   - 10 000 small writes spread evenly over a 100 GiB volume, the worst case: 39 MiB written,
     about 620 MiB of clusters, about 8 300 blocks, 32 GiB. Under the example that is 15.4 h:
     two nights for one day's changes. The clusters alone take 17 minutes.
   - zstd does not help on an encrypted volume (#52, Why 8), and edge hardware is "physically
     exposed" (§1.1): encryption is the expected case there.
   - The remedy is the repository format, which #52's ADR fixes and every later restore reads
     (#52, Why 11). After that ADR, changing it takes a superseding one. Smaller blocks cost
     the other way: more objects, and more of item 5.
   - Where real workloads fall between 1 and 64 is not measured.
5. **Requests are traffic too.** #53 checks every reused exact version: one retention read
   when covered, or three requests to extend and verify it. For 1 TiB in whole4MiB blocks that
   is262144..786432 requests plus pages/descriptor:7.3..21.8 hours serialized at100ms, even
   without changed payload. A manifest is complete and paged; variable exact-version IDs and
   JSON entries make its size graph-dependent, not a fixed small resume overhead. #52 resume
   also authenticates exact-version GETs. A byte cap alone does not bound request latency.
6. **A long outage is a question of local capacity, which has no ground yet.** §11.6's
   watermarks, overcommit and rebuild reserve are not in the code. Placement counts replicas
   per node, not bytes (`NodeLoad`, `meta/domain/domain.go:303`). A full store answers
   `OutOfSpace` (`flaminio-store/src/error.rs:43`); its use is known only to itself (`Stats`,
   logged once at start, `flaminio-io/src/node.rs:267`). #72 now plans the per-node capacity
   ledger/admission first step; it does not implement outage snapshot eviction. #51's scoped
   step adds no snapshot-pruning policy: needed pins cannot be dropped to obtain capacity.
7. **The lab can delay a link, not cap or cut one.** `scripts/guest/chaos.sh slow` adds a
   netem delay to all of `wg0`: replication and etcd with it. The lab's store is #52's, on a
   VM that may also be an etcd member.

## What we want

1. **A transfer limit:** a cap in bytes per second and, optionally, windows of the day in a
   stated time zone. Until `BackupTarget` exists it is the backup command's setting, as #52's
   other settings are. Without one, a backup behaves as #52 left it.
2. **The cap is honoured.** Over any interval of a stated length, a backup sends the store no
   more than the cap allows, within a stated margin: blocks, manifests, requests and retries
   together, independently measured after the owned local egress shaper; downstream arrival
   captures are diagnostic because network queues can batch otherwise paced packets.
3. **The window is honoured.** Closure stops admission, closes the owned egress gate and
   cancels store sockets, subject to the qualified published observation tail. Packets already
   beyond that boundary cannot be recalled. What arrived stays; only ambiguous in-flight block
   PUTs are retransmitted, with required verification/control traffic counted separately.
4. **An explicitly enabled transfer policy retries by itself.** Through closed windows and
   transient outages it waits and resumes while its grant and #53 retention/commit budget
   remain compatible. A tiny rate or a long closure may prevent completion in that budget; a
   late accepted root permanently ends its operation. Diagnose those outcomes and preserve pins
   for the prescribed abort/new operation. Killed, rerun resumes under the same policy. No-policy
   commands retain #52's store-failure exit and manual rerun; retries do not hammer the store.
5. **Slow is not failed.** An upload that still advances, however slowly, is not given up. One
   that has stalled is, by its progress and not by its age.
6. **A cut costs a bounded amount.** After a link failure at any point, the backup sends again
   at most K*(4194304+80) encoded block PUT bytes for K<=4 in flight. Exact-version GETs,
   page/root/control bytes and requests before new upload are separately graph-bounded and
   counted both ways per resume episode. Repeated outages have no finite aggregate bound.
7. **The trade is visible.** Before sending, a backup says how much it has to send and when,
   under its limit, it would end. While it runs, an administrator can see whether it is
   sending, waiting for its window or waiting for the link, what has arrived and what is left.
8. **A limit never touches safety or the application.** It decides only when bytes leave. No
   application I/O waits for it, no backup is listed sooner or with less because of it, and a
   wrong wall clock moves a window, nothing else.
9. **One limit for every mover.** The limit's rules are stated once, over a clock and a count
   of bytes, and know nothing of backups: #58's shipper can adopt them in its own change, and
   `BackupTarget` and `ReplicationRelationship` will carry the same setting. If the two movers
   are in different languages, they agree through shared vectors, as `make meta-vectors` does.
10. **Historical request to #52, now settled:** #52 fixes whole 4 MiB blocks, encrypted
    envelope bounds, amplification and the common `ObjectStore` adapter. This revision defines
    the concrete transport behind that seam; #52's approval is not reopened.
11. **Tests and the lab.** The limit's rules are tested without a network or a real clock. The
    lab can cap, cut and restore the path to the store alone, leaving the cluster's own
    traffic. A lab check, on a volume under write load, with windows of minutes: a capped
    backup, its rate measured independently at owned local egress and store arrivals recorded
    diagnostically; a cut that resumes automatically with only ambiguous in-flight block PUTs
    repeated (verification/control separate); closure/gate tail, reopening and completion under
    a compatible retention policy; the backup equal to the
    snapshot afterwards. Recorded, not judged: the time of a backup of a known size under a
    known cap, and the bytes/requests of a resume. #83 physical-only admission blocks the full
    campaign on current VMs; use authorized disposable qualified physical nodes, or explicitly
    label an authorized earlier legacy VM run exploratory. No fake physical profile or bypass.

## Out of scope

- **Step 3: a long outage.** Which snapshots are kept while backups cannot run, and how that
  stays within capacity. It needs snapshots deleted (#51 step 3), schedules (#52 step 4) and
  §11.6's fuller capacity policy beyond #72's first ledger step. The worker adds no snapshot
  schedule of its own.
- **Step 4: fleets.** Many clusters, one store: repositories side by side, credentials per
  site, policies managed centrally (§1.2, "later").
- **Schedules,** and what a schedule does with a run it missed: #52's step 4.
- **Changing the repository format,** the block size included: #52's ADR (outcome 10).
- **One budget for a site.** The cap bounds one run. Two backups at once, or a backup and a
  ship, each take their own. A budget shared across movers and nodes comes with the objects
  that name a destination.
- **#58 under the limit.** Its shipper applies outcome 9's rules in its own change.
- **Fewer requests in an immutable repository** (Why 5): #53's. The window bounds when they
  run; nothing here reduces them.
- **Verify and restore over a slow link.** Both read a backup in full (#52, outcome 10), and
  start again when cut. They take no limit here.
- **A first backup by other means,** such as a disk carried to the central site.
- **A cap that adapts** to other link use, and site-wide physical/tunnel QoS (`tc`, DSCP):
  the site's. The owned per-run local egress shaper is part of this design.
- **Fair queuing of the backup's replica reads** against the application (§16.1): #52's
  outcome 11 bounds them.
- **Alerts on a recovery point that grows old.** Outcome 7 shows it on request.
- **A KMS or a DR peer that is unreachable** (§1.1): #57, #58.

Review-round-1 changes above are proposed/unapproved; historical 2026-11-13 approvals and
settlements are preserved in [decisions](decisions.md). Exact contracts: [policy](policy-contracts.md)
and [transport](transport-profile.md). No runtime or shared-lab action is authorized by this draft.
