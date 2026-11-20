# Plan review: issue #71, round 1, focus design-code (plus the plan code checklist on intent and spec)

Scope: `docs/issues/71/{intent,spec,design,contracts,adr-proposal}.md`, checked against the code
of the same worktree and, where the plan relies on them, the drafts of #43, #54, #69, #70, #77, #83.
Paths below are relative to `/tmp/flaminio-plans-2026-11-12/71` unless absolute.

Counts: High 1, Medium 10, Low 6, Nit 4.

### R1-C1 — High
**Events about the cluster-scoped summaries cannot live in `flaminio-observability`.**
The CRDs are cluster-scoped (design.md:67, contracts.md:91), yet the publisher creates and the
reader reads their Events only in the observability namespace (design.md:84–87,
contracts.md:121–125). Kubernetes requires an Event whose `involvedObject.namespace` is empty
to be in `default` (`kube-system` is also allowed for `events.k8s.io/v1`); client-go's recorder
writes it to `default`. As designed, every Event create is refused (403 with the proposed Role,
or 422 if forced into the addon namespace) and the reader's Role sees nothing, so the issue's
"injected faults produce the expected Conditions/Events" cannot be met. Not verified against the
Kubernetes source here (no network); the rule is in the API server's Event validation.
Fix: either make the three kinds namespaced in the addon namespace, or publish to `default` and
grant create/patch there (publisher) and read there (reader), stating what else a reader then
sees; add the envtest case "Event for a cluster-scoped summary is accepted and readable".

### R1-C2 — Medium
**"Its agent certificate authorizes only its telemetry scope" is false in the code.**
contracts.md:73 and 126–127 let the node observer push with the node's `agent` identity. That
identity already may call `Attach` (only_if_target), `RegisterNode`, `NodeHeartbeat`,
`ListTombstones` and `ConfirmDeletion` for its node (`meta/service/auth.go:59–78`). A new
long-running root process holding it contradicts S5 ("node producers have separate
least-privilege roles") and the sentence itself.
Fix: a new node component (for example `observer`): the metadata service then gives it only the
three open reads (`auth.go:79`), replicas refuse it (`node_with("io")`), and the collector admits
Push only from `node/<own>/observer`. Add it to the Go/Rust identity vectors and the denial table.

### R1-C3 — Medium
**The snapshot producer and its handoff are not designed for how the engine is owned.**
design.md:19–30 speaks of "queue-owned" handles and "double buffering" only. In the code:
(a) with no volume attached, the main thread drives the engine from the `Slot`
(`flaminio-io/src/ublk.rs:785–790`), there is no queue, and that is the state of every
replica-only node, whose disk capacity must still export; (b) when attached, the engine sits
behind `Shared.engine` and up to 64 queue threads run (`ublk.rs:507`, `node.rs:203`), all of
which end at detach, while `instance_id` is per process: counters owned by a queue go back to
zero under an unchanged instance, which contracts.md:70–71 treats as impossible; (c) the
control-socket thread holds only `ctl`, `engine` (a sender), `wake` and `attached`
(`control.rs:67–73`), so `observe` needs a shared structure the design never names, and S11
forbids a new lock.
Fix: name the owner of the counters (node level, surviving attaches, or a per-attach generation
in the schema), who publishes the store snapshot in both states (the main loop and queue 0's
ticker), how per-queue counters are merged, and the primitive (per-queue atomics plus an
atomically swapped snapshot pointer, or a named equivalent).

### R1-C4 — Medium
**Sampled spans have no path out of the data plane; Rust logs have no approach.**
design.md:40–47 and contracts.md:80–87 fix the span ring and say "OTLP uses mTLS", but the
`observe` schema (contracts.md:53–59) and `Telemetry/Push` (contracts.md:61–65) carry no span
field, and the 4 MiB `observe` cap equals the ring size. Which process holds the OTLP client
(the Rust daemon, needing a new TLS client stack, endpoint config and an identity, none in
"Files"; or the node observer, with no transport from the daemon) is unstated. S3's structured,
rate-bounded logs are designed only for Go (CSI); the data plane logs through 52 `eprintln!`
sites in `flaminio-io`/`flaminio-node`, some on the read path (`ublk.rs:1579`, `1686`).
Fix: choose the span route and its bounds (for example a second root-only socket command
draining the ring, exported by the node observer), list the files and dependencies, and state
what happens to the Rust `eprintln!` sites (which become structured, how they are rate-bounded,
or that they are out of scope and why S3 still holds).

### R1-C5 — Medium
**The only recovery progress that exists today has no producer in the design.**
"Done when" needs recovery progress in metrics and CLI. #45/#54/#68 are declared absent, so the
lab's only recovery is the target's resync/reconcile. design.md:36–37 says "target progress
reflects durable copy/recovery work" and nothing more: `flaminio-repl` is not in "Files"
(design.md:143–151), `Target`'s `Stats` is `cfg(test)` (`flaminio-repl/src/target.rs:109`), and
no accessor exposes `Resync.todo` or `Phase::Reconciling.pending` (`target.rs:144–171`). How
`recovery_eligible` and `recovery_last_progress_age_seconds` are derived in a sans-I/O machine
is not said. As written the lab shows `unsupported` for `kind=resync`.
Fix: design the base producer: the read-only accessor on `Target` (remaining blocks per chunk and
member, blocked reason, time of the last acknowledged copy from the `now` it is given), its path
through `engine.rs` into the snapshot, and a test that fails without it; or state in the spec
that recovery progress waits for #45 and that the Done-when is not met until then.

### R1-C6 — Medium
**Volume incarnation and cluster identity are not readable through the interface the collector uses.**
Object names, `source_incarnation`, series retirement and plans bind "FirstEpoch" and "cluster"
(design.md:68–70, 106–107; contracts.md:11–12, 92, 174–176). `Volume` in the protocol has no
`first_epoch` (`api/proto/flaminio/meta/v1/meta.proto:117–127`; it exists only in
`meta/domain/domain.go:47`), #69 adds no field, and #70 proposes `first_epoch:12` in its own
draft (`/tmp/flaminio-plans-2026-11-12/70/docs/issues/70/design.md:152`). The metadata service
has no cluster identity at all; #54 proposes one (`home_cluster_uuid`, tag 41). Stage A–C
(design.md:128–131) name #69/#77 only, so "incarnation reuse" (S4 test) and "wrong cluster" (S8
test) cannot be built at the stage that needs them, and legacy volumes all have first_epoch 0.
Fix: declare the dependency (or own the field with a coordinated tag), say what the name hash and
retirement use while the field is absent or 0, and where cluster identity comes from before #54.

### R1-C7 — Medium
**S8's backend preconditions are required but have no contract, and are another owner's change.**
design.md:108–112 makes destructive verbs depend on "narrow conditional support [added] to
existing metadata RPCs where needed", and contracts.md:159–161 repeats it, without naming an
RPC, a field, a tag, the revision that is compared (no response exposes one) or the etcd
transaction. `DeleteVolumeRequest` and `MoveReplicaRequest` have only ids today
(`meta.proto:163–165`, `240–245`); there is no disk claim/release owner API at all. The issue
asks for the six families "reusing existing service authorization"; ten mutating verbs with
plan/confirm and new conditional metadata fields are new protocol work (AGENTS.md: no features
unless asked) that belongs to the owners (#8/#13/#18/#45/#70/#54).
Fix: either cut the mutating verbs to typed calls of RPCs that exist with their present
semantics and move the conditional-precondition work to named owner issues, or specify here per
verb: RPC, new fields with numbers, the compare in the transaction, old-caller behaviour, the
`auth.go` rule and its test.

### R1-C8 — Medium
**`recover plan/apply --confirm` does not match the only recovery owner contract drafted.**
contracts.md:152–153 and 174–185 require a backend that validates a plan digest and atomically
accepts or refuses, behind `Capabilities, Plan, Apply, Status`. #54's draft
(`/home/jsfakian/Documents/src/flaminio-54/docs/issues/54/contracts.md:25–41, 89–100`) has no
plan or digest: a restore is `BeginRestore(operation_uuid, …)` and a long-running
`flaminio-backup restore-volume` worker that needs repository credentials, key files and a
node's control socket, with `expected_job_revision` as its fence. contracts.md:183–184 says #54's
RPC names are absent; they are now drafted. `Apply` from a kubectl plugin cannot be that worker.
`recover status` does map to `GetRestore`.
Fix: map the adapter to #54's operations (begin/status/cancel by operation id, the worker run
where its credentials are), say what the digest binds when the backend takes none (or drop
plan/confirm for restore), and record the agreed shape in both plans.

### R1-C9 — Medium
**"Multiple collectors are safe idempotent publishers" does not hold with push.**
design.md:71–73. Producers push to one endpoint; with two replicas each producer's connection
lands on one of them. The other never receives that node, so its `/metrics` shows
`*_observation_fresh` 0 or absent and `TelemetryLost` fires on whichever replica Prometheus
scrapes, counters differ per replica, and its status view is `Unknown` for half the cluster.
Comparing `producerSequence` stops an older sample overwriting a newer one, but not a replica
publishing "Stale" for a source it simply does not receive.
Fix: state one replica (and what a restart costs: unknown until new evidence), or have producers
push to every replica, or elect the publisher and scrape target with a Lease (it selects a
publisher, it fences no storage). Add the two-replica test.

### R1-C10 — Medium
**The S1/S11 frontend tests cannot run where the plan puts them.**
design.md:162 and 171 name fixtures over fast-copy, zero-copy, fallback, error, flush and
discard "exactly once" and an enabled/disabled equivalence. Those branches are in
`engine_io_task` and `engine_io_task_zc` (`ublk.rs:1207–1288`, `1360–1462`), which need a ublk
device and root; the file has one unit test (`ublk.rs:1747–1768`) and CI runs no ublk. The
memory-device tests in `flaminio-node` do not pass through the frontend, so they cannot see a
fast read counted twice or not at all.
Fix: name the seam: one record call at the two commit sites (`ublk.rs:1284`, `1459`) taking
(op, len, result, start), a pure classifier unit-tested for every result, and a named lab check
(under `scripts/checks/`) that compares the counters with fio's totals over fast, fallback and
failing reads in both copy modes. Say which of the two proves S1.

### R1-C11 — Medium
**Four doctor checks have no implementable mechanism.**
design.md:98–103, contracts.md:111–118. (a) Peer RTT: the node observer is a push client with no
listener; the probe's target port and the identity it presents are not given, and the replica
port admits only `io` (contracts.md:78 forbids telemetry identities there). (b) Wall skew: no
estimator is defined (whose clocks, which exchange, how the uncertainty is bounded). (c) SMART:
tool or ioctl, the privilege it needs (an NVMe log page is an admin command, not a read-only
mount) and what the lab's virtio disks report are not stated. (d) "Risky configuration": no list
of the settings checked, so the "unsafe config" fixture (design.md:168) has nothing to assert.
Fix: specify each (endpoint and identity, estimator and its error term, probe and capability,
the enumerated settings and their sources), or mark the check `unsupported` in this issue and
say so in the spec against the issue's step 4.

### R1-C12 — Low
**The series bound undercounts.** contracts.md:41–44 gives `12*V`; the per-volume families are
under_replicated 1 + liveness_risk 1 + three recovery families × four kinds 12 + fresh 1 = 15.
The constant 100 must also hold `telemetry_dropped_total` (5 kinds × 5 reasons = 25 per
component), `telemetry_complete` and `feature_supported` for every component. The fixture that
must "prove actual series <= bound" fails, or admission accepts more than the budget.
Fix: recompute per family in the table (a column with the count) and add a per-component term.

### R1-C13 — Low
**Exit code 2 means two things.** contracts.md:169–170: invalid syntax exits 2 and doctor's
verified failures exit 2. A script cannot tell a typo from a failed prerequisite; #54's draft
uses 2 for usage only. Fix: give doctor failures their own code and test it in the golden matrix.

### R1-C14 — Low
**`io_errors_total` reasons do not map to what the frontend can see.** contracts.md:19 lists
io/capacity/timeout/stale/precondition/other. The frontend receives only
`IoError::{NotAttached, Invalid, Deposed, Unreadable}` (`flaminio-node/src/engine.rs:241–250`)
plus local failures and `EOPNOTSUPP` (`ublk.rs:1259–1280`); nothing yields capacity or timeout.
Fix: give the mapping table from each completion to a reason, and drop or source the two unused
reasons (changing `IoError` is a data-plane change to list in "Files").

### R1-C15 — Low
**`disk get ID` / `volume get ID` have no lookup.** Names are `vol-`/`disk-` plus a hash that
includes the incarnation (contracts.md:92–94), which the user does not know; ids are up to 128
characters (`meta/domain/domain.go:87`), over the 63 of a label value. The design does not say
how the CLI finds one object without listing all of them. Fix: state it (list and filter within
the entity limit, or a selectable field/hashed-id label) and test the reused-id case.

### R1-C16 — Low
**Retirement is undefined for Disk and StorageNode.** design.md:81–82 retires a summary "only
after authoritative source disappearance". Disks have no metadata record and nodes are never
deregistered (`meta.proto:15–61` has no such RPC), so no authoritative disappearance exists for
either. Fix: define it (for example: stale indefinitely, removed only by an administrator
command or after a stated retention with a `Retired` condition) and test a replaced disk.

### R1-C17 — Low
**`Disk` as a pure summary diverges from ARCHITECTURE §17.2.** There `Disk` carries desired
state (active, cordoned, draining, released) (`docs/ARCHITECTURE.md:893–905`); here it is an
immutable publisher-created object with a hashed name, deleted on retirement (design.md:67–70,
contracts.md:91–94). A later owner of the desired state inherits names and a deleter it cannot
use. Fix: say in the design and the ADR proposal how the architecture's `Disk` is reached (spec
left for its owner and never deleted by the publisher, or a different kind name for the summary)
and include §17.2 in the docs to update.

## Nits
- spec.md:3 links `decisions.md`, which does not exist in the folder ("if created"): a broken link.
- design.md:143 "`flaminio-io/src/ublk.rs`, control/node" reads as the Go `control/` tree; write `control.rs`, `node.rs`.
- contracts.md:22 "includes not-yet-reusable frees": quarantined clusters are free bits, not allocated ones (`flaminio-store/src/bitmap.rs:10`, `61–63`); state capacity = allocated + quarantined + reusable.
- spec S1 "every frontend path" and design.md:19–21: say whether the MVP `flaminio-io serve` path (`ublk.rs:166–350`, commit at `346`) is instrumented or out of scope.

Reviewed commit: 76cf82f7ae03bab66a9410264470a6f3991715de
