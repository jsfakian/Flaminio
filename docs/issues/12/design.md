# Issue #12: the data plane in Kubernetes — design

The change adds:
- a DaemonSet;
- two subcommands (`check`, `status`) and a flag (`claim --no-format`) of `flaminio-agent`;
- in `flaminio-io`: `status` reporting an attach in progress, and a SIGTERM handler;
- in the CSI node plugin: parsing that answer;
- the lab steps that replace `make node-up`.

The other control commands, the claim policy and the on-disk format are unchanged.

**The base.** The scripts are described as they are on `origin/main` (8aaa7a3): `node.sh detach`
and `delete` exit 3 with `not running` when the unit is inactive, `delete` and `detach`
validate `VOLUME` (`volume_id`), `control()` is in `common.sh`, and
`scripts/tools/node-scripts-test.sh` checks them. The Rust and Go files this change touches
(`control.rs`, `dataplane.go`, `node.go`, the agent) are unchanged on `main`.

## #74, the privileged helper

#74's step 1 (approved, not implemented) changes the same scripts and socket code. Whichever of
#12 and #74 lands second applies these rules; neither plan undoes the other.
- **The pod runs direct mode** (`flaminio-io node`, no helper), as #74's intent foresees until
  the pod adopts the helper. Adopting it is #12's follow-up (follow-ups §9; intent, Out of
  scope), with #74's list of needs. ADR 0012 names #74's ADR as its successor.
- **`make csi-deploy` keeps a unit mode** (`DATAPLANE=unit`, The lab), so #74's evidence still
  runs: `make node-up` under the helper, then **`DATAPLANE=unit make csi-deploy`** and
  `make csi-test`. A plain `make csi-deploy` would replace the helper units with direct-mode
  pods. Whichever of #12 and #74 lands second writes that command into #74's lab plan and
  `docs/lab.md`.
- **The PKI** (The DaemonSet, "The PKI directory"): under `PRIVSEP=1`, the unit's I/O process
  runs as the system user `flaminio-io` and reads `--pki` itself. So once #74 has landed,
  `/etc/flaminio/io-pki` is `root:flaminio-io` 0750 with `io.key` 0640: #74's `node.sh up` and
  `pki.sh:10` apply their modes to `io-pki` instead of `/etc/flaminio/pki`, which can stay
  0700. Whichever of #12 and #74 lands second changes #74's lab-node line (the `chgrp` and
  `chmod` of the directory and `io.key`) and `pki.sh:10` to `io-pki`; otherwise #74's
  `node.sh up` would `chgrp` an `io.key` #12 has removed, and stop. The pod reads `io-pki` as
  root in either mode, and S2 still holds.
- **Deleting ublk devices:** #74's leftover deletion (`down` deletes a remaining
  `/dev/ublkc$id` after stopping the unit; `ublk.sh up` deletes a leftover `/dev/ublkc0`) runs
  only after the pod-node guard passes (The socket). The merged rules are under "`node.sh down`"
  and "`ublk.sh`" below; #74's "`node.sh up` runs `down` first" is kept.
- **The socket's directory:** one function, `control::bind`, makes the directory, checks it and
  binds (The socket); `control::spawn` calls it in direct mode. #74's helper binds at its step
  3 "as `control.rs` does": whichever of #12 and #74 lands second moves the function into
  `flaminio-helper`'s library, which `flaminio-io` already uses for `node_params`, and the
  helper calls it (its owner from `rustix`'s `geteuid` there). The directory is then root's and
  0700 in both modes. `--privsep` hands `spawn` the bound listener, as #74 states.
- **SIGTERM:** `install_sigterm()` runs in `node::main`, so the I/O process under `--privsep`
  has the handler too. Under the helper, `systemctl stop` signals both processes, and the I/O
  process exits when the helper goes (#74's watcher).

## The DaemonSet (S1)

**The manifest:** `deploy/dataplane/dataplane.yaml`, rendered by `scripts/lab.sh` like
`deploy/csi/driver.yaml`, with `{{IMAGE}}`, `{{META_ENDPOINTS}}`, `{{TRUST_DOMAIN}}` and
`{{SERIAL}}`.
- **Values:** `lab.sh` checks each value's grammar before rendering, and stops otherwise:
  - `IMAGE`: `^docker\.io/flaminio/dataplane:[0-9a-f]{16}$`;
  - `META_ENDPOINTS`: `^[0-9.]+:[0-9]+(,[0-9.]+:[0-9]+)*$`;
  - `TRUST_DOMAIN`: `^[a-z0-9]([a-z0-9.-]{0,253}[a-z0-9])?$`;
  - `SERIAL`: `^[A-Za-z0-9._-]{1,64}$` (a regular expression for `flaminio-agent`, here limited
    to characters with no shell or YAML meaning).

  Each goes only into a double-quoted YAML scalar: an `env` value or an image name, never into
  a shell script. The scripts below are constant text, and read the values from the
  environment, quoted (`"$SERIAL"`).
- **Kind:** `DaemonSet` `flaminio-dataplane` in `flaminio-system`, with its service account
  `flaminio-dataplane` and `automountServiceAccountToken: false`. The ServiceAccount is in
  `deploy/csi/rbac.yaml`, with no Role or binding, as #43 requires; the pods carry
  `app: flaminio-dataplane`.
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
- **The init container `claim`:** privileged, `readOnlyRootFilesystem`, `env: SERIAL`
  (`"{{SERIAL}}"`). It runs `sh -ec` with this constant script, so a failed line stops it:

  ```sh
  install -d -m 700 /run/flaminio
  flaminio-agent check
  flaminio-agent claim --serial "$SERIAL" --no-format > /claim/device
  ```

  `/claim` is an `emptyDir`.
- **The container `dataplane`:** privileged, `readOnlyRootFilesystem`. Both containers have
  `imagePullPolicy: Never`.
  - **Its port:** `ports: [{containerPort: 7411, hostPort: 7411}]`.
  - **Its environment:** `META` (`"{{META_ENDPOINTS}}"`), `TRUST_DOMAIN`
    (`"{{TRUST_DOMAIN}}"`), `NODE_NAME` (`spec.nodeName`), `HOST_IP` (`status.hostIP`).
  - **Its command:** `sh -ec` with this constant script; the `exec` makes `flaminio-io` PID 1:

    ```sh
    read -r DEVICE < /claim/device
    exec flaminio-io node --device "$DEVICE" --node "$NODE_NAME" --listen "$HOST_IP:7411" \
      --meta "$META" --pki /etc/flaminio/io-pki --trust-domain "$TRUST_DOMAIN" \
      --control /run/flaminio/node.sock
    ```

    It has no `$(...)`, which Kubernetes would try to expand. There is no `--id`, `--format`
    or `--force`.
  - **Readiness probe:** `exec: flaminio-agent status`, every 5 s.
  - **preStop:** `exec: sh -c 'kill -TERM 1; until flaminio-agent status --none; do sleep 2;
    done'`. It signals first, so `begin_attach` refuses from the start of the hook, not only
    from kubelet's SIGTERM after it (Lifecycle). With nothing attached, or once the detach
    comes, the signal ends PID 1, and the hook's own `sh` dies with the container: kubelet then
    records a `FailedPreStopHook` warning event. That event is expected on such deletions,
    not a failure.
  - **PID 1:** `flaminio-io` is PID 1 of the container's PID namespace: the script `exec`s it,
    `hostPID` is false, and `shareProcessNamespace` is unset. The preStop hook runs in that
    namespace, so `kill -TERM 1` reaches it. A namespace's init drops a signal it has no
    handler for, but `flaminio-io` blocks SIGTERM in every thread and takes it with `sigwait`,
    and a blocked signal is never ignored (kernel `sig_ignored`), so the signal is queued and
    taken.
- **Volumes:**
  - **hostPath:**
    - `/dev` (`Directory`);
    - `/run/udev` (`Directory`, read-only, `claim` only);
    - `/run/flaminio` (`DirectoryOrCreate`);
    - `/etc/flaminio/io-pki` (`Directory`, read-only, same path; `dataplane` only).
  - **emptyDir:** `/claim`, and `/run/ublksrvd` (libublk's `flush_json`, `ctrl.rs`).

**The PKI directory** `/etc/flaminio/io-pki` (root, 0700; `root:flaminio-io` 0750 once #74
has landed) holds only the four files `--pki` reads: `io.crt`, `io.key` (0600; 0640 group
`flaminio-io` once #74 has landed), `ca.crt` and `token.pub`.
- **Why a directory:** a file replaced by rename, as #16's rotation does, reaches a pod only
  through a directory mount. A single-file mount keeps the old inode.
- **Why its own:** `/etc/flaminio/pki` also holds `token.key`, the `meta` and `etcd` keys, and
  on fl-a the administrator's key (S2: no other key).
- **The lab:** `pki.sh` keeps `io` there (`csr`, `install`), and `file` writes `ca.crt` and
  `token.pub` to both directories; every other name, `token.key` and `etcd-ca.crt` included,
  goes to `/etc/flaminio/pki` only. `pki.sh install io` then removes the two old files by name,
  as root (`sudo rm -f /etc/flaminio/pki/io.crt /etc/flaminio/pki/io.key`): a glob of the
  calling shell would expand as the VM user, who cannot list the 0700 directory.
  `cmd_pki issue` issues `io` that way. The unit's `node.sh up`
  passes `--pki /etc/flaminio/io-pki` too, so the pair exists once. `metactl` with `AS=io`, and
  `cmd_meta_smoke`'s etcd refusal, read it there.
- **#16:** its approved `pki.sh` keeps one directory per name (`meta-pki`, `agent-pki`, the rest
  in `pki`). Whichever of #12 and #16 lands second puts `io` in `io-pki` in that map, with the
  copies of `ca.crt` and `token.pub`, so #16's renewal by rename writes there and the pod and
  the unit pick it up. #16's "Paths" contract then names `io-pki`, and its renewal keeps
  `io.key`'s mode and group.
- **An existing lab:** `cmd_csi deploy` and the lab.sh paths that run `node.sh up`
  (`cmd_node up`, `cmd_data_node_up`, `ensure_node_c`) check for `/etc/flaminio/io-pki/io.crt`
  on the VM, as `cmd_csi deploy` already does for `agent.crt`. Where it is missing, they copy
  `ca.crt` and `token.pub` (`put_file`, which writes both directories) and issue `io` there,
  before the unit starts or the DaemonSet is applied. `docs/lab.md` says so.
- **Every other caller** of `node.sh up`, such as `scripts/checks/reconcile.sh` and
  `scripts/checks/pvc-loop.sh`, which run it directly: `node.sh up` itself checks
  `sudo test -f /etc/flaminio/io-pki/io.crt` and dies naming `make pki-issue`. That covers any
  caller with one rule. The check comes first in `up`, with the pod-node guard, before
  `systemctl is-active && down` (or #74's `down` that always runs), so a refusal changes
  nothing: a running unit stays running. (`pvc-loop.sh` restarts the units
  under the CSI driver, so it runs with `DATAPLANE=unit`, as S8 says of checks that drive the
  unit.)

**`flaminio.io/storage` and the CSI node plugin:** unchanged. The plugin has no tolerations
(`driver.yaml`), so the out-of-service mark keeps it off.

## The image (S1, S7)

**`scripts/dev/dataplane-image.sh`** builds it with `image.sh`, as `csi-image.sh` does for the
CSI image:
- `flaminio-io` and `flaminio-disk`: from a data VM's release build (`make deploy`), copied back
  as `fio_image` copies fio;
- `flaminio-agent`: built on the operator's machine (`GOOS=linux`), as `cmd_agent` does.

**The base:** `image.sh` pins it by digest (`ubuntu:resolute-20260912@sha256:...`), for both
images. The digest is the multi-arch index's, not a platform manifest's, so the demo's arm64
CSI image (`GOARCH=arm64`, `cmd_csi deploy`) still resolves it.

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

**The directory:** one function, `control::bind(path, owner) -> io::Result<UnixListener>`
(`control.rs`), which `spawn` calls in direct mode; #74's helper uses it too (#74, above). With
safe `std` only:
1. It creates the directory if missing.
2. It opens it with `O_DIRECTORY | O_NOFOLLOW` (`OpenOptionsExt`).
3. It checks the directory's owner against `owner` (`File::metadata`, an fstat).
4. It sets the mode to 0700 (`File::set_permissions`, an fchmod).
5. It removes the socket's path, binds, and sets the socket to 0600, as `spawn` does today.

A symlink, or another owner, is refused. **The owner:** callers pass the process's euid, from
`flaminio_uring::process::euid()` (Lifecycle): `std` has no euid, and `libc::geteuid` needs
`unsafe`, which `flaminio-io` forbids. The tests pass their own euid, or another one to see
the refusal, so they run as the VM user.

**Attach state:** `Attached` becomes three states: `None`, `Attaching(volume)` and
`Attached(volume, device)`.

**One helper, `begin_attach`** (`control.rs`), builds every attach request: for `serve`, and
for the startup `--attach` and its retry thread (`ublk.rs`). In `serve`, its checks replace
`serve`'s own check of the attached state (`V is attached here; one volume at a time`,
`control.rs:85–96`), and come first, before the token's base64 and the limit. In one
acquisition of the lock, it:
1. refuses once a SIGTERM was requested (`error: the node is shutting down`). It reads the
   flag through a predicate its caller passes: `process::sigterm_requested` in production, a
   test's own flag in the tests;
2. refuses unless the state is `None`, naming the volume that holds the node:
   `error: already attaching V` or `error: already attached to V`. Main's text named it too
   (`V is attached here; one volume at a time`), and people and lab scripts read it to see
   which volume holds the node;
3. sets `Attaching(volume)`;
4. then sends `Control::Attach`. If the send fails, it sets `None` again.

So no attach starts between a SIGTERM and the exit, whether it comes before the main loop's
next check or after the detach a remembered SIGTERM waits for.

It calls `wake()` after releasing the lock: `wake()` takes the engine sender's mutex
(`ublk.rs`).

**Only the main thread** moves `Attaching` on: to `Attached`, or to `None` on an error or a
dropped request.
- So the state stays `Attaching` after the caller's own limit has passed, until the main thread
  ends the attach.
- **The startup retry thread:** a pure function, `startup_next(&refusal)`, decides:
  - a refusal because the state is `Attaching`, or a failed send, is retried like a failed
    attempt;
  - a refusal because it is `Attached`, or because the node is shutting down, stops the
    thread, as `done()` does.
- `detach V`'s wait loop (`control.rs`) waits until the state no longer names V. A
  `begin_attach` of W right after V's device stops does not make it wait through W's attach.
- **`detach V` while `Attaching(V)`** answers `error: V is attaching`. Today, "is not attached
  here" counts as success for the plugin (`dataplane.go`). The CSI node plugin maps the new
  error to `Unavailable`, and kubelet retries the unstage.

**`status`:**
- `ok VOLUME /dev/ublkbN` once attached;
- `ok attaching VOLUME` while attaching;
- `ok none` otherwise.

An attached answer's second word always starts with `/dev/`. The parser tells the two forms
apart by that word.

**Reading the answers.** Every new or changed reader of the data plane's answers
(`flaminio-agent status`, `node.sh socket`, the CSI node plugin's `Status`, and `common.sh`'s
`control()`, which `node.sh attach`, `detach` and `delete` use) treats them as untrusted, as
#74's S6 does:
- it reads at most 4096 bytes, up to the first newline, all printable ASCII;
- it accepts only its command's forms, or an `error ...` line, with `ID` matching the volume id
  grammar `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$` and `N` `0|[1-9][0-9]{0,6}`:
  - `status`: `ok none`, `ok attaching ID` and `ok ID /dev/ublkbN`;
  - `attach`: `ok /dev/ublkbN`; `detach`: `ok`; `delete`: `ok N replicas deleted`;
- anything else is malformed: `flaminio-agent status` exits 2, `node.sh socket` prints
  `socket: error malformed`, `Status` returns an error, and `control()` returns `malformed`,
  which `node.sh` prints in its `die` instead of the answer. `node.sh attach` waits for the
  device only at a path of that grammar.

The CSI plugin's bound is #74's (`ControlSocket.command`, `io.LimitReader`): whichever of #12
and #74 lands first adds it, and the second keeps it.

**The CSI node plugin** (`dataplane.go`, `Status`) parses the new form.
- **`device`:** it returns `Unavailable` while attaching. So does the raw-block branch of
  `NodePublishVolume`.
- **An old plugin with a new data plane** reads `attaching VOLUME` as volume `attaching`, and
  answers `ResourceExhausted`. Kubelet retries, and the deploy updates both. No volume is named
  `attaching`: CSI volume ids are `pvc-<uuid>`.

**`node.sh`:**
- **Is a data plane running** (`running()`, for `detach` and `delete`; `attach` and `status`
  alike): yes where the unit is active, or where `node.sh socket` prints `socket: none` or an
  answer, so that a pod's data plane is reached through the socket.
  - **Exit 3, `not running`:** only where the unit is inactive and the socket is absent: no
    data plane at all. On a pod node, that is a data-plane container that is down.
  - **`socket: error`** with the unit inactive: exit 1, naming the reason.
- **On a pod node, `lab.sh`** then works through the pod's data plane: `detach_all` detaches
  there, `delete_volume` (`make vol-delete`) deletes the replicas there, and
  `detached_everywhere` sees `not attached`. Where the pod's data plane is down (exit 3),
  `detached_everywhere` refuses if `pgrep -x flaminio-io` finds its process starting, and
  `delete_volume`'s `cmd_node up` meets the pod-node guard, which refuses naming the pod;
  `delete_volume` then stops with its "fix it, then run again" message. None of these
  functions changes.
- **Ids:** `up` checks `ATTACH`, and `attach`, `detach` and `delete` check `VOLUME`, against
  `main`'s `volume_id` grammar before building any path or socket line. On `main`, `attach`
  does not; it now reaches a pod's data plane too.
- **Token files:** `up` with `ATTACH` writes them with `install -m 600`.
  - `detach` deletes `/run/flaminio/<VOLUME>.token` after `detached` or `not attached`.
  - `down` deletes every token file once the pod-node guard passes with the unit inactive
    (below), a crashed unit included. The glob is expanded as root, since the VM user cannot
    list the 0700 directory: `[[ ! -d /run/flaminio ]] || sudo find /run/flaminio -maxdepth 1
    -name '*.token' -type f -delete`. `/run` is tmpfs and world-readable, so after a reboot of
    a node without the CSI plugin the directory may be missing, and `down` then skips the
    deletion instead of failing under `set -e`. The unit's `up` is their only writer, since the
    CSI node plugin stores none.
- **The token** still travels on command lines, as today (decisions, accepted risks).

**`node.sh down`** (in both of #74's modes):
1. The mount check, as today.
2. **Where the unit is active:** where `node.sh socket` names a volume V attached, `detach V`
   through the socket, as today's `del` ended the attach first; while V is attaching, it
   refuses. The SIGTERM handler would otherwise hold `systemctl stop` until the stop time-out,
   and under #74's helper a root `del` of the owned device while the unit runs is unsupported.
   Then it stops the unit, also when the socket does not answer (a hung unit), as today.
3. **Then the pod-node guard,** the unit now inactive. Where it refuses, `down` deletes nothing
   and exits 1, naming the cause: on a pod node, `make node-down` leaves the pod alone.
4. **Where it passes:** it deletes a remaining `/dev/ublkc1` (#74's leftover deletion: no live
   data plane can own it now) and every token file.

**`ublk.sh`** (`serve`, id 0, which binds no socket and keeps SIGTERM's default):
- **`down`:** unmounts `$mnt` as today, then the pod-node guard; where it refuses, `down`
  deletes nothing and exits 1. Where it passes, `del` of `/dev/ublkc0` and the stop, as today.
- **`up`:** the pod-node guard first. Then #74's step: with the unit inactive, it dies if
  `$mnt` is mounted, else deletes a leftover `/dev/ublkc0` before starting.

**Why the guard comes first:** in a pod, the kernel picks the id, so a pod's device can be
`/dev/ublkc0` or `/dev/ublkc1`. `node.sh` and `ublk.sh` used to delete their id whatever owned
it, and `findmnt` misses a raw block volume bind-mounted for a pod. Once the guard passes, no
pod data plane can be on the node, so a device left at the script's id is dead. Deleting a dead
device that a raw block volume still holds waits for it, as today's `del` does.

**`node.sh socket`** prints exactly one line to stdout, and nothing else. It sends `status`, and
catches every error. One `timeout 5` wraps the whole command, sudo and Python included. Python
reads at most 4096 bytes (Reading the answers):
- **`socket: absent`:** only when the socket's path does not exist, or the connection fails with
  `ENOENT` or `ECONNREFUSED`;
- **`socket: none`:** the answer is `ok none`;
- **`socket: answer <the answer>`:** `ok attaching ID` or `ok ID /dev/ublkbN`, matched whole;
- **`socket: error <reason>`:** a time-out, a malformed or `error` answer, any other error, or a
  failure of sudo or Python. `<reason>` is one word from a fixed set: `timeout`; `malformed`;
  `refused`, for a well-formed `error ...` answer (the data plane refused `status`;
  `ECONNREFUSED` is `absent`); an errno name, for any other connection error; `failed`, for
  sudo or Python. The answer itself is never printed unless it matched.
  A hung data plane gives this, never `absent`.

It lives in `common.sh`, as `socket_state`, beside `control()`; `node.sh socket` prints it.
Under `FLAMINIO_SCRIPT_TEST=1`, `CONTROL_SOCKET` overrides its path, as for `control()`.

**The host's match:** the caller captures stdout alone, without stderr. It requires ssh to exit
0, and exactly one line, matched whole with `grep -qx`. Empty or extra output refuses.

**The pod-node guard** (`pod_node_guard`, `common.sh`) passes only when no pod data plane can
be on the node:
- `socket_state` prints `socket: absent`, or the `flaminio-node` unit is active (its socket is
  the unit's: the unit holds the disk with `O_EXCL`, so a pod's claim waits);
- where the `k3s` binary is installed, `sudo k3s crictl pods --state ready --namespace
  flaminio-system --label app=flaminio-dataplane --name '^flaminio-dataplane-' -q` exits 0 with
  empty stdout. Its stderr is ignored.
  - A tenant's pod with that name in another namespace does not count.
  - A ready sandbox stays while its container is stopped. The NotReady sandboxes of a removed
    DaemonSet do not count.
  - A `crictl` error refuses. This happens with k3s stopped, while its shims keep the
    containers running.
  - Where k3s is absent (fl-c), only the socket counts.

On a refusal it exits 1, naming the cause (the socket's line, or the data-plane pod). It runs
in `node.sh up` (first, with the `io.crt` check, before the `down` that `up` runs, which runs
it again), `node.sh down`, `ublk.sh up` and
`down`, `replica.sh up`, and the raw-disk benchmarks `fio-raw.sh` and `store-bench.sh`, which
write the data disk without `O_EXCL` or format it by default.
- **`fio-raw.sh` reading only:** it skips the guard when every selected job is a read
  (`randread` or `read`) and the disk is already preconditioned (its marker exists), so that
  it writes nothing. The guard exists for writes. `make bench-pod` runs it that way, read only,
  on a node whose store is live, a pod node included.
- **`cmd_bench_pod`'s warning** for a disk never preconditioned, today "make bench-raw first,
  before node-up", becomes: run `make bench-raw VM=X` before the first `make csi-deploy`, or
  after `make csi-undeploy`, on a disk whose store may be lost (preconditioning overwrites it),
  then format the store again (`FORMAT=1 make node-up VM=X`). On a pod node `make bench-raw`
  without the marker refuses.

**Overrides for the script tests,** honoured only under `FLAMINIO_SCRIPT_TEST=1`, as
`CONTROL_SOCKET` is: the scripts' `flaminio-io` binary, the directory of `/dev/ublkc*`,
`RUN_DIR` for `/run/flaminio` (the token files), `PKI_ROOT` for `/etc/flaminio` (`pki.sh`), and
`K3S_CONFIG_DIR` for `/etc/rancher/k3s` (`k3s.sh`). The tests stub the commands that need root
or a VM, as they stub `sudo` and `systemctl`: `modprobe`, `systemd-run` (recording the unit's
arguments), `journalctl` (the ready line), `lsblk` (or `DEVICE` set), `findmnt`, `mountpoint`,
`k3s`, `curl` and `fio`. The `sudo` stub refuses, without running it, any command in which
any argument contains `/run/flaminio`, `/etc/flaminio` or `/etc/rancher` as a substring, so
that `sudo sh -c "..."` and `sudo env ...` forms are caught too, and a script that falls back
to a real path fails in the test instead of touching the runner. No command is exempt: under
the overrides, `systemd-run`'s arguments name only temporary paths (`--pki $PKI_ROOT/io-pki`,
`--token $RUN_DIR/...`).

**Pod Security:**
- `k3s.sh server` writes `/etc/rancher/k3s/psa.yaml` (0600), an `AdmissionConfiguration`
  (`apiserver.config.k8s.io/v1`) whose one plugin, `PodSecurity`, carries a
  `PodSecurityConfiguration` (`pod-security.admission.config.k8s.io/v1`):
  `defaults.enforce: baseline` (`enforce-version: latest`), with
  `exemptions.namespaces: [kube-system, flaminio-system]`.
- It adds `kube-apiserver-arg: ["admission-control-config-file=/etc/rancher/k3s/psa.yaml"]` to
  `config.yaml`. k3s has no `pod-security-admission-config-file` key (an RKE2 option); its
  documentation configures Pod Security this way (docs.k3s.io, Security, Hardening Guide, "Pod
  Security"; the server's flags list no admission option, docs.k3s.io, CLI, `k3s server`), for
  k3s v1.25 and newer, so for the pinned `v1.36.5+k3s1` (`scripts/lab.conf`).
- **It checks the result:** once the API answers, `k3s.sh server` makes a server-side dry run
  (`kubectl run --dry-run=server`) of a privileged pod in `default`, which has no Pod Security
  label. It stops unless the API refuses it with `violates PodSecurity`. A dry run needs an
  existing namespace, so `default` stands for a fresh unlabelled one.
- **PersistentVolumes are not checked.** A hostPath PV at `/run/flaminio` mounts in a
  `baseline` namespace. These can create PVs:
  - the cluster's administrators;
  - the CSI controller's service account, `flaminio-csi-controller` (`rbac.yaml`).

  This is an accepted risk.

## Lifecycle (S4)

**SIGTERM.** A module `process` goes in `flaminio-uring`, where `unsafe` is allowed;
`flaminio-io` forbids it.
- **`install_sigterm()`:** blocks SIGTERM, and starts the `sigwait` thread that sets a static
  `AtomicBool` (below).
- **`sigterm_requested()`:** safe.
- **`euid()`:** a safe wrapper of `geteuid`, for `control::bind` (The socket).
- **Docs:** `flaminio-uring/src/lib.rs`'s list of the modules that hold `unsafe` (lines 10–11)
  and ARCHITECTURE §15.5 ("`unsafe` is confined to the io_uring, ublk and crypto FFI modules")
  name the new module.

**The handler.** `install_sigterm()` runs first in `node::main`, before that subcommand starts
any thread.
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
  true, it removes the socket and exits 0. The main thread drives the engine itself, so there
  is nothing to stop first.
- **While attached,** the main thread is inside `attach_and_serve` and checks nothing. So a
  SIGTERM while attached makes the process exit after the next detach. `begin_attach` refuses
  every attach after the SIGTERM, so nothing new is attached in between.
- **Nothing is logged** for an ignored SIGTERM.

**The preStop hook and `status --none`:** `flaminio-agent status --none` exits 0 only on
`ok none`, on a missing socket path, or on a connection that fails with `ENOENT` or
`ECONNREFUSED`, so a pod deleted before its data plane is up does not wait 3600 s. It exits
non-zero on anything else:
- no answer within its own bound, 5 s;
- an `error` answer, or a malformed one (Reading the answers);
- an attach, attached or attaching;
- any other connection error.

A hung data plane then holds the preStop hook, up to the grace period, as an attached one does.
Step 4 and step 5 of the upgrade use the same check.

**The preStop hook signals first** (`kill -TERM 1`, The DaemonSet). Kubelet sends its own
SIGTERM only after the hook returns, and the hook returns on `ok none`. Without the signal, an
attach arriving between the two (a NodeStage retry on a node that is not cordoned, after
`csi-undeploy` or a removed label) would be taken and defer the exit, and the new volume would
be killed when the grace ran out. With it, `begin_attach` refuses from the start of the hook:
with nothing attached the process exits at once, and attached it exits after the detach.

**The pod nodes asked from the host** (`pod_nodes_idle`, `lab.sh`), for `make k8s-down`,
`make k8s-uninstall` and `make csi-undeploy`. It asks over ssh, not through Kubernetes, which
may be the thing that is down:
- **Which VMs:** each of `vms_of all` (fl-a, fl-b: the VMs whose k3s agent runs data-plane
  pods), except a VM whose `flaminio-node` unit is active (`systemctl is-active`, same ssh): its
  socket is the unit's, which stopping k3s does not touch. fl-c runs no k3s.
- **The question:** `run_on_vm NODE node.sh socket`, matched as above.
- **It proceeds** only on `socket: absent` or `socket: none`.
- **Anything else refuses, naming the VM:** an attach, `socket: error`, an ssh failure (an
  unreachable VM, a time-out), or output that is empty or extra.
- **`FORCE=1`** skips the check.

**`make csi-undeploy`** (`cmd_csi undeploy`), in this order:
1. it deletes the namespace `flaminio-test`, as today;
2. it waits, up to 120 s, until no VolumeAttachment of `csi.flaminio.io` names a PV whose claim
   was in `flaminio-test` (ControllerUnpublish runs after the namespace is gone);
3. `pod_nodes_idle`: it refuses while any pod node's socket names an attach, whatever made it
   (a PVC elsewhere, `make vol-attach`), or cannot be asked;
4. it deletes the DaemonSet `flaminio-dataplane` and waits for its pods to go;
5. it deletes the driver, as today (`render_csi_placeholder`);
6. it removes the pod-node labels.

**Leaked ublk ids:** the kernel picks a free id. A dead device stays until the node reboots,
or, at id 0 or 1, until a unit script deletes it once the pod-node guard passes (The socket).

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
  2. finds the served ones, from the VolumeAttachments and from each data node's socket:
     `node.sh socket` on every VM of `data_vms` (fl-a, fl-b, and fl-c once it exists), matched
     as above. `socket: answer` names a served volume; `absent` and `none` name none. Any other
     line, or a VM that cannot be asked, refuses, naming the VM: a volume served from it would
     otherwise count as unserved;
  3. checks every chunk's ISR against its replicas.
- **`minIsr`:** from `metactl get`, for the served volumes only, the set `served_isr_full`
  uses: for each chunk with a replica on the node, `(isr | length) - 1 >= minIsr`. An unserved
  RF 1 volume, such as `wbench`, does not block the upgrade.
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
`ps` for a moment, to any local user (`/proc` has no `hidepid`). sudo no longer logs it.

**The server token's old copies:** earlier installs logged it on fl-a and fl-b (sudo's
`K3S_TOKEN`), in the journal and `/var/log/auth.log`. They stay, valid until the token is
rotated (out of scope); this change writes no new copy.

**Modes:** `config.yaml` and `agent-token` are 0600 before any secret is written. Today's
`sudo tee` creates them 0644 (`k3s.sh`).
- **An absent file** is created with `sudo install -m 0600 /dev/null FILE`.
- **An existing one** is first read, for the restart decision's comparison, then set with
  `chmod 600`, then written.

**When the configuration changes:** `install_k3s` returns early while the right version runs.
So `server` and `agent` compare the configuration they write with the old one, and restart k3s
when it differs. A restart of the service leaves the pods running.

## The lab (S7, S8)

**`cmd_csi deploy`,** when not `demo_active` and with `DATAPLANE=pod` (the default):
1. **Claim:** `cmd_agent claim` on fl-a and fl-b, which formats only a blank disk.
2. **The unit:** where the unit runs, it refuses if `node.sh socket` names an attach, and
   otherwise runs `node.sh down`.
3. **The image:** build it, import it on fl-a and fl-b, and check the tag is listed (The image).
4. **Labels:** `kubectl label node fl-a fl-b node-restriction.kubernetes.io/flaminio-dataplane=pod`.
5. **The DaemonSet:** check the values' grammar, render and apply it. `kubectl rollout status`
   fails for `OnDelete`, so it polls until `.status.observedGeneration` equals
   `.metadata.generation` and `numberReady` equals `desiredNumberScheduled` (300 s).
6. **The driver:** as now.

**`DATAPLANE=unit`** (S7): only step 6, over the units that `make node-up` started, as
`make csi-deploy` does today. It refuses where a node carries the pod-node label
(`make csi-undeploy` first). `DATAPLANE` is read by `csi-deploy` alone; #74's evidence runs
`make node-up` under the helper, then `DATAPLANE=unit make csi-deploy` and `make csi-test`.
The demo always deploys this way.

**The other commands:**
- **`csi_failover`:** it reads the mode from the cluster, not from `DATAPLANE`: where the
  DaemonSet `flaminio-dataplane` exists, it waits for the data-plane pod on the lost VM to be
  Ready, instead of `cmd_node up`; otherwise it runs `cmd_node up` as today. A later command
  does not need the variable `csi-deploy` was given.
- **`cmd_csi undeploy`:** the steps of Lifecycle ("`make csi-undeploy`").
- **`make node-upgrade`:** a Makefile target with its `##` help line, and an entry in `lab.sh`'s
  `usage()`.
- **Alignment** (AGENTS.md):
  - the Makefile's help: `csi-deploy` without "(needs node-up)", with `DATAPLANE=unit`;
    `csi-undeploy` naming its refusal; `node-upgrade`;
  - `docs/lab.md`: the CSI section without `make node-up`, the two kinds of data node,
    `DATAPLANE=unit` (and #74's evidence run with it, once both have landed),
    `make node-upgrade`, the PKI directory and its issue on an existing lab, and
    `make bench-pod`'s read-only raw comparison on a pod node;
  - `docs/followups.md` §9: what this change delivers, and what stays: adopting #74's helper in
    the pod (intent, Out of scope), ublk user recovery, separating the metadata members.

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
    - exits 2 on `error x`, and on each malformed answer: 4097 bytes with no newline, a
      non-printable byte, `ok attaching v;x`, `ok v /dev/sda`, `ok v /dev/ublkb01`;
  - **`status --none`:** exits 0 on `ok none`, with no socket path, and on `ECONNREFUSED` (a
    bound socket nobody listens on). Non-zero on either attach form, on `error x`, on a
    malformed answer, and on a socket that accepts and never answers (within its 5 s bound).
- **`control/csi`:**
  - `Status` on `ok attaching v` gives an attach of `v` in progress;
  - `Status` on `ok attaching /dev/ublkb3` gives volume `attaching`, attached at
    `/dev/ublkb3`;
  - `Status` on `ok attaching v;x` and `ok attaching ../x` gives an error (the id grammar);
  - `device`, and the raw-block `NodePublishVolume`, return `Unavailable` while attaching;
  - **`Detach`** (`dataplane.go`) on `error: v is attaching` gives an error that
    `NodeUnstageVolume` returns as `Unavailable`. On "is not attached here", it still
    succeeds.
- **`deploy/` (a new `deploy/manifests_test.go`):** renders `dataplane.yaml` and `driver.yaml`
  with placeholder values, and checks every field listed under "The DaemonSet" above:
  - `/run/flaminio` mounted only by `flaminio-dataplane` and `flaminio-csi-node`;
  - the data plane's `--control` equal to the node plugin's `--control-socket`;
  - `out-of-service` tolerated by the data plane, and not by the node plugin;
  - **quoting:** rendered with values holding a quote, `$(x)`, `a|b` and a newline, no
    container's command or script contains a value: each script is the constant text above,
    run with `sh -ec`, and the values appear only in `env`;
  - **the PKI:** one `Directory` hostPath, `/etc/flaminio/io-pki`, read-only, and no `File`
    hostPath under `/etc/flaminio`;
  - **the ServiceAccount** `flaminio-dataplane` is in `rbac.yaml`, not in `dataplane.yaml`;
  - **preStop:** its command runs `kill -TERM 1` before the `status --none` loop; the
    container's script `exec`s `flaminio-io`, `hostPID` is false and `shareProcessNamespace`
    is unset, so the hook's PID 1 is `flaminio-io`.

**Rust:**
- **`flaminio-uring`, `tests/sigterm.rs`:** declared as `[[test]]` with `harness = false`.
  - Its own `main` calls `install_sigterm()` before anything else, so libtest's threads, which
    would take the signal, are never started.
  - It then sends `kill(getpid(), SIGTERM)`: `sigterm_requested()` becomes true, and the process
    lives on. `euid()` equals the owner of a file the test creates.
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
  - a second attach while `Attaching(v)` is refused with `already attaching v`, and one while
    `Attached(v, ...)` with `already attached to v`;
  - **state before token:** attached to `v`, no SIGTERM, `attach w` with a token that is not
    base64 answers `already attached to v`, not `the token is not base64`;
  - two concurrent attaches: exactly one is sent to the engine;
  - `detach v` while the slow fake engine holds `Attaching(v)` answers `error: v is
    attaching`, and the attach then completes;
  - **after SIGTERM** (the test's flag): attached to `v`, the flag set, `detach v`, then
    `attach w`: refused with `the node is shutting down`, and nothing is sent to the engine;
    `should_exit` is then true;
  - **the order in `serve`:** attached to `v`, the flag set, `attach w` with a well-formed token
    answers `the node is shutting down`, not `already attached to v` (the shutdown check comes
    first, and `serve` has no attached-state check of its own);
  - **`detach v`'s wait:** the fake engine starts an attach of `w` as soon as `v`'s device
    stops; `detach v` answers `ok` without waiting for `w`'s attach;
  - **`startup_next`:** `Attaching` and a failed send retry; `Attached` and shutting down stop;
  - **`control::bind`:** the directory is 0700 and the socket 0600; a symlinked directory, and
    an `owner` other than the directory's, are refused.

**Scripts** (`make lint`; bash 4.4 guard, as the existing ones):
- **`scripts/tools/node-scripts-test.sh`** (extended; with the overrides and stubs of "Overrides
  for the script tests" (The socket): `RUN_DIR`, `PKI_ROOT` and `K3S_CONFIG_DIR` in the case's
  temporary directory, so nothing writes `/run/flaminio`, `/etc/flaminio` or `/etc/rancher`):
  - **`node.sh socket`:** `absent` with no socket path; `none`; `answer ok attaching v` and
    `answer ok v /dev/ublkb3`; `error malformed` on `ok v /dev/sda`, on `ok attaching v;x` and
    on a 5000-byte line; `error refused` on an `error x` answer; `error timeout` on a socket
    that never answers; `absent` on a socket file with no listener (`ECONNREFUSED`, a crashed
    data plane's stale socket); exactly one line each time;
  - **the guard:** refuses on a socket that answers with the unit inactive, on a ready sandbox,
    and on a failing `crictl`; passes with no `k3s` and the socket absent, and with the unit
    active whatever the socket says; `crictl` is given `--namespace flaminio-system` and
    `--label app=flaminio-dataplane`, so a decoy listed only without them does not refuse;
  - **`detach` and `delete`, unit inactive:** with the socket answering they reach it (a pod
    node); with it absent, `not running` and exit 3, as on `main`; with `socket: error`,
    exit 1 and nothing sent;
  - **`control()`'s answers:** a 5000-byte answer and one with a non-printable byte make
    `detach`, `delete` and `attach` exit 1 printing `malformed`, never the bytes; `attach` on
    `ok /dev/sda` exits 1 without waiting for a device;
  - **`attach`'s id:** `VOLUME` of `v x`, `v;id` or a newline: exit 1, nothing sent;
  - **`node.sh down`:** unit active and `v` attached: `detach v` is sent before the stop, and
    `/dev/ublkc1` is deleted after it; unit active and `ok attaching v`: exit 1, no stop and no
    `del`; unit inactive with a socket answering, or a ready sandbox: exit 1, no `del`, token
    files kept; unit inactive (a crash), socket absent: the leftover `/dev/ublkc1` and a
    planted `.token` in `RUN_DIR` are deleted, and the `sudo` stub, which records its
    arguments, shows the deletion ran as `sudo find ... -delete`, not as a glob of the
    calling shell; the same with `RUN_DIR` missing: exit 0, the leftover `del` still runs, and
    no `find` is run;
  - **`node.sh up`:** refuses on a pod node, and starts nothing; `ATTACH` of `../x` or `v x` is
    refused before any path is built; the token file is written 0600; the unit's arguments
    that `systemd-run` records carry `--pki` of the `io-pki` directory (`$PKI_ROOT/io-pki`;
    `/etc/flaminio/io-pki` outside the tests); with `io-pki/io.crt` missing it dies naming
    `make pki-issue`, `systemd-run` is not called, and a running stubbed unit stays active (no
    `systemctl stop` recorded, no `del`). `io.key`'s 0640 and the group
    `flaminio-io` are set by #74's `node.sh up`: once #74 has landed, its own case checks them
    on `io-pki`, from the commands the `sudo` stub records;
  - **`pki.sh`** (in `PKI_ROOT`): `csr io` and `install io` write into `io-pki`, and
    `install io` removes `pki/io.crt` and `pki/io.key` by name, under `sudo`; `csr meta` stays
    in `pki`; `file ca.crt` and `file token.pub` write both directories, `file token.key` and
    `file etcd-ca.crt` only `pki`; after `cmd_pki issue`'s sequence of `pki.sh` calls,
    `io-pki` holds exactly `io.crt`, `io.key`, `ca.crt` and `token.pub`. Modes only, as
    `pki.sh` sets them: `io.key` 0600 (`csr`'s `umask 077`), and `io-pki` 0700, or 0750 once
    #74 has landed (`pki.sh:10`);
  - **the overrides without the switch:** for each of `RUN_DIR`, `PKI_ROOT` and
    `K3S_CONFIG_DIR`, set with `FLAMINIO_SCRIPT_TEST` unset, as `main` checks
    `CONTROL_SOCKET`:
    - `pki.sh csr io` and `k3s.sh server`: nothing is written in the temporary directory, and
      the `sudo` stub's refusal of the real path fails the script;
    - `node.sh down` (unit inactive, socket absent, a `.token` planted in the temporary
      `RUN_DIR`): the planted token survives, and the `sudo` stub records no command naming
      the temporary directory. Honouring `RUN_DIR` would run `sudo find <tmp> ... -delete`
      and delete it, so the case can fail; the real `/run/flaminio` may be absent on the
      runner, where `down` reaches no `sudo` for it;
  - **`k3s.sh server`** (in `K3S_CONFIG_DIR`, `curl` and `k3s` stubbed): with the stub's dry run
    answering `violates PodSecurity`, it exits 0 and its `config.yaml` (0600) carries the
    `kube-apiserver-arg` line and `psa.yaml` the `AdmissionConfiguration`; with the dry run
    admitted, it exits non-zero;
  - **`node.sh detach`** deletes `<VOLUME>.token` after `detached` and after `not attached`;
  - **`ublk.sh`:** `up` with the unit inactive, the guard passing and `/dev/ublkc0` left:
    deletes it, then starts; with the guard refusing: exit 1, no `del`, no start; `down` with
    the guard refusing: no `del`;
  - **`fio-raw.sh`, `store-bench.sh`, `replica.sh up`:** refuse on a pod node before touching
    the disk (`fio` never runs); `fio-raw.sh` with `bench-pod`'s read-only job filter and the
    marker present skips the guard and runs only read jobs; with a write job selected, or the
    marker missing, it refuses.
- **`scripts/tools/lab-k8s-test.sh`** (new, added to `make lint` beside the others), lab.sh's
  functions extracted and run against fake VMs, as `lab-deletion-test.sh` does:
  - **the values:** `SERIAL` of `a|b`, `x"y` and `a b`, `META_ENDPOINTS` of `$(id)`, a
    `TRUST_DOMAIN` with a newline: refused, and nothing rendered or applied;
  - **`pod_nodes_idle`:** proceeds on `socket: absent` and `socket: none`; refuses on an
    attach, on `socket: error`, on empty or two-line output, and on ssh's 255, naming the VM;
    skips a VM whose unit is active;
  - **`served_isr_full`:** asks fl-c once it exists; a volume served only from fl-c counts;
    refuses when a VM cannot be asked;
  - **`cmd_csi undeploy`:** the namespace, the bounded wait, the check, then the DaemonSet, the
    driver and the labels, in that order; nothing is deleted after a refusal;
  - **`DATAPLANE=unit`:** with a node carrying the pod-node label, `cmd_csi deploy` refuses and
    applies nothing;
  - **`csi_failover`'s mode:** with the fake cluster listing the DaemonSet, it waits for the pod
    and runs no `cmd_node up`; without it, it runs `cmd_node up`;
  - **the io identity on an existing lab:** with `io-pki/io.crt` missing on a VM,
    `cmd_csi deploy`, `cmd_node up`, `cmd_data_node_up` and `ensure_node_c` each copy `ca.crt`
    and `token.pub` and issue `io` before `node.sh up` or the DaemonSet; with it present, they
    issue nothing.
- **`lab-deletion-test.sh`:** its `start-fails` case stands for a pod node whose guard refuses
  `cmd_node up`: `delete_volume` stops before deleting any replica.

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

     The whole journal and `/var/log` are searched too: their hits for the server token are
     listed, expected from earlier installs (S7), and none is dated after the restart.
   - **Kubernetes refuses:**
     - a kubelet's own label (`kubectl --kubeconfig` with the node's credentials);
     - a privileged pod, and a hostPath pod, in a new namespace; `k3s.sh server`'s own dry run
       passed;
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
     continuously. Its pod has `preStop: sleep 30`, so fio writes on for 30 s once the pod is
     Terminating.
   - **Setup:** its pod on fl-a, then a rebuilt `flaminio-io`, `make csi-deploy`, and
     `make node-upgrade`.
   - **fl-b:** the load pod's UID and restart count are unchanged, and fio logs no error.
   - **fl-a:** the pod is Ready on fl-b within 300 s, and fio verifies the saved state.
   - **The data-plane pods:** both at the newest revision, with restart count 0.
   - **A second `make csi-deploy`** with the same build changes no revision.
   - **A second `make node-upgrade`** skips both nodes: neither is cordoned, and no pod is
     deleted.
5. **The refusals,** each from a clean lab, with its own setup:
   - **#15:** a PVC pod on each node. It refuses at fl-a's step 2.
   - **Outside Kubernetes:** vol1 attached by `make vol-attach VM=a`, with no load pod. It
     refuses at fl-a's step 4, and fl-a stays cordoned.
   - **PodDisruptionBudget:** `minAvailable: 1` on the load StatefulSet. It refuses at fl-a's
     step 3.
   - **No controller:** a bare pod with a PVC on fl-a. It refuses at step 2.
   - **RF 1:** the load StatefulSet on fl-a, its PVC from a StorageClass with `replicas: "1"`.
     fl-b, with no VolumeAttachment, goes first. It refuses at step 2 of the first node, in
     that order, that holds one of the volume's chunks (`metactl get`), and no data-plane pod
     on a node holding them is deleted.
   - **`minIsr`:** the load StatefulSet on fl-a, its PVC from a StorageClass with
     `replicas: "2"` and `minIsr: "2"`. fl-b goes first, and it refuses at fl-b's step 2.
   - **ISR not full:** the load on fl-a. fl-b goes first and is upgraded. During fl-a's step 4,
     while the evicted load pod is still Terminating on fl-a and writing (its `preStop`),
     fl-b's data-plane container is killed (`crictl stop -t 0`): the target on fl-a drops
     fl-b's replicas from the ISR. The load pod then starts on fl-b, and step 4 ends once its
     VolumeAttachment names fl-b. The step-5 check finds the load volume's ISR not full, and
     refuses at once; fl-a's data-plane pod is not deleted.
   - **An administrator's cordon:** fl-a cordoned before the run. It stays cordoned after.
   - **Interrupted:** the run killed (SIGINT) during fl-b's step 2. The `EXIT` trap uncordons
     fl-b.
   - **`make csi-deploy` over a unit:** with the unit serving vol1 on fl-a, it refuses there,
     and fl-a gets no data-plane pod.
6. **Deletions while attached,** `make csi-test` with `KEEP=1` on fl-a:
   - **The label removed:** the pod is `Terminating` and keeps serving. It is gone within 10 s
     of the test pod's move (cordon and delete).
   - **The DaemonSet deleted:** the same.
   - **SIGTERM by hand** to `flaminio-io`:
     - with nothing attached, it exits within seconds;
     - attached, it keeps serving, and exits after the volume's detach.
   - **The preStop hook's own SIGTERM:** with vol1 attached on fl-a by `make vol-attach VM=a`
     and no PVC pod, fl-a's data-plane pod is deleted. While it is Terminating, `node.sh attach`
     on fl-a with `VOLUME=x` and a well-formed token answers `the node is shutting down`, which
     only the hook's signal can cause before kubelet's SIGTERM. The pod keeps serving vol1, and
     goes within seconds of `scripts/lab.sh vol detach a`. Its `FailedPreStopHook` warning
     event, from the hook's `sh` ending with the container, is expected.
   - **Refusals:** `make k8s-down` and `make k8s-uninstall`, with the test pod attached.
   - **`make csi-undeploy`** deletes `flaminio-test` first, so its refusal is shown with an
     attach outside it: the upgrade load on fl-a (`flaminio-upgrade`), and, in a second run
     with no PVC pod, vol1 attached by `make vol-attach VM=a` (no VolumeAttachment). Each time
     it refuses naming fl-a, and the DaemonSet, the driver and the labels stay.
   - **With Kubernetes down:** k3s stopped on fl-w, `make k8s-down` still refuses.
   - **An unreachable node:** with fl-b's ssh blocked, `make k8s-down` and
     `make k8s-uninstall` refuse, naming fl-b.
   - **A hung data plane:** fl-a's `flaminio-io` SIGSTOPped. `node.sh socket` prints
     `socket: error`, so `make k8s-down` and `node.sh up` refuse.
   - **The `crictl` error alone:**
     1. `crictl stop` on fl-a's data-plane container;
     2. k3s stopped on fl-a right after, before kubelet restarts the container;
     3. `node.sh socket` is asserted to print `socket: absent`;
     4. `crictl` fails, so `node.sh up` refuses.

     Then k3s is started again.
7. **A crash.**
   - **Setup:** `crictl stop -t 0` on the data-plane container, with the test pod on fl-a.
   - **The data plane:** fl-a is alive again. A few container restarts are allowed, as the
     claim does not rerun.
   - **The test pod:** deleted, with fl-b cordoned, it is Ready on fl-a with "verified on
     fl-a", at a higher epoch.
8. **One data plane per node,** with fl-a's data-plane container stopped (`crictl stop`):
   `node.sh up`, `ublk.sh up`, `replica.sh up`, `fio-raw.sh` and `store-bench.sh` on fl-a
   refuse, and no unit starts and nothing touches the disk. **A decoy:** after
   `make csi-undeploy`, an ordinary pod named `flaminio-dataplane-x` in `default`, on fl-a,
   does not make the guard refuse: `make node-up` starts fl-a's unit.
9. **The units' `down`s,** with `make csi-test` `KEEP=1` on fl-a: `make node-down` and
   `make ublk-down` on fl-a refuse, and the test pod's volume stays attached.
10. **Token files** (fl-c, the unit, `ATTACH` given to `node.sh up` by hand, with `PRIVSEP=0`
    once #74 has landed): the token file is 0600, and gone after `node.sh detach`. After a new
    attach and a stray `.token` file planted by hand, the unit is killed (`systemctl kill`):
    `node.sh down` still leaves none.
11. **`DATAPLANE=unit`:** after `make csi-undeploy` and `make node-up`, `DATAPLANE=unit
    make csi-deploy` deploys the driver with no data-plane pod and no pod-node label, and
    `make csi-test` passes over the units. `make csi-failover`, run without `DATAPLANE`, then
    restarts the lost VM's unit (`cmd_node up`). With the label put back on fl-a by hand,
    `DATAPLANE=unit make csi-deploy` refuses.
12. **`make bench-pod` on pods** (the default deployment). **Setup,** after `make csi-undeploy`
    (lab 11 leaves the driver over units): `make node-down`, which stops both units (the guard
    passes with no data-plane pod), so no store is live beneath the next write; `make
    bench-raw VM=b` preconditions fl-b's disk (its store is lost); `FORMAT=1 make node-up
    VM=b` formats the store again; `make node-down VM=b`; then `make csi-deploy`, which finds
    no unit active. The fio pod's run and the raw read-only comparison both complete,
    `raw.tsv` has the three read jobs, and fl-b's data-plane pod is not restarted. Volumes
    made before it lose their fl-b replica, so it runs after the labs that use them, or on a
    fresh lab; lab 13 makes its own state.
13. **An existing lab's identities:** after `make csi-undeploy`, `/etc/flaminio/io-pki` removed
    on fl-a by hand, as on a lab issued before this change. `make csi-deploy` issues `io` there,
    and fl-a's pod is Ready within the deploy's 300 s. The same with `make node-up` instead:
    the unit starts.
