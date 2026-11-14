# Issue #12: the data plane in Kubernetes — decisions

These are settled. Each line gives the choice, then the reason.

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- None: `spec.md`, `design.md` and this file leave no open question, and no choice is labelled
  Proposed. The plan depends on ADR 0012 (`docs/adr/0012-privileged-data-plane-pod.md`; 0007
  until round 1 of the plan review, R1-C14), which is still Proposed; accepting it is a
  separate step.

## Scope

- **"Keeps a pod with a PVC running"** means two things. A pod runs without an I/O error while
  other nodes upgrade. On its own node's upgrade, it is evicted and restarts elsewhere with its
  data. Running through its own node's restart needs ublk user recovery.
- **ublk user recovery** is a new issue, whose text is in the intent. It needs the epoch
  re-taken through the agent identity, and its device state on the host.
- **#15 and #17 are not needed.** Only a pod's move needs a free node, and no replica moves.
- **The pod never formats.** `make csi-deploy` formats only blank disks, on the host. A restart
  can then never create a new store for a node the cluster knows.
- **The Mac demo is unchanged,** and keeps `make node-up`. It is a separate setup.
- **Benchmarks and checks keep the systemd unit,** and need `make csi-undeploy` on fl-a and
  fl-b. They read the unit's journal and flags.
- **Out of scope:**
  - separating the metadata members from the data nodes;
  - privilege separation in the pod: adopting #74's helper there is #12's follow-up
    (follow-ups §9), with #74's list of needs (changed after approval, R1-C13, R1-S1);
  - #16;
  - a registry;
  - CPU pinning;
  - rotating the k3s server token.

  Each is a follow-up.

## Shape

- **One DaemonSet pod, `flaminio-dataplane`,** holds the agent's checks and claim (init) and
  the data plane. The claim's result is needed only at start.
- **The CSI node plugin keeps its own DaemonSet.** Its upgrades must not restart the data
  plane.
- **The socket path is unchanged** (`/run/flaminio/node.sock`), shared through the host. The
  node plugin, `node.sh` and the checks already use it.
- **Pod nodes are chosen by a `node-restriction.kubernetes.io/` label.** Kubelets cannot set
  it, and the label lets a node go back to the unit.
- **Tolerations are explicit:** out-of-service, not-ready, unreachable, unschedulable, and the
  pressures. Out-of-service is needed so the failover mark can be lifted; other taints keep
  their meaning.
- **The CSI node plugin does not tolerate out-of-service,** so nothing is staged on a node
  under the mark.
- **Device ids are chosen by the kernel.** Leaked ids are reclaimed at reboot. A crash's
  leftover then cannot block an attach, and no cleanup code is needed.
- **There is no dead-device cleanup in the pod,** for the reason above. User recovery will need
  those devices. The unit scripts delete a dead device at their own id, as #74 needs, only once
  the pod-node guard passes (changed after approval, R1-C1, R1-S1).
- **`hostPort` 7411 is declared,** so a successor cannot run beside its predecessor.
- **`flaminio-io` does not retry `EBUSY` on the disk.** The claim does, and a restarted
  container is restarted again by Kubernetes.
- **An `emptyDir` at `/run/ublksrvd`:** libublk writes there, and the root filesystem is
  read-only.
- **`terminationGracePeriodSeconds: 3600`** bounds the preStop wait, which is long enough to
  move a pod by hand.
- **SIGTERM:** the data plane exits at once with nothing attached or attaching. Otherwise it
  exits after the next detach. Pod deletions then end in seconds, and never stop a served
  volume early. After a SIGTERM, `begin_attach` refuses every attach (changed after approval,
  R1-C9, R1-S4).
- **The handler lives in `flaminio-uring`,** which allows `unsafe`, unlike `flaminio-io`.
  `should_exit` is a pure function, so it can be tested.
- **`status` reports an attach in progress.** An attach that is queued or running counts as
  attached for the upgrade, the preStop hook and SIGTERM.
- **Attaching lasts until the main thread ends the attach,** not until the caller's limit. A
  late attach must not look like none.
- **One attach at a time, checked and set under one lock** (`begin_attach`), for the socket
  and the startup attach alike. Only the main thread ends an attach. Concurrent attaches would
  otherwise both be sent. Its refusal names the volume that holds the node (`already attaching
  V`, `already attached to V`), as main's did (changed after approval, round 4, R4-M1).
- **`ok attaching VOLUME`** is told apart from an attached answer, whose second word starts
  with `/dev/`. An old CSI plugin answers `ResourceExhausted`, which kubelet retries. No volume
  is named `attaching` (`pvc-<uuid>`).
- **SIGTERM is blocked, and taken by a `sigwait` thread,** so no ublk control command is
  interrupted.
  - `install_sigterm()` runs first in `node::main`, not in `main.rs`: the other subcommands,
    `serve` among them, keep SIGTERM's default.
  - Its test is a harness-less test binary, as libtest's threads would take the signal.
- **`status --none` passes only on `ok none`, a missing path, `ENOENT` or `ECONNREFUSED`,** so
  a preStop hook does not wait 3600 s for a data plane that never came up, and a hung, failing
  or malformed one counts as attached (changed after approval, R1-S3).
- **The preStop hook signals `flaminio-io` (PID 1) before it waits,** so no attach is taken
  between its `ok none` and kubelet's SIGTERM (changed after approval, round 2, R2-B1). The
  window is closed, not accepted. The `FailedPreStopHook` event it leaves is expected (round 3,
  R3-N3). In `serve`, the shutdown check comes before any other attach check (round 3,
  R3-C3).
- **`k8s-down`, `k8s-uninstall` and `csi-undeploy` ask the socket over ssh, not through
  Kubernetes** (`pod_nodes_idle`). They proceed only on `absent` or `none`, and `FORCE=1` skips
  the check.
  - With Kubernetes down, a live data plane still blocks them.
  - A dead data plane does not.
  - They ask fl-a and fl-b, skipping a VM whose unit is active, whose socket is the unit's
    (changed after approval, R1-C10, R1-C3).
- **`detach` while attaching answers an error,** which the plugin maps to `Unavailable`. It does
  not wait for the attach to end: that is simpler, and kubelet retries.
- **A sentinel subcommand, `node.sh socket`,** prints one line:
  - `absent`, only for no path, `ENOENT` or `ECONNREFUSED`;
  - `none`;
  - `answer`, only for an answer of the grammar, read within 4096 bytes;
  - `error`, which includes any time-out (5 s) and a malformed answer, with a reason from a
    fixed set (changed after approval, R1-S8); `refused` names a well-formed `error ...`
    answer, never `ECONNREFUSED` (round 2, R2-N4).

  A hung data plane must never read as absent, and an ssh failure, which looks like silence,
  refuses.
- **The pod-node guard needs the socket absent (or the `flaminio-node` unit active), and, where
  k3s is installed, a `crictl` that succeeds with no ready data-plane sandbox in
  `flaminio-system` labelled `app=flaminio-dataplane`.**
  - A stopped data plane still blocks the units' `up`.
  - So does k3s stopped, as its shims keep containers running.
  - A tenant's pod of the same name elsewhere does not (changed after approval, R1-S9).
  - The raw-disk benchmarks run it too (changed after approval, R1-C11), except a
    `fio-raw.sh` run whose jobs are all reads on an already preconditioned disk, which writes
    nothing: `make bench-pod`'s comparison (changed after approval, round 2, R2-C2).
- **The units' scripts delete a ublk device only at their own id, and only once the guard
  passes;** then a device left is dead. `node.sh down` detaches its unit's volume through the
  socket before the stop, and deletes leftovers after it (changed after approval, R1-C1,
  R1-S1; S4 restated). They used to delete ids 1 and 0 whatever owned them.
- **The kernel checks are those of ADR 0003,** `io_uring_disabled` must be 0 included. The ADR
  stays as it is.
- **`hostPID` is false.** The kernel checks the ublk server's pid in its own namespace. The
  fallback is `hostPID: true`.

## Upgrade

- **It is an ordered upgrade, with `OnDelete` and `make node-upgrade`, not `RollingUpdate`.**
  `RollingUpdate` ignores attached volumes, and counts a pod available before resync.
- **The steps:** cordon, checks, evict, a bounded wait for nothing attached, checks again
  without waiting, delete, a bounded wait for Ready and full ISRs, uncordon. It is simple, and
  a rerun starts over.
- **No hold, no instance id, no nonce, no annotations.** Their machinery carried most of the
  findings. The bounded waits and the re-check are enough for the lab.
- **The upgrade uncordons only if it cordoned.** An administrator's cordon survives.
- **A refusal after the eviction leaves the node cordoned,** and prints the
  `kubectl uncordon` to run. A rerun does not uncordon what it did not cordon.
- **Step 5's re-checks:**
  - `status --none`;
  - no VolumeAttachment;
  - ISRs full, and `minIsr`;
  - the same pod UID and revision.

  The delete carries a UID precondition. This closes the gap between the check and the
  deletion, without a hold.
- **The checks:** RF 1, every ISR full, ISR − 1 ≥ `minIsr`, and room for an evicted pod. They
  are what an eviction or a restart needs. All of them cover served volumes only: an unserved
  volume has no I/O to lose, and unserved RF 1 volumes would block every upgrade.
- **Nodes without a Flaminio volume go first,** which saves moves.
- **"Needs an upgrade"** is the pod's `controller-revision-hash` against the newest revision.
  A change to the manifest alone rolls out too.
- **The image is named by a content-derived tag,** from its inputs: the binaries, the base's
  digest and `image.sh`. It is computed before the build, as the tarball's digest is not known
  yet. `ctr import` names by tag only, and an unchanged build then changes nothing.
- **The pod is deleted with a UID precondition** (`kubectl delete --raw` with `DeleteOptions`),
  not through a proxy.
- **The k3s token never goes on an installer's command line.** It goes in `config.yaml`
  (0600), written before the install.
- **`kubectl rollout status` is not used:** it fails for `OnDelete`. The deploy polls
  `observedGeneration` and `numberReady` instead.
- **No `O_EXCL` change in `flaminio-disk format`:** no rule needs it.

## Security

- **The data-plane pod is privileged** (ADR 0012). There is no privilege separation yet.
- **Pod Security:** a k3s default of `baseline`, with only `kube-system` and `flaminio-system`
  exempt. Baseline refuses every hostPath and privileged pod.
- **No admission policy at all,** neither a ValidatingAdmissionPolicy nor a PV check. The
  policy was bypassable. Baseline covers pods, but not PersistentVolumes: PV creators are an
  accepted risk.
- **The socket is unauthorized beyond its mode:** `detach` and `delete` take no token, as
  today. Whoever reaches it can write the raw disk anyway. A token on `detach` could wedge a
  volume, and the lab cleans leftovers with `delete`.
- **The CSI node plugin stores no token.**
- **`node.sh` writes token files 0600, and deletes them on `detach`, and on `down` once no data
  plane runs, a crashed unit included** (changed after approval, R1-C18, security Nit), with
  the glob expanded as root (round 2, R2-N3), and skipped when `/run/flaminio` is missing
  (round 3, R3-C1). The directory is 0700.
- **PKI:** one read-only `Directory`, `/etc/flaminio/io-pki`, holding only the data plane's four
  files, so that #16's rotation by rename reaches the pod (changed after approval, R1-C5; it
  was four `File` mounts). It limits mistakes; the boundary is the host.
  - Once #74 has landed it is `root:flaminio-io` 0750 with `io.key` 0640, so the unit's
    unprivileged I/O process can read it; the pod reads it as root (round 2, R2-C1).
  - Whichever of #12 and #16 lands second puts `io` in it in #16's per-name `pki.sh` map, so
    #16's renewal writes there (round 2, R2-C1).
  - `cmd_csi deploy` and the paths that run `node.sh up` issue `io` there when it is missing,
    as for `agent.crt` (round 2, R2-C4); `node.sh up` itself dies naming `make pki-issue`
    when it is still missing, for every other caller (round 3, R3-C4), first, before it stops
    a running unit (round 4, R4-L3).
  - Only `ca.crt` and `token.pub` are copied there; `pki.sh install io` removes the two old
    files by name (round 3, R3-B1, R3-N1).
- **The k3s agents join with a separate agent token,** not the server's (cluster-admin) token.
  It is in the full `K10<CA hash>::node:<secret>` form, which keeps the CA pinning, and its
  files are 0600.

## Accepted risks

- **A compromised data-plane pod has the node, and in the lab the cluster.** The pod nodes hold
  the token key, the meta and etcd keys, and the administrator's key on fl-a. Fixing it needs
  separate metadata nodes, and it may block any use beyond the lab.
- **The k3s server token was on the data nodes** until this change, and its copies stay in their
  logs (the journal, `/var/log/auth.log`), valid until it is rotated. Rotation is out of scope
  (changed after approval, R1-S6).
- **Capability tokens appear in `/proc/*/cmdline`** on the VMs, as today, and so does the k3s
  agent token on ssh's command line for a moment. Any local user can read them: the VMs mount
  `/proc` without `hidepid` (`scripts/templates/user-data.yaml`). Accepted for the lab
  (corrected after approval, R1-S7: it said root-equivalents).
- **Who reaches the socket:**
  - host root;
  - pods in `kube-system` and `flaminio-system`;
  - anyone who may create or relabel namespaces, `exec` or `attach` into `flaminio-system`
    pods, use `nodes/proxy`, update the DaemonSets, or create HelmChart objects;
  - the privileged CSI node plugin.

  All of them are administrators, in the lab.
- **The failover controller's service account may patch nodes,** including the pod-node label
  and taints. It needs to, for its taint.
- **A compromised data plane can read the kubelet's credentials** and label its node a witness,
  drawing the controllers and their admin identity. It already has the cluster's keys here.
- **The image is built on a data VM** (`flaminio-agent` on the operator's machine). A
  compromised VM can poison it. There is no registry or signing yet.
- **A deleter that shortens the grace period** bypasses the preStop wait. That is a crash under
  the volume.
- **PersistentVolumes:** a hostPath PV reaches the socket from any namespace. PVs are created
  by the cluster's administrators and by `flaminio-csi-controller`.
- **A reboot under an attached volume is a crash.** The lab's k3s has no graceful node
  shutdown.
- **Concurrent `make node-upgrade` runs** are not locked. One run at a time is documented.
- **Attaches outside Kubernetes during an upgrade** (`node.sh`, `make vol-attach`) are not
  stopped. The bounded wait and the re-check catch most; the rest is a crash risk the lab
  accepts.

## Review

- **The review loop ended with only Lows,** all fixed in the last revision.

## Plan review round 1 (2026-11-14), made after approval

Fixes of round 1 of this batch's review (reviewed commit 6f70e98), made after the approval of
2026-11-13. Each changes the plan in place; none is an approval.
- **R1-C1, R1-S1, R1-C13 (#74):** #12 now names #74 and agrees with its split (design, "#74,
  the privileged helper").
  - The pod stays in direct mode, as #74's intent foresees; adopting the helper in the pod is
    #12's follow-up (follow-ups §9), which owns running it there, with #74's list of needs
    (intent, Out of scope; ADR 0012). R1-S1's option (b) was taken: `make csi-deploy` keeps a
    unit mode (`DATAPLANE=unit`), so #74's evidence still runs. It is the smaller change and
    keeps privilege separation out of this plan, while option (a) would make #12 depend on #74
    and reopen the device-id and PID 1 choices (a fixed id whose dead device a volume still
    holds would block the pod's start, against S4).
  - The unit scripts delete a ublk device only at their own id, and only once the pod-node
    guard passes, whichever plan lands second; #74's leftover deletion and "`node.sh up` runs
    `down` first" are kept. `node.sh down` detaches its unit's volume through the socket
    before the stop: the SIGTERM handler would otherwise hold the stop, and under #74's helper
    a root `del` of the owned device while the unit runs is unsupported. `ublk.sh up`'s
    refusal of a leftover `/dev/ublkc0` is cut: #74 deletes it once the guard passes.
  - **S4 restated:** "the units' `down` never deletes a ublk device its own unit does not
    serve" cannot hold with #74's leftover deletion, which must delete a dead device no unit
    serves. The rule now says what it protects: no device a live data plane serves is ever
    deleted, which the guard ensures.
  - The socket's directory rules are one function, `control::bind`, which #74's helper also
    calls once both have landed.
- **R1-C2:** the scripts are described as on `origin/main` (8aaa7a3). `running()` counts a pod's
  data plane, `detach` included; exit 3 means no data plane at all; `vol-delete`,
  `detach_all` and `detached_everywhere` on pod nodes are stated, unchanged. Every new shell
  rule has a case in `node-scripts-test.sh` or the new `lab-k8s-test.sh`.
- **R1-C3:** `make csi-undeploy` waits, bounded, for the test namespace's VolumeAttachments,
  then asks the sockets as `k8s-down` does, then deletes the DaemonSet, the driver and the
  labels. Lab 6 holds the attach outside `flaminio-test`.
- **R1-C4, R1-S5 (quoting):** the template values are checked against a grammar in `lab.sh`,
  and reach the containers only as `env`; the scripts are constant, run with `sh -ec`, and
  quote them. `ATTACH` and `VOLUME` are checked before any path is built. The ServiceAccount
  is in `deploy/csi/rbac.yaml`, as #43 requires. Exec form was not used for the claim: its
  output goes to a file, and R1-C7 asks for one `sh -ec` script; env vars quoted in `sh` are
  the reviewers' other option.
- **R1-C5:** the data plane's PKI is one read-only directory, `/etc/flaminio/io-pki`, holding
  only its four files, used by the unit too. A dedicated directory keeps S2 ("no other key").
- **R1-C6:** lab 5's RF 1 and `minIsr` cases are served from a controller-owned PVC pod on
  fl-a, so fl-b goes first; the ISR case kills fl-b's container while the load pod is still
  Terminating and writing (a 30 s `preStop` in the load).
- **R1-C7:** both scripts run with `sh -ec`.
- **R1-C8:** the euid comes from a safe `euid()` in `flaminio-uring`'s new `process` module,
  and `control::bind` takes the owner as an argument, for the tests.
- **R1-C9, R1-S4:** `begin_attach` refuses with `the node is shutting down` once a SIGTERM is
  requested; unit test added.
- **R1-C10:** `served_isr_full` asks every VM of `data_vms` (fl-c included) and refuses when one
  cannot be asked; `pod_nodes_idle` asks fl-a and fl-b and skips a VM whose unit is active.
- **R1-C11:** the pod-node guard runs in `fio-raw.sh` and `store-bench.sh`.
- **R1-C12:** the base's digest is the multi-arch index digest.
- **R1-C14:** the ADR is renumbered 0012 (`git mv`), as the orchestrator decided: the audit's
  Proposed ADRs keep 0007–0011.
- **R1-C15:** the Makefile's help, a `node-upgrade` target and `usage()` entry,
  `docs/lab.md` and `docs/followups.md` §9 are in the design.
- **R1-C16:** S3 lists every protocol change.
- **R1-C17:** `detach V` waits until the state no longer names V.
- **R1-C18, security Nit:** `down` deletes the token files once no data plane runs, a crash
  included; S3 says what deletes them and what leaves them.
- **R1-C19:** `startup_next` is a pure function with a unit test; the `ublk.sh up` refusal it
  asked a script test for is cut (R1-C1), and its replacement, the guarded leftover deletion,
  has script tests.
- **R1-S2:** k3s takes Pod Security through `kube-apiserver-arg:
  ["admission-control-config-file=..."]` with an `AdmissionConfiguration`, per docs.k3s.io
  (Security, Hardening Guide, "Pod Security"; CLI, `k3s server`, which lists no admission
  option), for k3s v1.25 and newer, so for the pinned `v1.36.5+k3s1`. `k3s.sh server` checks
  the result with a server-side dry run in `default`: a dry run needs an existing namespace.
- **R1-S3:** `status --none` exits 0 only on `ok none`, a missing path, `ENOENT` or
  `ECONNREFUSED`, with its own 5 s bound.
- **R1-S6:** S7 and ADR 0012 say the server token's old copies stay in the agents' logs until it
  is rotated, and lab 1 searches the whole journal and `/var/log`. The rule as stated ("no
  longer kept on the data nodes") could not hold for those copies. Purging the logs was not
  chosen: it would delete every other entry, and the token stays valid anyway.
- **R1-S7:** the accepted risk is corrected to "any local user". The reviewer's other options
  (`hidepid`, tokens on stdin) change more than this plan needs.
- **R1-S8:** the new readers of the data plane's answers read at most 4096 printable bytes and
  accept only the grammar; the CSI plugin's bound is #74's, added by whichever lands first.
- **R1-S9:** the `crictl` filter adds `--namespace flaminio-system --label
  app=flaminio-dataplane`; script and lab tests with a decoy.
- **N1:** the exit no longer "sends `Input::Stop`". **N2:** the deploy no longer reads the
  image's digest. **N3:** `flaminio-uring/src/lib.rs` and ARCHITECTURE §15.5 name the new
  module.

## Plan review round 2 (2026-11-14), made after approval

Fixes of round 2 of this batch's review (reviewed commit 74979ae; plan content at 082c8f0),
made after the approval of 2026-11-13. Each changes the plan in place; none is an approval.
- **R2-C1:** a PKI bullet under "#74, the privileged helper": once #74 has landed, `io-pki` is
  `root:flaminio-io` 0750 with `io.key` 0640, as #74's `node.sh up` and `pki.sh:10` would set
  `/etc/flaminio/pki`, which can then stay 0700. The pod reads it as root in either mode, so S2
  holds. One rule for #16: whichever of #12 and #16 lands second puts `io` in `io-pki` in
  #16's one-directory-per-name `pki.sh`, so #16's renewal by rename reaches the pod and the
  unit. `pki.sh install io` removes the old `/etc/flaminio/pki/io.*`. Script cases: `pki.sh`'s
  routing and removal, `file ca.crt|token.pub` to both directories, and `node.sh up`'s
  `--pki /etc/flaminio/io-pki`.
- **R2-C2:** the reviewer's first option. `fio-raw.sh` skips the guard when every selected job
  is a read and the disk is already preconditioned, since then it writes nothing and the guard
  exists for writes. `make bench-pod` keeps working on pods; S8 and the intent say so; lab 12
  and a script case added. The second option (`bench-pod` needs `DATAPLANE=unit`) would take
  the benchmark off the default deployment.
- **R2-C3:** `csi_failover` reads the mode from the cluster (the DaemonSet `flaminio-dataplane`
  exists or not), not from `DATAPLANE`, which only `csi-deploy` reads. #74's evidence becomes
  `DATAPLANE=unit make csi-deploy`; whichever of #12 and #74 lands second writes it into #74's
  lab plan and `docs/lab.md`. Lab 11 and a script case added.
- **R2-C4:** `cmd_csi deploy`, `cmd_node up`, `cmd_data_node_up` and `ensure_node_c` issue `io`
  into `io-pki` (with `ca.crt` and `token.pub`) when `io-pki/io.crt` is missing, as `cmd_csi
  deploy` does for `agent.crt`; `docs/lab.md` says so. Lab 13 and a script case added.
- **R2-B1:** the reviewer's first option, so nothing new is accepted. The preStop hook runs
  `kill -TERM 1` before its wait, so `begin_attach` refuses from the hook's start. `flaminio-io`
  is PID 1 of the container (its script `exec`s it; `hostPID` false; `shareProcessNamespace`
  unset), and a blocked SIGTERM reaches a namespace's init. S4 says a deletion stops new
  attaches from its start. A manifest case and a lab case (lab 6) added.
- **R2-S1:** `node.sh attach` validates `VOLUME`; `control()` reads at most 4096 printable
  bytes, accepts only its command's forms, and `node.sh` prints `malformed` instead of an
  answer outside them; `attach` waits only for a `/dev/ublkbN` path. Script cases added.
- **R2-C5:** script cases for `down`'s refusal while attaching, `node.sh up`'s `--pki`, the
  `pki.sh` routing, and `k3s.sh server` stopping when its dry run is admitted. The spec's
  sentence is narrowed to the rules the script tests cover; the other S7 rules stay lab tests.
- **R2-C6:** the test overrides (`RUN_DIR`, `PKI_ROOT`, `K3S_CONFIG_DIR`) and the stubs
  (`modprobe`, `systemd-run`, `journalctl`, `lsblk`, `findmnt`, `mountpoint`, `k3s`, `curl`,
  `fio`) are named, honoured only under `FLAMINIO_SCRIPT_TEST=1`.
- **R2-N1:** ADR 0012 says the fixed id "must be reconciled with" the crash rule. **R2-N2:**
  intent says `node.sh up` runs `down` first when the unit is active. **R2-N3:** `down` deletes
  token files with `sudo find`, expanded as root. **R2-N4:** `socket: error refused` names a
  well-formed `error ...` answer.

## Plan review round 3 (2026-11-14), made after approval

Fixes of round 3 of this batch's review (reviewed commit 0165a65; plan content at cbe3d62),
made after the approval of 2026-11-13. Each changes the plan in place; none is an approval.
- **R3-C1:** `down`'s token deletion is `[[ ! -d /run/flaminio ]] || sudo find ...`, so a
  missing directory (tmpfs after a reboot, a fresh fl-c) no longer fails `down` under
  `set -e`. Script case: `RUN_DIR` missing, exit 0, the leftover `del` still runs.
- **R3-C2:** the `pki.sh` case checks only the modes `pki.sh` sets (`io.key` 0600; `io-pki`
  0700, or 0750 once #74 has landed). The 0640 and the `flaminio-io` group belong to #74's
  `node.sh up` and are checked in its case, from the `sudo` stub's record.
- **R3-C3:** the reviewer's fix: in `serve`, `begin_attach`'s checks replace `serve`'s own check
  of the attached state and come first, the shutdown check before the state check, then the
  token and the limit. Unit case: attached to `v`, the flag set, `attach w` answers `the node
  is shutting down`. Lab 6's expectation then holds. The other option (lab 6 expecting
  `main`'s answer) would leave the order unpinned.
- **R3-C4:** the reviewer's first option, the simplest safe one: `node.sh up` dies naming
  `make pki-issue` when `io-pki/io.crt` is missing, which covers every caller, including
  `scripts/checks/reconcile.sh` and `pvc-loop.sh`, with one script case. The lab.sh paths keep
  issuing it; `cmd_data_node_up` and `ensure_node_c` join the lab-k8s-test case.
  `pvc-loop.sh` drives the units under the driver, so it runs with `DATAPLANE=unit`.
- **R3-B1:** negative cases added: `file token.key` and `file etcd-ca.crt` stay out of
  `io-pki`, which holds exactly its four files after `cmd_pki issue`'s sequence; each of
  `RUN_DIR`, `PKI_ROOT` and `K3S_CONFIG_DIR` is ignored without `FLAMINIO_SCRIPT_TEST=1`, and
  the `sudo` stub refuses the real paths so a fallback cannot touch the runner;
  `socket: error refused` on an `error x` answer; `socket: absent` on a stale socket file.
- **R3-N1:** `pki.sh install io` removes `/etc/flaminio/pki/io.crt` and `io.key` by name, as
  root. **R3-N2:** whichever of #12 and #74 lands second changes #74's lab-node line and
  `pki.sh:10` to `io-pki`. **R3-N3:** the `FailedPreStopHook` event is stated beside the
  preStop and in lab 6. **R3-N4:** `cmd_bench_pod`'s warning names when to precondition
  (before the first `make csi-deploy`, or after `make csi-undeploy`, then a `FORMAT=1` node-up),
  and lab 12's setup does so.

## Plan review round 4 (2026-11-14), made after approval

Fixes of round 4 of this batch's review (reviewed commit 3008b06; plan content at 596b7cb),
made after the approval of 2026-11-13. Each changes the plan in place; none is an approval.
- **R4-L1 (Medium):** lab 12's setup starts, after `make csi-undeploy`, with `make node-down` on
  fl-a and fl-b, so `make bench-raw VM=b` writes beneath no live store and the final
  `make csi-deploy` finds no unit; then `bench-raw`, `FORMAT=1 make node-up VM=b`,
  `make node-down VM=b`, `make csi-deploy`. Volumes made before it lose their fl-b replica, so
  it runs after the labs that use them, or on a fresh lab.
- **R4-L2:** the "overrides without the switch" case for `node.sh down` asserts what can fail:
  the token planted in the temporary `RUN_DIR` survives, and the `sudo` stub records no command
  naming it. The stub's refusal stays the check for `pki.sh csr io` and `k3s.sh server`.
- **R4-L3:** `node.sh up` checks `io.crt` first, with the pod-node guard, before `systemctl
  is-active && down`, so a refusal changes nothing; the script case checks that a running unit
  stays active.
- **R4-M1:** `begin_attach`'s step 2 names the volume: `error: already attaching V` and
  `error: already attached to V`; S3 says so. New unit case: attached, a token that is not
  base64: `already attached to v`, so the state is checked before the token.
- **R4-N1:** the `sudo` stub matches a substring of any argument, so `sudo sh -c "..."` and
  `sudo env ...` are caught. The `systemd-run` exemption is dropped: under the overrides its
  arguments name only temporary paths.
