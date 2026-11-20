# #55 plan review, round 1, design-code and design-security

Scope: the whole plan (`docs/issues/55/{intent,spec,design,decisions}.md` at cd584f4, branch issue/55-compact-mode), checked against the code in /home/jsfakian/Documents/src/flaminio-55, the lab scripts, ARCHITECTURE, ADR 0004, and the #12 and #71 plans where #55 relies on them. Most line references hold (node.rs:134/140/331, ublk.rs:786/1116/1178/1263, net.rs:98/103/139/212, metaclient lib.rs:99, control.rs:53/60, types.rs:54, tombstones.go:9, topology.go:48, the 5 s failover interval, meta.rs:483, store.rs:555, lab.conf sizes); the Map-page format, the 64 MiB buffer arithmetic and the timers are correct.

## Correctness

**R1-C1 (High): fl-c is outside the inventory, and placement is not pinned to fl-a and fl-b.** S2 inventories "fl-a/fl-b/fl-w"; the matrix requires "no unrelated replicas". fl-c is a configured data node (`scripts/lab.conf:61-67`), in `data_vms` once `.lab/fl-c/created` exists (`scripts/lab.sh:1628-1630`), started by `delete_volume` (`lab.sh:1749`); the lab creates volumes with one preferred node (`lab.sh:704`, `:1897`), so RF2 spreads second replicas over fl-b and fl-c. Fix: preflight requires fl-c's node not running or not alive; assert every chunk's replicas are exactly {fl-a, fl-b} (`metactl get`); per-node slope from its own replica count; a fixture where fl-c holds a replica refuses.

**R1-C2 (Medium): the verdict's memory metric is not named.** "≤512MiB resident" and a verdict, while PSS, RSS and cgroup memory are kept and "never added". The unit form's cgroup holds only flaminio-io (`node.sh:65`); #12's pod cgroup also charges the 5 s exec probe and the `--privsep` helper (#12 spec:6, :48; design:110); zero-copy staging is memfd `MAP_SHARED` (`flaminio-uring/src/staging.rs:28-45`), i.e. shmem. Fix: name one metric per deployment form and whether shmem and file pages count; test the verdict on a fixture where PSS and cgroup memory disagree.

**R1-C3 (Medium): offline disktool inspection gives stale and coarse counts.** No SIGTERM handler, so `node.sh down` kills without a checkpoint (`node.sh:23-38`); `flaminio-disk inspect` reads bitmaps as of the last checkpoint and does not replay the journal (`flaminio-disktool/src/main.rs:320-367`); it reports "page pool used", not a map-page count; `Store::open` replays and checkpoints (`store.rs:467-468`). Fix: accept an inspection only with "to replay 0 batches", else restart with `FORMAT=0`, stop, inspect; record pool pairs; derive map pages from chunks × layers × pages per chunk; a non-empty-journal fixture yields "estimate".

**R1-C4 (Medium): the memory slope mixes map growth with retained heap.** Expected map delta ≈ 17.5 MiB; every write allocates and frees a copy buffer (`ublk.rs:1263`) under glibc malloc; map pages load eagerly at open (`store.rs:555`, `:2336`). Fix: a "fresh open, no device" capture at each population (restart with `FORMAT=0` after the R1-C3 inspection, before any attach) as the slope's primary evidence; order: capture, inspect, restart, capture fresh.

**R1-C5 (Medium): the expected product members are not defined, so S2's "missing members invalidate" cannot be tested.** The node agent is a one-shot CLI (`scripts/guest/agent.sh:10-13`); no operator; flaminio-meta on all three VMs (`lab.sh:509-525`); CSI controller and failover on fl-w (`deploy/csi/driver.yaml:18`, `:178`). Fix: a per-form table of expected members per VM with identity sources, tested; thread churn (control.rs:60, #12's probe) is not an identity change — only boot id, pid, start ticks; a fixture for it.

**R1-C6 (Medium): "no unrelated replicas" conflicts with "no deletion of existing data".** vol1 (and possibly wbench or CSI test PVCs) sit on fl-a and fl-b and load at open (`store.rs:555`). Fix: allow existing unattached idle replicas as the recorded known initial allocation, subtracted by the slope; forbid only other attached volumes, running fio and active PVC pods.

**R1-C7 (Medium): the architecture-wording instructions contradict each other, and one premise is false.** Intent keeps ARCHITECTURE's and ADR 0004's "from the start"; design says README/architecture must distinguish and "Current root audit already corrects feature-status wording", but `docs/ARCHITECTURE.md:82`, `:239` still say it. Fix: drop the audit claim; say what is edited (results note and README roadmap, or record the change of intent in decisions.md).

**R1-C8 (Low): zero-copy staging replaces the queue buffers.** `ublk.rs:1097-1127` is `if zero_copy { Staging } else { IoBuf × depth }`. Fix: "instead of the 64 MiB of anonymous buffers: a 64 MiB shared memfd (shmem)".

**R1-C9 (Low): per-process CPU from tick-sampled `/proc/PID/stat`.** Fix: `/proc/PID/schedstat` and task schedstat as the per-process source; cgroup `cpu.stat` `usage_usec` for totals; stat as a cross-check.

**R1-C10 (Low): the post-load warm-up job is not specified.** Fix: read-only jobs within the written extent, `bs=io_buf` (512 KiB), `iodepth=depth` (128), every queue; record the job.

**R1-C11 (Low): not feasible as written in four places.** `run_on_vm` pipes `common.sh` + script into bash (`scripts/lib.sh:152-158`) — name a runner (e.g. `sudo python3 -I -` over vssh, streaming stdout, no temp file); `make lint` runs no Python (`Makefile:84-94`) — name the test command; sampling every k3s thread and `smaps_rollup` each second nears the 64 MiB cap — sample k3s as cgroup totals at the window's ends; verifying state via the control socket wakes the engine (`control.rs:60`) — do it outside the window.

**R1-C12 (Low): no recovery or authorization mechanism.** OUT must be new, so an SSH-cut run cannot resume and the next preflight refuses its leftover volume; no lab lock; "explicitly authorized" names no mechanism. Fix: `MODE=cleanup OUT=<prior dir>` from the lifecycle manifest; `CONFIRM=matrix` (pattern of `lab.sh:487`); list the exclusivity checks (demo not active, no fio, no other attachments, no PVC pods) as check-then-act; test that the matrix without CONFIRM exits 2 with no remote mutation.

## Security

**R1-S1 (Medium): an inherited `FORMAT=1` can reformat a node.** `node_env` takes `FORMAT=${FORMAT:-0}` from the environment (`lab.sh:679`), node.sh adds `--format --force` (`node.sh:46`); make exports command-line variables; `delete_volume`'s second argument formats (`lab.sh:1745-1746`). Fix: set `FORMAT=0` explicitly on every `cmd_node up`, never pass delete_volume's format argument, pin other `node_env` settings from the recorded ExecStart; extend the S5 fake test to run with `FORMAT=1` and `ZERO_COPY=1` in the environment and assert no `--format` in any `systemd-run` argument list.

**R1-S2 (Low): capture mode uses the admin identity.** `metactl` defaults to `AS=admin` (`lab.sh:529-535`); GetVolume, ListNodes, ListVolumes allow any authenticated identity (`meta/service/auth.go:79-80`). Fix: capture reads as a non-admin identity; a fake test asserts capture never uses admin.

**R1-S3 (Low): artifacts can collect secret-adjacent data.** ExecStart recorded verbatim carries key paths (`scripts/guest/meta.sh:28-33`) and the token path (`node.sh:63`); `metactl attach` output contains the capability token (`lab.sh:1908`). Fix: ExecStart through the flag-name allowlist; never persist raw metactl output, only epoch, target and replicas; a fixture with a token and key paths, asserted absent.

**R1-S4 (Low): tenants control the names the inventory sees.** Fix: select by namespace `flaminio-system` plus owner-reference UID; treat every /proc or API string as data; a fixture pod `flaminio-csi-node-x` in a tenant namespace.

**R1-S5 (Low): the owned volume's identity is not validated, and its deletion is not fenced.** `delete_volume` deletes by id unconditionally (`lab.sh:1741`). Fix: generate the id under `valid_id` (`lab.sh:1633`) with a `cbudget-<ts>` prefix; record its first epoch at create (`lab.sh:620`); before deleting, `get` and refuse if the epoch differs or the id is missing; test with a recreated id.

## Nits
1. `docs/performance-plan.md` has no budget section: say "add".
2. Missing spaces in spec ("step2", "target1core/512MiB", "Current100GiB", "at512MiB", "because1vCPU") and design ("≤1.0core", "a1TiB/1.28GiB", "Exit0").
3. Startup log `eprintln!` is at node.rs:268-271, not :267.
4. Name tracefs (`/sys/kernel/tracing`, per-CPU `stats` overrun) as the wake-up source; perf is not installed by `provision.sh`.
5. "operator if deployed": none exists yet; say so or point to #43.
