# Issue #69: UI — intent

[Issue #69](https://github.com/jsfakian/Flaminio/issues/69), from the README roadmap (phase 6) and
ARCHITECTURE §17.2 and §18. This change is the part of the issue that can be built today: the
read-only identity of step 2, and the view of the cluster that step 1's dashboard will show, as
a command. No page is served yet: issue #69 stays open after this change.

## Why

1. **Nothing the dashboard is to stand on exists.**
   - No operator (#43; its first step makes no metadata call, so it holds no identity). No
     resource types: `api/` holds `proto` and `gen`. No Kubernetes client in `go.mod`.
   - No metrics endpoint in any component, and no `kubectl-flaminio` (no `cli/`). The issue's
     "done when" compares the dashboard with `kubectl flaminio status`.
   - No issue tracks the summarized resources (§17.2), the observability of §18 or the plugin.
2. **What the demo shows is not the cluster's state.** Its Grafana dashboard has three panels:
   Kubernetes nodes ready, a table of them, and `flaminio-meta up`
   (`deploy/demo/dashboards.yaml:23-35`). The last is a TCP connect by a Python exporter
   (`deploy/demo/status-exporter.yaml:43`), which holds no identity and so can read nothing.
   "Dataplane histograms are not wired yet" (`docs/demo.md:89`).
3. **The metadata service holds the cluster's state, and only part of what the issue lists.**
   - Nodes: id, address, labels, alive, draining (`meta.proto:63`). Volumes: size, chunk size,
     replicas, minimum ISR, epoch, target, and per chunk its replicas, ISR, version and requested
     move (`meta.proto:104-127`). Tombstones: the nodes named and those pending (`:169`).
   - No disk, no capacity, no rebuild progress: no record holds them (keys,
     `meta/service/service.go:4-13`).
   - A node says less. Its control socket's `status` answers the attached volume and its device
     (`flaminio-io/src/control.rs:104`), to root only (`:10`). The plans of #46, #47 and #62 add
     findings, latency and byte counts there, read on request. Nothing exports any of it.
4. **No command gathers it.**
   - `flaminio-meta` prints each RPC's answer as JSON (`meta/cmd/flaminio-meta/main.go:501`):
     `volumes` gives ids only, `get` one volume with every chunk, `tombstones` asks node by
     node (`:445`).
   - The lab has seven status targets (`vm-`, `ublk-`, `etcd-`, `meta-`, `node-`, `k8s-`,
     `csi-status`); none lists the volumes. `make vol-show` shows one, the test volume
     (`scripts/lab.sh:731`).
   - Before #14 added `tombstones`, the lab's stale ones were read from etcd, after they had
     stopped `metactl create wbench` (docs/issues/14, intent, Why 2 and 3).
5. **A view is many reads, not a snapshot.** `ListVolumes`, then one `GetVolume` each, as the
   drain does (`control/drain/drain.go:50-57`). A `GetVolume` is itself two reads, the header and
   then the chunks (`service.go:303-308`), and carries no revision. Liveness is a record that
   expires 10 s after a heartbeat and "never grants permission to serve I/O" (`meta.proto:21`).
   With the etcd quorum lost, no read answers at all.
6. **Health is defined nowhere, and computed three ways.**
   - The lab counts chunks whose ISR is smaller than their replica set (`vol_degraded`,
     `scripts/lab.sh:740`; again at `:1194`), and groups chunks by ISR (`:732`).
   - A requested move gives a chunk one more replica until its source is dropped: that count
     calls a moving chunk degraded.
   - The service never records an ISR below the minimum (`CheckIsrChange`,
     `meta/domain/domain.go:238`). So an ISR's size never says "unavailable" (§11.1, "ISR = ∅"):
     only its members' liveness can, and liveness is a hint.
7. **No identity can only read.**
   - Identities have two shapes, a node's component and an administrator
     (`pkg/pki/pki.go:233-238`; the data plane's parser has the same two,
     `flaminio-net/src/identity.rs:95-104`). Any administrator name may call every method but
     `ChangeIsr` (`meta/service/auth.go:57`).
   - `ListNodes`, `GetVolume` and `ListVolumes` answer any authenticated identity (`auth.go:79`);
     `ListTombstones` answers an administrator, or a node's agent for itself (`:75`).
   - So the whole view takes an administrator. In the lab that key is on fl-a and fl-w
     (`scripts/lab.sh:592`), and in the CSI controller's Secret (`:1004`). A dashboard would be
     a network listener holding it: "a pod that mounts the Secret is an administrator"
     (docs/issues/43, intent, Why 6).
   - A node identity with an unknown component is refused everywhere but those three reads. No
     rule says so, it is named after a node, and it cannot list tombstones.
8. **What the view shows is an administrator's.** Which node serves which volume, sizes,
   placement, node addresses. A volume's id is the name its creator gave it
   (`control/csi/controller.go:232`). §15.3 keeps such summaries in admin-only resources.
9. **A web page is a new toolchain.** ADR 0001 names Go and Rust, and nothing else.

## What we want

1. One command shows the cluster as the metadata service records it: the nodes (alive, draining,
   failure domain, what they hold and serve), the volumes (size, replication, minimum, epoch,
   target, health, moves in progress) and the tombstones pending, per node. For people, and in a
   form scripts can read.
2. Health is defined once, from the records: what each state of a chunk, a volume and a node
   means, and how a volume's follows from its chunks'. A move is not a fault. A state that rests
   on liveness says so. One piece of code derives it, for this command and for what comes
   later: the plugin, the resources' conditions, the metrics, the dashboard.
3. A read-only identity. It reads the nodes, the volumes and the tombstones. Every other method
   of the metadata service refuses it, those added later included; so does every replica. The
   command needs no other identity, and what it shows holds no key and no token.
4. The view never passes for more than it is. It says when it was read, and that it is not a
   snapshot. A metadata service that cannot answer is an error, never an empty cluster. What no
   record holds (disks, capacity, rebuild progress) is absent, and said to be. Nothing takes it
   as the ground for a fence, a failover, a deletion or a confirmation.
5. The lab shows its cluster with it, as the read-only identity: a stopped data node's chunks
   show as such and come back, and each changing command is refused to that identity.

## Out of scope

- **The dashboard itself** (step 1), and a browser authenticated through Kubernetes (the rest of
  step 2). It needs an operator that holds the read-only identity (#43, a later step), a
  listener beside the control plane, and a decision on the web toolchain (ADR 0001).
- **The summarized resources** (`Volume`, `Disk`, `StorageNode`; §17.2), their conditions and
  events. They need the operator; no issue tracks them.
- **Metrics and tracing** (§18). No issue tracks them. The demo's exporter and dashboards stay.
- **`kubectl flaminio`.** No issue tracks it. Its `status` will show this view; reaching the
  metadata service through Kubernetes waits for the operator.
- **Disks, capacity and rebuild progress.** They wait for records that do not exist.
- **What only a node knows:** its attached device, and what #46, #47 and #62 add to the control
  socket. The socket stays root-only, and nothing exports it.
- **New read RPCs:** a listing at one revision, or a summary the service computes. The existing
  ones do for the lab's size. Their caller rules change only to admit the read-only identity.
- **Narrowing what node identities read.** Any node still reads every volume's record.
- **Issuing the identity in a cluster** (#16, #43). The lab issues it with `make pki-issue`.
- **The lab's own waits** (`vol_degraded` and the like) stay as they are.
- **Destructive or changing actions** (step 3): the command has none.
