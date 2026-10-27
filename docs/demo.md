# Investor / colleague demo

MacBook runs the **Kubernetes control plane** (no workloads). Storage workers are the existing
lab VMs: **fl-a on rhythm (Site A)** and **fl-b on rhythm-backup (Site B)**. etcd and
`flaminio-meta` stay on fl-a / fl-b / fl-w as today; fl-w is **not** the k3s server during the
demo.

k3s is Linux-only, so the “Mac CP” is a **Lima** guest (`flaminio-cp`) on this machine, joined
to the lab WireGuard mesh as `10.77.0.10`. From the audience’s seat: the laptop is the control
plane.

## Prerequisites

On this Mac:

```bash
brew install bash lima kubectl helm jq
```

Lab already healthy: overlay, etcd, meta, data plane (`make mesh-check`, `make node-up`).

You need this Mac’s address **as the lab hosts see it** (campus or VPN):

```bash
export DEMO_CP_SRC_IP=…          # required for UDP forwards to the VMs
# optional override if WG is not on SRC_IP:51825
export DEMO_CP_ENDPOINT=$DEMO_CP_SRC_IP:51825
```

## Bring-up

```bash
make demo-up                     # Lima + WG peer + k3s server; fl-a/fl-b join; fl-w k3s removed
export KUBECONFIG=$PWD/.lab/kubeconfig
make csi-deploy                  # Flaminio CSI (builds amd64 for workers + arm64 for Lima CP)
make demo-obs                    # Prometheus, Grafana, status exporter, scoreboard
make demo                        # talk track
DEMO_AUTO=1 SCENARIOS=0,1,2,7,8 make demo   # unattended smoke (skip node-kill / partition)
```

`csi-deploy` builds the CSI image twice when the demo marker is set: amd64 for fl-a/fl-b,
arm64 for the Lima control plane (Apple Silicon).

Optional live Longhorn comparison:

```bash
make demo-longhorn
LONGHORN=1 make demo             # scenario 8 runs bench-compare
# or: make demo-compare
```

Tear down:

```bash
make demo-down
# restore previous lab k3s on fl-w:
RESTORE_LAB_K8S=1 make demo-down
# also delete Lima:
DESTROY_LIMA=1 RESTORE_LAB_K8S=1 make demo-down
# drop Mac UDP forwards:
unset DEMO_CP_SRC_IP && make net-setup
```

## Talk track (~25–35 min)

| # | Scenario | What to say | Expect |
|---|---|---|---|
| 0 | Topology | Two physical servers, CP on the laptop, NoSchedule | 3 Ready nodes |
| 1 | Provision | CSI dynamic volume RF2 | PVC Bound, pod Ready |
| 2 | Durable write | Checksum on the volume | `created` / `verified` in logs |
| 3 | Kill Site B | Replica loss; ISR shrinks; restart rejoins | App stays up |
| 4 | Kill Site A | Target loss; failover controller | Pod on other site ~2 min, data verifies |
| 5 | Partition | Epoch / lease fencing | `make partition-test` story |
| 7 | Perf honesty | Scoreboard vs raw | Reads strong; 1M write gap open |
| 8 | Competitors | Longhorn peer; Mayastor later; Ceph wrong weight | Scoreboard + optional live |

Skip 6 (drain) unless fl-c is up.

## Visibility

```bash
kubectl -n monitoring port-forward svc/monitoring-grafana 3000:80
# user admin; password: cat .lab/demo/grafana-admin-password
kubectl -n flaminio-demo port-forward svc/scoreboard 8080:80
```

Exporter: `flaminio_meta_up` (TCP to meta on the overlay). Dataplane histograms are not wired yet;
use canned panels from [benchmarks](benchmarks/) for IOPS/bandwidth.

## Claims guardrails

**Do say:** raw devices, ISR + epoch fencing, meta off the I/O path, CSI failover we just timed,
honest vs-raw numbers from this lab.

**Do not say:** encryption shipped, independent AZ (fl-w shares rhythm with fl-a), 1M write at
80%+ of raw, Longhorn numbers from blogs, Mayastor/Ceph “wins” without running them.

See [demo-competitors.md](demo-competitors.md).
