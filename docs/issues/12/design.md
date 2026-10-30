# Issue #12: the data plane in Kubernetes — design

The change adds:
- a DaemonSet;
- two subcommands (`check`, `status`) and a flag (`claim --no-format`) of `flaminio-agent`;
- in `flaminio-io`: `status` reporting an attach in progress, and a SIGTERM handler;
- in the CSI node plugin: parsing that answer;
- the lab steps that replace `make node-up`.

The other control commands, the claim policy and the on-disk format are unchanged.

## The DaemonSet (S1)

**The manifest:** `deploy/dataplane/dataplane.yaml`, rendered by `scripts/lab.sh` like
`deploy/csi/driver.yaml`, with `{{IMAGE}}`, `{{META_ENDPOINTS}}`, `{{TRUST_DOMAIN}}` and
`{{SERIAL}}`.
- **Kind:** `DaemonSet` `flaminio-dataplane` in `flaminio-system`, with its service account
  `flaminio-dataplane` and `automountServiceAccountToken: false`.
- **Node selector:** `node-restriction.kubernetes.io/flaminio-dataplane: pod` and
  `flaminio.io/storage: "true"`.
- **Updates:** `updateStrategy: {type: OnDelete}`.
- **Pod:**
  - `hostNetwork: true`, `dnsPolicy: ClusterFirstWithHostNet`, and `hostPID` and `hostIPC`
    false;
  - `priorityClassName: system-node-critical`;
  - `terminationGracePeriodSeconds: 3600`.
- **Tolerations,** each `operator: Exists`:
  - `node.kubernetes.io/out-of-service` (`NoExecute`);
  - `node.kubernetes.io/not-ready` and `unreachable` (`NoExecute`);
  - `node.kubernetes.io/unschedulable` (`NoSchedule`);
  - `node.kubernetes.io/memory-pressure`, `disk-pressure` and `pid-pressure` (`NoSchedule`).
- **The init container `claim`:** privileged, `readOnlyRootFilesystem`. It runs:

  ```sh
  install -d -m 700 /run/flaminio
  flaminio-agent check
  flaminio-agent claim --serial {{SERIAL}} --no-format > /claim/device
  ```

  `/claim` is an `emptyDir`.
- **The container `dataplane`:** privileged, `readOnlyRootFilesystem`. Both containers have
  `imagePullPolicy: Never`.
  - **Its port:** `ports: [{containerPort: 7411, hostPort: 7411}]`.
  - **Its command:** `sh -c 'exec flaminio-io node --device "$(cat /claim/device)" ...'`. The
    `exec` makes it PID 1. Its flags:
    - `--node $(NODE_NAME)` (`spec.nodeName`);
    - `--listen $(HOST_IP):7411` (`status.hostIP`);
    - `--meta {{META_ENDPOINTS}}`, `--pki /etc/flaminio/pki`, `--trust-domain {{TRUST_DOMAIN}}`;
    - `--control /run/flaminio/node.sock`.

    There is no `--id`, `--format` or `--force`.
  - **Readiness probe:** `exec: flaminio-agent status`, every 5 s.
  - **preStop:** `exec: sh -c 'until flaminio-agent status --none; do sleep 2; done'`.
- **Volumes:**
  - **hostPath:**
    - `/dev` (`Directory`);
    - `/run/udev` (`Directory`, read-only, `claim` only);
    - `/run/flaminio` (`DirectoryOrCreate`);
    - `/etc/flaminio/pki/{io.crt,io.key,ca.crt,token.pub}` (`File`, read-only).
  - **emptyDir:** `/claim`, and `/run/ublksrvd` (libublk's `flush_json`, `ctrl.rs`).

**`flaminio.io/storage` and the CSI node plugin:** unchanged. The plugin has no tolerations
(`driver.yaml`), so the out-of-service mark keeps it off.

## The image (S1, S7)

**`scripts/dev/dataplane-image.sh`** builds it with `image.sh`, as `csi-image.sh` does for the
CSI image:
- `flaminio-io` and `flaminio-disk`: from a data VM's release build (`make deploy`), copied back
  as `fio_image` copies fio;
- `flaminio-agent`: built on the operator's machine (`GOOS=linux`), as `cmd_agent` does.

**The base:** `image.sh` pins it by digest (`ubuntu:resolute-20260912@sha256:...`), for both
images.

**The tag is content-derived:** `docker.io/flaminio/dataplane:<first 16 hex digits>` of the
sha256 of:
- each binary's base name and sha256;
- the base's digest;
- `PLATFORM_ARCH`;
- `image.sh` itself.

It is computed before the build, since the tarball's own digest is not known while its tag is
written (`image.sh`).
- **Why a tag:** `k3s ctr -n k8s.io images import` names the image by its tag only, so the
  template names that tag.
- **After the import,** `lab.sh` lists the tag with `k3s ctr -n k8s.io images ls` on each pod
  node, and stops if it is missing.
- **Same inputs, same tag:** so an unchanged build changes no template.
- **The CSI image** keeps its dated tag, and its pods restart on each deploy, as now.

## Checks and claim (S2)

**`flaminio-agent check`** (`control/agent/kernel.go`) refuses, naming what is missing:
- `uname -r` below 6.6;
- no `/dev/ublk-control`;
- `/proc/sys/kernel/io_uring_disabled` other than `0`, or missing.

Its tests take the version and a root directory as arguments.

**`claim --no-format`:**
- **The verdict:** `Pick` as now. A `Claimable` result (a blank disk) is refused: "blank disk
  /dev/X: format it first (make csi-deploy formats blank disks)".
- **For `Ours`:** the claim opens the device with `O_EXCL`. On `EBUSY`, it tries again every
  second, logging each try, until the device is free.

## The socket (S3)

**The directory** (`control.rs`, `spawn`, safe `std` only):
1. It creates the directory if missing.
2. It opens it with `O_DIRECTORY | O_NOFOLLOW` (`OpenOptionsExt`).
3. It checks the owner against the process's euid (`File::metadata`, an fstat), so that the
   tests run as the VM user.
4. It sets the mode to 0700 (`File::set_permissions`, an fchmod), before `bind`.

A symlink, or another owner, is refused.

**Attach state:** `Attached` becomes three states: `None`, `Attaching(volume)` and
`Attached(volume, device)`.

**One helper, `begin_attach`** (`control.rs`), builds every attach request: for `serve`, and
for the startup `--attach` and its retry thread (`ublk.rs`). In one acquisition of the lock, it:
1. refuses unless the state is `None` ("already attaching" or "already attached");
2. sets `Attaching(volume)`;
3. then sends `Control::Attach`. If the send fails, it sets `None` again.

It calls `wake()` after releasing the lock: `wake()` takes the engine sender's mutex
(`ublk.rs`).

**Only the main thread** moves `Attaching` on: to `Attached`, or to `None` on an error or a
dropped request.
- So the state stays `Attaching` after the caller's own limit has passed, until the main thread
  ends the attach.
- **The startup retry thread:**
  - a refusal because the state is `Attaching` is retried like a failed attempt;
  - a refusal because it is `Attached` stops the thread, as `done()` does.
- `detach`'s wait loop (`control.rs`) waits for `None`.

**`status`:**
- `ok VOLUME /dev/ublkbN` once attached;
- `ok attaching VOLUME` while attaching;
- `ok none` otherwise.

An attached answer's second word always starts with `/dev/`. The parser tells the two forms
apart by that word.

**The CSI node plugin** (`dataplane.go`, `Status`) parses the new form.
- **`device`:** it returns `Unavailable` while attaching. So does the raw-block branch of
  `NodePublishVolume`.
- **An old plugin with a new data plane** reads `attaching VOLUME` as volume `attaching`, and
  answers `ResourceExhausted`. Kubelet retries, and the deploy updates both. No volume is named
  `attaching`: CSI volume ids are `pvc-<uuid>`.

**`node.sh`:**
- **Is a data plane running:** it tests whether the socket answers, not the unit (`attach`,
  `delete`, `status`). `up` refuses where the socket answers and the unit is not active.
- **Token files:** `up` with `ATTACH` writes them with `install -m 600`. `detach` and `down`
  delete `/run/flaminio/<volume>.token`.
- **The token** still travels on command lines, as today (decisions, accepted risks).

**`node.sh down` and `ublk.sh down`** delete a ublk device only while their own unit is active,
and only that unit's id (1 and 0). Where the socket answers but the unit is inactive, they
refuse.

**`ublk.sh up`** refuses where the socket answers, or `/dev/ublkc0` already exists. Its unit is
active before `serve` fails, so an active unit does not prove it owns the id.
- **A device left by a killed `serve`:** when the unit is inactive and no socket answers, the
  refusal prints `flaminio-io del --id 0` to run.
- **Why:** both used to delete their id whatever owned it.
- **The mount check misses binds:** `findmnt` does not see a raw block volume bind-mounted for
  a pod.

**Pod Security:**
- `k3s.sh server` writes `/etc/rancher/k3s/psa.yaml`: `defaults.enforce: baseline`, with
  `exemptions.namespaces: [kube-system, flaminio-system]`.
- It adds `pod-security-admission-config-file: /etc/rancher/k3s/psa.yaml` to `config.yaml`.
- **PersistentVolumes are not checked.** A hostPath PV at `/run/flaminio` mounts in a
  `baseline` namespace. These can create PVs:
  - the cluster's administrators;
  - the CSI controller's service account, `flaminio-csi-controller` (`rbac.yaml`).

  This is an accepted risk.

## Lifecycle (S4)

**SIGTERM.** A module `sigterm` goes in `flaminio-uring`, where `unsafe` is allowed;
`flaminio-io` forbids it.
- **`install()`:** blocks SIGTERM, and starts the `sigwait` thread that sets a static
  `AtomicBool` (below).
- **`requested()`:** safe.

**The handler.** `install()` runs first in `node::main`, before that subcommand starts any
thread.
- **Why not in `main.rs`:** `main.rs` dispatches `replica`, `digest`, `node`, `serve` and
  `del`, and only `node` checks `should_exit`. `ublk.sh down` stops `serve` with
  `systemctl stop`, so `serve` must keep SIGTERM's default action.
- **The mask:** it blocks SIGTERM in the calling thread's mask, which threads started after it
  inherit.
- **The flag:** a dedicated thread runs `sigwait` and sets it.

No thread is interrupted, so ublk control commands never see `EINTR` (libublk ignores
`submit_and_wait`'s errors, `ctrl.rs`).

**When it exits:** `should_exit(requested, state)`, a pure function, is true when SIGTERM was
requested and the state is `None`.
- **The main thread's loop** (`ublk.rs`, after `drive_until`) checks it on each turn. When
  true, it sends `Input::Stop`, removes the socket, and exits 0.
- **While attached,** the main thread is inside `attach_and_serve` and checks nothing. So a
  SIGTERM while attached makes the process exit after the next detach.
- **Nothing is logged** for an ignored SIGTERM.

**The preStop hook:** `flaminio-agent status --none` exits 0 when no socket answers, so a pod
deleted before its data plane is up does not wait 3600 s.

**`make k8s-down` and `make k8s-uninstall`:**
- they refuse when a pod node's `status` (through `kubectl exec`) names an attach, attached or
  attaching;
- on no answer, they proceed;
- `FORCE=1` skips the check.

**`make csi-undeploy`:** it deletes `flaminio-test` first. It then refuses while any
VolumeAttachment of `csi.flaminio.io` remains.

**Leaked ublk ids:** the kernel picks a free id. A dead device stays until the node reboots.

## The ordered upgrade (S5)

**`make node-upgrade`** runs `cmd_node_upgrade` in `lab.sh`, on the cluster's `kubectl`
(`kubectl_lab`). Every `kubectl` call has `--request-timeout=30s`.
- **The newest revision:** the `ControllerRevision` with the highest `.revision` among those
  whose owner reference is the DaemonSet `flaminio-dataplane`. A pod is at it when its
  `controller-revision-hash` label matches.
- **The order:** pod nodes without a VolumeAttachment of `csi.flaminio.io`, then the others,
  by name.

**Per node, as S5:**
- **Cordon:** `kubectl cordon`, if `.spec.unschedulable` was false. The result is kept in a
  shell variable.
- **Every ISR full:** a new helper, `served_isr_full`. `wait_isr_full` checks only `$VOL`.
  The helper:
  1. lists the volumes (`metactl ... volumes`);
  2. finds the served ones, from the VolumeAttachments and from each data node's `status`;
  3. checks every chunk's ISR against its replicas.
- **`minIsr`:** from `metactl get`: for each chunk with a replica on the node,
  `(isr | length) - 1 >= minIsr`.
- **Controllers:** each pod using a PVC bound to a `csi.flaminio.io` PV whose VolumeAttachment
  names the node has an `ownerReferences` entry with `controller: true`, whose owner exists.
- **Room:** another uncordoned `flaminio.io/storage=true` node with no VolumeAttachment.
- **Evict:** each pod goes through the Eviction API
  (`kubectl create --raw /api/v1/namespaces/NS/pods/POD/eviction`), so that only those pods go.
  `kubectl drain` is not used.
- **Wait for nothing attached:** `flaminio-agent status --none` through `kubectl exec`, and no
  VolumeAttachment for the node, polled every 2 s for up to 300 s.
- **Step 5's re-checks,** none of which waits:
  - `status --none`;
  - no VolumeAttachment for the node;
  - every ISR full, and `minIsr`;
  - the data-plane pod's UID and revision are those read at step 1.
- **Delete:**
  `kubectl delete --raw /api/v1/namespaces/flaminio-system/pods/POD -f -
  --request-timeout=30s`.
  - Its body, on stdin, is a `DeleteOptions` with `preconditions.uid`. No temporary file is
    left on fl-w.
  - A pod replaced since the re-check is not deleted.
- **Then** it waits for a pod on the node at the newest revision to be Ready (300 s), then for
  `served_isr_full` (900 s). On a time-out, it reports whether the old pod is still
  `Terminating` and serving.

**Refusals:**
- **At step 2,** it uncordons what it cordoned.
- **Later,** it leaves the node cordoned, and prints the `kubectl uncordon NODE` to run once the
  cause is fixed. A rerun does not uncordon a node it did not cordon itself.
- **An interrupted run:** an `EXIT` trap uncordons a node the run cordoned, if it was still at
  steps 1–2.

## The k3s agents (S7)

**The agent token:** `k3s.sh server` writes `agent-token:` into `config.yaml` once. The secret
is kept in `/etc/rancher/k3s/agent-token`, mode 0600.

**`k3s.sh token`** prints it in the full form `K10<CA hash>::node:<secret>`, which keeps the CA
pinning.

**On the agents:**
- `k3s.sh agent` writes `token:` into `config.yaml` before the install;
- `install_k3s` no longer passes `K3S_TOKEN`. On its command line, sudo logs it, and the
  installer writes it into the service's environment file;
- it removes `K3S_TOKEN` from `/etc/systemd/system/k3s-agent.service.env`;
- it restarts the agent when either changed.

**The token still crosses the ssh command line to the agents,** in `ENV_VARS`. It is visible in
`ps` for a moment. sudo no longer logs it.

**Modes:** `config.yaml` and `agent-token` are 0600 before any secret is written. Today's
`sudo tee` creates them 0644 (`k3s.sh`).
- **An absent file** is created with `sudo install -m 0600 /dev/null FILE`.
- **An existing one** is first read, for the restart decision's comparison, then set with
  `chmod 600`, then written.

**When the configuration changes:** `install_k3s` returns early while the right version runs.
So `server` and `agent` compare the configuration they write with the old one, and restart k3s
when it differs. A restart of the service leaves the pods running.

## The lab (S7, S8)

**`cmd_csi deploy`,** when not `demo_active`:
1. **Claim:** `cmd_agent claim` on fl-a and fl-b, which formats only a blank disk.
2. **The unit:** `node.sh down` where the unit runs. It refuses where `status` names a volume.
3. **The image:** build it, import it on fl-a and fl-b, and read its digest.
4. **Labels:** `kubectl label node fl-a fl-b node-restriction.kubernetes.io/flaminio-dataplane=pod`.
5. **The DaemonSet:** render and apply it. `kubectl rollout status` fails for `OnDelete`, so it
   polls until `.status.observedGeneration` equals `.metadata.generation` and `numberReady`
   equals `desiredNumberScheduled` (300 s).
6. **The driver:** as now.

**The other commands:**
- **`csi_failover`:** it waits for the data-plane pod on the lost VM to be Ready, instead of
  `cmd_node up`.
- **`cmd_csi undeploy`:**
  1. the test namespace;
  2. the refusal of Lifecycle;
  3. the DaemonSet;
  4. the labels.
- **`docs/lab.md`:** the CSI section without `make node-up`, and the two kinds of data node.

## Tests

**Go:**
- **`control/agent`:**
  - **`check`:** version 6.5, no `/dev/ublk-control`, `io_uring_disabled` at 1, at 2 and
    missing, and all present;
  - **`claim --no-format`:** a blank disk is refused. With a fake prober, `EBUSY` twice, then
    the device.
- **`control/cmd/flaminio-agent`,** against a socket in a temporary directory:
  - **`status`:**
    - exits 0 on `ok none`, `ok attaching v` and `ok v /dev/ublkb3`;
    - exits 1 with no socket;
    - exits 2 on `error x`;
  - **`status --none`:** exits 0 on `ok none` and with no socket, and non-zero on either
    attach form.
- **`control/csi`:**
  - `Status` on `ok attaching v` gives an attach of `v` in progress;
  - `Status` on `ok attaching /dev/ublkb3` gives volume `attaching`, attached at
    `/dev/ublkb3`;
  - `device`, and the raw-block `NodePublishVolume`, return `Unavailable` while attaching.
- **`deploy/` (a new `deploy/manifests_test.go`):** renders `dataplane.yaml` and `driver.yaml`
  with placeholder values, and checks every field listed under "The DaemonSet" above:
  - `/run/flaminio` mounted only by `flaminio-dataplane` and `flaminio-csi-node`;
  - the data plane's `--control` equal to the node plugin's `--control-socket`;
  - `out-of-service` tolerated by the data plane, and not by the node plugin.

**Rust:**
- **`flaminio-uring`, `tests/sigterm.rs`:** declared as `[[test]]` with `harness = false`.
  - Its own `main` calls `install()` before anything else, so libtest's threads, which would
    take the signal, are never started.
  - It then sends `kill(getpid(), SIGTERM)`: `requested()` becomes true, and the process lives
    on.
  - **`unsafe`:** the crate denies it. The file has `#![allow(unsafe_code)]`, and a `// SAFETY`
    comment on the `libc::kill` call.
  - **Off Linux,** where `libc` is not a dependency: the Linux body is under
    `#[cfg(target_os = "linux")]`. A stub `main` does nothing under
    `#[cfg(not(target_os = "linux"))]`, so the harness-less target still builds.
- **`flaminio-io`, `should_exit`:** true only for SIGTERM requested with the state `None`.
  False for `Attaching` and for `Attached`, and true once the state goes back to `None`: an
  exit after the next detach.
- **`flaminio-io`, `control.rs`:**
  - with a fake engine that answers after the caller's limit, `status` answers
    `ok attaching v` after the caller's error, until the late answer. It then answers
    `ok none` on an error, or `ok v /dev/...` on success;
  - a second attach while `Attaching` is refused, and so is one while `Attached`;
  - two concurrent attaches: exactly one is sent to the engine;
  - the directory is 0700 after `spawn`;
  - a symlinked directory, and one with another owner, are refused.

**shellcheck:** the changed scripts.

**Lab**, the spec's list:
1. **From the unit to pods.**
   - **Setup:** `make node-down`, `make k8s-up` (on the existing lab), `make deploy`,
     `make csi-deploy`.
   - **k3s:** it was restarted. On fl-a and fl-b, `grep -r` searches `/etc`,
     `/var/lib/rancher`, `/etc/systemd` and the journal since the restart:
     - the server token's secret is in none of them;
     - the agent token's secret is only in 0600 files;
     - the configuration files are 0600.
   - **Kubernetes refuses:**
     - a kubelet's own label (`kubectl --kubeconfig` with the node's credentials);
     - a privileged pod, and a hostPath pod, in a new namespace;
     - namespace creation and relabelling by a non-admin service account (`kubectl auth
       can-i`).
   - **Accepted, and shown:** a hostPath PV at `/run/flaminio`, created by the administrator,
     mounts in a `baseline` namespace.
   - **The image:** two builds of the same binaries give the same manifest digest. Both nodes
     list the tag.
   - **The pods:** `hostPID` is false, and both pods are Ready.
2. **The claim:** the pod's `claim` log against `make agent-disks VM=a` and `VM=b`, same
   `--serial`. Same disk, and the same state per device.
3. **`make csi-failover`.**
4. **An ordered upgrade.**
   - **The load:** `deploy/lab/upgrade-load.yaml`, in the namespace `flaminio-upgrade`
     (`restricted`): a StatefulSet of one pod with a PVC. It runs fio with `verify=crc32c`,
     `verify_state_save=1` and `verify_state_load=1`, the state on the PVC, writing
     continuously.
   - **Setup:** its pod on fl-a, then a rebuilt `flaminio-io`, `make csi-deploy`, and
     `make node-upgrade`.
   - **fl-b:** the load pod's UID and restart count are unchanged, and fio logs no error.
   - **fl-a:** the pod is Ready on fl-b within 300 s, and fio verifies the saved state.
   - **The data-plane pods:** both at the newest revision, with restart count 0.
   - **A second `make csi-deploy`** with the same build changes no revision.
5. **The refusals,** each from a clean lab, with its own setup:
   - **#15:** a PVC pod on each node. It refuses at fl-a's step 2.
   - **Outside Kubernetes:** vol1 attached by `make vol-attach VM=a`, with no load pod. It
     refuses at fl-a's step 4, and fl-a stays cordoned.
   - **PodDisruptionBudget:** `minAvailable: 1` on the load StatefulSet. It refuses at fl-a's
     step 3.
   - **No controller:** a bare pod with a PVC on fl-a. It refuses at step 2.
   - **RF 1:** an RF 1 volume whose chunks are on fl-b, served from fl-a, with no load pod. It
     refuses at fl-b's step 2.
   - **`minIsr`:** an RF 2, `minIsr: 2` volume served from fl-a. It refuses at fl-b's step 2.
   - **ISR not full:** the load on fl-a, and fl-b's data-plane container killed during fl-a's
     step 4, after the eviction. The step-5 check finds an ISR not full, and refuses at once.
   - **An administrator's cordon:** fl-a cordoned before the run. It stays cordoned after.
6. **Deletions while attached,** `make csi-test` with `KEEP=1` on fl-a:
   - **The label removed:** the pod is `Terminating` and keeps serving. It is gone within 10 s
     of the test pod's move (cordon and delete).
   - **The DaemonSet deleted:** the same.
   - **SIGTERM by hand** to `flaminio-io`: the volume stays served.
   - **Refusals:** `make k8s-down`, `make k8s-uninstall` and `make csi-undeploy`.
7. **A crash.**
   - **Setup:** `crictl stop -t 0` on the data-plane container, with the test pod on fl-a.
   - **The data plane:** fl-a is alive again. A few container restarts are allowed, as the
     claim does not rerun.
   - **The test pod:** deleted, with fl-b cordoned, it is Ready on fl-a with "verified on
     fl-a", at a higher epoch.
8. **One data plane per node:** `node.sh up` on fl-a refuses.
9. **The units' `down`s,** with `make csi-test` `KEEP=1` on fl-a: `make node-down` and
   `make ublk-down` on fl-a refuse, and the test pod's volume stays attached.
10. **Token files:** after `make vol-attach` with the unit (fl-c), the token file is 0600. It
    is gone after `node.sh detach`, and, after a new attach, after `node.sh down`.
