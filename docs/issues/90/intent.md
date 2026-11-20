# Issue #90: Bind replica membership to store identity — intent

[Issue #90](https://github.com/jsfakian/Flaminio/issues/90) · [spec](spec.md) · [design](design.md).
**Unapproved draft; not implemented.** Moved from #61 at
`5696573a126c0abe26347d3c9700e557489613cf` under the user-approved tracker split.
Historical #61 approvals are recorded in [decisions](decisions.md) as provenance only.

This shared root cause is independent of cloud policy: an empty replacement disk can inherit
a reusable node name's old ISR/source/lease role despite losing data and fences. #90 owns the
single-store identity, authorized bootstrap and full reconstruction transition. #45's recovery
work and #61's cloud policy compose with it; neither is claimed implemented here. #13's planned
retained fences and exact cleanup compose with the same identity. No cloud account is needed
to reproduce this failure model.

## Why

The protocol rests on a replica's durable state, its blocks and its fenced epochs, outliving a
restart (protocol §1). The simulator's crash loses volatile state only (`crash`,
`flaminio-repl/src/sim/mod.rs:1107`). An instance store breaks that on every stop and start: the
node returns under its name with an empty disk. Nothing notices.

1. **Nothing identifies a store.** Both formatters write a random `disk_uuid` into the
   superblock (`flaminio-disktool/src/main.rs:190`, `flaminio-io/src/cli.rs:82`; format spec,
   offset 40). `Store::superblock()` has no caller outside tests
   (`flaminio-store/src/store.rs:698`); only `flaminio-disk inspect` prints the UUID. A node
   registers a name and an address (`register`, `flaminio-node/src/engine.rs:584`); its record
   is an address and labels (`nodeRecord`, `meta/service/service.go:154`); chunk records name
   nodes (`Chunk`, `meta/domain/domain.go:19`).
   ARCHITECTURE §4 ("identified by the UUID in its superblock") and §10.1 ("reappears (same
   UUID)") say what is wanted.
2. **A blank store's `Hello` makes it a holder of empty chunks.** The reply lists the chunks held
   before the `Hello`: none (`ReplicaCore::handle`, `flaminio-repl/src/replica.rs:179`). The node
   then creates, empty and with nothing uncertain, every chunk the target's view places on it
   (`ReplicaService::hello`, `flaminio-node/src/replica/mod.rs:236`; `add_chunk`). The target
   takes a chunk missing from the reply for one with nothing uncertain (`on_fenced`,
   `flaminio-repl/src/target.rs:1311`). Right at a volume's first attach; wrong ever after.
3. **In the ISR, it is trusted.** The records keep a stopped node in an ISR when no target ran
   meanwhile (the stopped instance ran the volume's pod) or the ISR was at its minimum (#45,
   Why 4).
   - **Reads return zeros.** With nothing uncertain there is no reconciliation. Reads go to the
     target's own node if it is a member, else to the lowest id (`source`, `target.rs:1644`).
   - **A digest comparison finds a difference, not which side is right.** Digests cover only
     the extents some member reports (protocol §6). A mismatch is repaired from `source` again
     (`Work::Repair`, `target.rs:1792`): a blank authority's zeros are written over the good copy.
   - **The only member:** nothing to compare. Every other replica is then resynced from it:
     nothing is mapped, so they are told to discard (`on_mapped`, `target.rs:1445`). The stale
     but real copies are erased.
   - **At the minimum under a running target:** no new target is needed. The session reopens,
     and the acknowledgements its old store gave still count (`retransmit`, `target.rs:1332`).
4. **Outside the ISR, it rejoins on the dirty set.** The target that removed a member copies
   back only what was written since (`drive_resync`, `target.rs:2351`); digests cover that plan
   only. A node that stops and starts under a running volume (RF 3 with minimum 2, RF 2 with
   minimum 1) joins the ISR holding those blocks and nothing else.
5. **A blank store has no fence.** Fences are records in the store (format spec, `Fence`); an
   unknown volume starts at 0 (`replica.rs:169`). So a deposed target's `Hello` is accepted and
   its lease confirmed: ADR 0005 rests on "a fenced replica refuses to confirm older epochs".
   Traced, not run: RF 3, minimum 1; the old target on a, cut off, has ISR {a, b}; the new one
   on c fences b, removes a, resyncs c, acknowledges writes on {b, c}; b returns blank and
   reaches a first. a then serves reads without those writes, and acknowledges its own, until
   the new target's `Hello` reaches b.
6. **Nothing checks it.** The simulator creates chunks at setup (`sim/mod.rs:395`) and never
   loses a store; `Hello`'s creation exists only in the data plane. No test brings a node back
   on another store. #13's rule (a session creates chunks only when it raises the fence) does
   not cover it: every `Hello` raises a fence of 0.
7. **How a store goes blank.** `--format --force` (`make node-up FORMAT=1`,
   `scripts/guest/node.sh:47`) checks no record; only the write campaign refuses to format a
   node that other volumes name (`write_start`, `scripts/lab.sh:2154`). The agent's `claim`
   formats the one blank device (`control/agent/disks.go:136`,
   `control/cmd/flaminio-agent/main.go:74`): the claim policy guards other systems' data, not
   ours. Without a format a blank disk does not open (`store.rs:476`) and the node stays down.
   A claim at boot, as ARCHITECTURE §5.2 wants, turns every stop and start into this case.
8. **#45's states do not help.** A stop and start takes minutes: the node returns well inside
   `replicaReplenishmentWait` (10 min), missing then active, and "is only resynced, as today".
9. **Not run.** All of this is a static trace. There is no cloud: the lab is VMs on two
   bare-metal servers (`docs/lab.md`). `FORMAT=1` on a lab node is the same event to the
   protocol.

## What we want

1. Record the opened superblock UUID and service-owned lifetime with each observed store;
   bind earned membership, source/lease/ack and cleanup authority to the exact ReplicaRef.
2. A changed store is an unanswered old member. Existing minimum-ISR rules decide removal;
   no automatic below-minimum recovery, source promotion or blank-store trust.
3. Authorize every new remote/local Hello against current token/incarnation/epoch/assignment,
   before cleanup or creation, then persist its fence before confirming readiness. A later
   move needs a new authorized session. Keep steady I/O and heartbeats free of metadata calls.
4. Reconstruct replacement current data/holes/live writes completely; when snapshots/roots
   exist, prove every committed historical stage and committed/pinned root/receipt before join.
5. Define bounded paged grants, fair control scheduling, exact cleanup/capacity/wire contracts
   and restartable administrator-evidenced migration; ambiguous legacy provenance quarantines.
6. Check each new safety rule in the sans-I/O model and catch its named mutation, with real
   remote and synthetic local adapters covering delayed callbacks and restart.
7. Report observed/earned mismatches and unavailable chunks honestly. A formatter still makes
   a new UUID; reformat is destructive and grants no source or cleanup authority by itself.
8. Specify later authorized physical RF2/min1 qualification under #83. A separately authorized
   pre-#83 VM reformat is exploratory only; current shared VMs cannot satisfy production clock
   admission. No lab, disk or cloud experiment ran for this draft.
9. Update protocol, architecture §10.1, README/lab/follow-ups/Make help to actual behavior.

## Out of scope

- Cloud device classification/automatic claiming, stop/start policy, surviving-copy placement,
  zone cost and cloud evidence remain #61/#62; autoscaling/name changes remain #63.
- #45/#17 recovery stages for detached volumes, missing targets and ISR-minimum blockages;
  no new availability exception, force promotion or failure classifier.
- Same-UUID disk snapshot rollback/clones, malicious device provenance and `cluster_uuid`.
- Multiple opened disks per node and disk I/O failure states. #90 owns the ReplicaRef type for
  later users, without claiming #72 already provides that ABI.
- A runtime implementation, accepting an ADR, or evidence that a cloud policy is safe.
