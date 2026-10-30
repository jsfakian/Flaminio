# Issue #14: Tombstones on nodes outside Kubernetes — design

How the [spec](spec.md)'s rules are built. Settled choices: [decisions](decisions.md).

The facts it starts from:
- `DeleteVolume` writes the tombstone with the nodes `replicaNodes` finds (replicas, and a
  moving chunk's source and destination); `CreateVolume` refuses an id with one;
  `ConfirmDeletion` succeeds for a missing tombstone and answers `InvalidArgument` for a node not
  on an existing one; `ListVolumes` skips headers being created or deleted
  (`meta/service/service.go`).
- `Authorize` lets an administrator list and confirm for any node, a node's agent for itself;
  `pki.PeerIdentity` takes the leaf's single URI SAN in the trust domain (`meta/service/auth.go`,
  `pkg/pki/pki.go`).
- The CSI sweep lists, deletes through the control socket, then confirms, every 5 s
  (`control/csi/tombstones.go`). The control socket's delete refuses a volume attached on that
  node and deletes under epoch `u64::MAX` (`dataplane/crates/flaminio-node/src/engine.rs`,
  `dataplane/crates/flaminio-node/src/replica/mod.rs`).
- `node.sh delete` today succeeds silently when the node is not running.

**Every data node,** below: `vms_of all` (fl-a, fl-b), and `$VM_C` once it is created.

## The commands (S1, S2)

`meta/cmd/flaminio-meta/main.go`.
- **The client** becomes `client(args []string, stdout, stderr io.Writer) error`; `main` exits.
  Its flag set uses `ContinueOnError` and writes to `stderr`, so `-h` and a bad flag exit 1. It
  gains `--trust-domain`, which `metactl` passes.
- **`tombstones [node]`:** with a node, `ListTombstones`; without, `ListNodes` then
  `ListTombstones` for each, merged by volume. The JSON is built whole and printed only on
  success: `{"tombstones": [{"volume", "nodes", "pending"}]}`, `{}` for none. A tombstone
  pending only for an unregistered node is not listed, so reads as gone: `CreateVolume` still
  refuses its id, which fails safe.
- **`confirm <volume> <node> --replicas-deleted`:** its own argument scan, not `flag`: exactly
  two positionals and the exact token `--replicas-deleted`, before or after them. Anything else
  (`--replicas-deleted=false`, `-replicas-deleted`, a third argument, another flag): exit 1, the
  refusal's text says what the token asserts, no call.
  1. **Identity, before any call:** the leaf that `tls.LoadX509KeyPair` loads from `--cert`
     (its first certificate) must have exactly one URI SAN, in `--trust-domain`, which
     `pki.ParseID` reads as component `agent`, node equal to the argument: as
     `pki.PeerIdentity` checks a peer. No certificate, an administrator, another node, `io` or
     `meta`, two URI SANs, another trust domain, a file whose leaf is not first: exit 1,
     pointing to `lab.sh vol-confirm`.
  2. **Read:** `ListTombstones(node)`. Stderr: the identity, the volume, the node, and the
     tombstone's `nodes` and `pending`, or "nothing pending".
  3. **Not listed:** stdout `{}`, stderr "nothing to confirm", exit 0, no further call.
  4. **Listed:** `ConfirmDeletion`. Stdout: the tombstone as read, `pending` less the node.
     Errors: the service's (`rpc error: code = ...`), exit 1.
- **Usage text:** the doc comment and usage line say `lab.sh vol-confirm` is the only supported
  entry point, never shell history, and what a wrong confirmation does.

## The service (S4)

`ConfirmDeletion` writes through `kv.TxnRev`, not `Txn`. After the write succeeds, and only if
the node was not confirmed before, one `slog.Info`: the caller (`callerOf`; "unauthenticated"
without), the peer address (`peer.FromContext`), the certificate serial (from the peer's TLS
state), the volume, the node, the nodes still pending, the revision. No line on a retry's
failed attempt, a missing tombstone, or a repeat. A write committed by etcd whose answer was lost
and that the client retries (`meta/kv/etcd.go`) finds the node confirmed: no line.

## node.sh (S5, S7)

`scripts/guest/node.sh`. One line on stdout, the log stays on stderr:
- **`delete`:** `deleted <n>` (exit 0) for `ok <n> replicas deleted`; `not running` (exit 3);
  anything else exit 1.
- **`detach`:** `detached` (0), `not attached` (0), `not running` (3), else 1.
- **`control()`** moves to `scripts/guest/common.sh`, which `run_on_vm` prepends to every guest
  script, so `confirm.sh` uses the same one. The socket path is a variable passed to `python3` as
  an argument (`sudo` resets the environment).
- **Callers that do not redirect stdout** get `>/dev/null`: `cmd_vol` (attach, detach),
  `node_fault`'s detach, the CSI cleanup's, the drain test's, and `scripts/checks/reconcile.sh`.

## delete_volume (S5)

`scripts/lab.sh`, `delete_volume <volume> [format-node]`. `delete_wbench` and the `$VOL`
removal pass the campaign's node; `vol delete` passes none. In order:
1. **Detach** on every data node: each must answer `detached`, `not attached` or `not running`.
   Exit 1 otherwise, before the metadata delete; ssh's 255: 255.
2. **Delete** from the metadata service. `NotFound` (`code = NotFound` on stderr) goes on; any
   other error: exit 1.
3. **Start** every data node not running. The format node as `write_volume` starts it
   (`FORMAT=1 cmd_node up`, the disk by serial, `data_disk`), even when it is fl-c: that rule
   wins over `ensure_node_c`'s claimed device. In the lab both are the same disk; were they not,
   the serial disk is formatted, as `write_volume` does today. Every other node starts with
   `FORMAT=0` passed explicitly: fl-c as `ensure_node_c` does, the others with `cmd_node up`.
   A failed start: exit 1.
4. **Re-check:** `get <volume>` answers `NotFound`; anything else (a creation meanwhile): exit
   1, before any delete.
5. **Clean** every data node: `node.sh delete` must print `deleted <n>`; anything else, `not
   running` included: exit 1, naming the node.
6. **Pending without a sweep:** `tombstones` (administrator) shows the volume pending for a node
   outside `vms_of all`: exit 1 at once, naming that node, and printing the procedure:
   `lab.sh vol-confirm <volume> <node>`, then the interrupted command again (`RESUME=<dir> make
   bench-writes`, or `make vol-delete`).
7. **Wait:** `tombstones` every 5 s, for `TOMBSTONE_WAIT` s (120). A failed poll is retried. On
   time-out: exit 255 if the last poll failed with 255, else 1, naming the pending nodes and
   `make csi-deploy`.

ssh's 255 from any step exits 255. `unknown command` from `metactl`: exit 1 with `make meta-up`.
Callers run it before stopping or formatting a node (`write_reset`, `write_whole`, the restore).

## The campaign (S6)

`cmd_bench_writes`, `write_volume`.
- **Before the first reset, decided over every volume before any removal:** `metactl volumes`
  (ids), then `get` of each; a volume's nodes are its chunks' replicas, and a moving chunk's
  source and destination, as `replicaNodes`. Any volume other than wbench and `$VOL` on `NODE`:
  exit 1, naming them, nothing removed. Only then, if `$VOL` is on `NODE`, `delete_volume "$VOL"
  "$node"`. A volume being created or deleted is not listed (`ListVolumes`); the check holds at
  the campaign's start only (decisions, accepted risks).
- **`write_volume`:** after `node up`, it polls `ListNodes` until `NODE` is alive (bounded),
  creates wbench, and checks every chunk has a replica on `NODE`; else exit 1.
- **The restore's drain loop** (`$VOL`'s replicas moved back) goes, with its comment, and the
  comment above `cmd_bench_writes` ("NODE's replicas of other volumes move away first and come
  back after") is updated.

## vol-confirm (S7)

`lab.sh vol-confirm <volume> <node>`. The workstation runs the steps; three node-side parts run
on the node, as subcommands of one guest script, `scripts/guest/confirm.sh`, which takes the
paths of `flaminio-meta`, `flaminio-agent` and the disk tool (`agent.sh`'s,
`~/flaminio/dataplane/target/release/flaminio-disk`) as variables, and confirms as the node's
agent (`/etc/flaminio/pki/agent.crt`).
1. **Workstation:** `vms_of <node>` must give exactly one VM, not in `vms_of cluster`; it is
   used for every later step. Else exit 1, before anything is installed.
2. **Workstation:** builds `flaminio-meta` as `meta up` does; installs it on the node when its
   hash differs.
3. **`confirm.sh check`** (node): `tombstones <node>` lists the volume, else exit 0 "not listed:
   nothing done". No process `flaminio-csi` or `flaminio-agent` (`pgrep -x`); `k3s` and
   `k3s-agent` each `inactive`, `disabled` or `not-found` (`systemctl is-active`, `is-enabled`).
   Issue #13's sweep unit, when it comes, joins this list.
4. **Workstation:** `node.sh detach` on every data node, then again: the second round must
   answer only `not attached` or `not running`, and where `not running`, no `flaminio-io`
   process runs.
5. **`confirm.sh delete`** (node): `flaminio-node` is active, and exactly one `flaminio-io`
   process runs, whose PID is the unit's `MainPID`: no stray process holds the control socket.
   `flaminio-agent disks`, as root, without `--serial`, with the disk tool: exactly one `ours`
   disk, whose `readlink -f` is the running unit's `--device` (`systemctl show flaminio-node`).
   Then the control socket's delete: `ok <n> replicas deleted`.
6. **Workstation:** step 4's check again.
7. **`confirm.sh confirm`** (node): step 5's unit and process check again; the delete again,
   `ok 0 replicas deleted`; then `confirm <volume> <node> --replicas-deleted`. `{}`: exit 1,
   "confirmed by another caller".

Any other stop: exit 1; ssh's 255: 255. Nothing is confirmed before step 7.

## The one-time cleanup (S8)

`make meta-up`; then `vol-confirm <volume> fl-c` for each volume `tombstones fl-c` lists; then
`metactl tombstones` lists none.

## Tests

- **S1, S2** (`meta/cmd/flaminio-meta/main_test.go`, new; in-memory service on a localhost gRPC
  server; certificates as `auth_test.go`'s `writeIdentity` makes them, copied; nodes heartbeat
  before any create):
  - `tombstones`: two tombstones over three nodes, each once, by volume; a node with none: `{}`;
    a tombstone written for an unregistered node: not listed, `create` still refused; as an
    agent without a node: `PermissionDenied`, nothing on stdout;
  - `confirm` refusals, each with no call (a counting server): no token, `=false`, single dash,
    misspelling, a third argument, `-h`; no certificate; an administrator for a registered,
    stopped node; an `io` and a `meta` certificate; an agent for another node; two URI SANs;
    another trust domain; a certificate file with the leaf second;
  - as the node's agent: the token before and after the positionals; stdout and stderr as
    above; the last confirmation frees the id; again, an unknown volume, a node not on the
    tombstone: `{}`, exit 0, no `ConfirmDeletion`.
- **S3** (`auth_test.go`): an administrator lists and confirms for any node; `io` may not
  confirm.
- **S4** (`service_test.go`): the log line's fields, with the identity and the peer set in the
  context (`peer.NewContext`, as `Authorize` sees them); "unauthenticated" without; none for a
  missing tombstone or a repeat; two confirmations interleaved after the first's read
  (`interleaveStores`): both count, each logs once. `slog`'s default logger is captured: the
  package's tests do not run in parallel.
- **S5–S7, scripts** (`scripts/tools/`, named in `make lint`; bash 4.4 guard as
  `bench-reference-test.sh`; `LOCAL_STATE_DIR` and fl-c's `created` flag in a temporary
  directory; the fake socket in a short temporary path, under macOS's 104 bytes):
  - `node.sh`: `common.sh` prepended, `sudo` and `systemctl` stubbed, a real `python3` against a
    fake socket server: each status line and exit, for its answer, an inactive unit, an error;
  - `confirm.sh`: `pgrep` and `systemctl` stubbed, fakes at the given paths: each check's stop;
    the unit inactive, no `flaminio-io`, two, one whose PID is not `MainPID`; the disk check
    with none, two, the wrong one, a symlink to the right one; the second delete finding 1;
    `confirm` printing `{}`;
  - `delete_volume`, `write_volume`, `vol-confirm`, the campaign's start and the restore,
    extracted from `lab.sh` as `scripts/checks/resync.sh` extracts `metactl`, with fake
    `run_on_vm`, `vssh`, `metactl`, `cmd_node`, `ensure_node_c`, `sleep` and the build and
    install, recording calls and their `FORMAT`, `TOMBSTONE_WAIT=1`:
    - `delete_volume`: never a `confirm`; a detach answering exit 1: stop, no metadata delete;
      255: 255; `NotFound` goes on, another error stops; fl-b stopped with a raw disk, through
      `delete_wbench` and the `$VOL` removal: started with `FORMAT=1`, cleaned, the campaign
      resumes; fl-c as the format node, formatted; only the format node ever gets `FORMAT=1`
      (fl-a stopped while `NODE=fl-b`; fl-c stopped during `vol delete`: `FORMAT=0`); a failed
      start; the volume found again at step 4: no `node.sh delete`; `not running` at the
      clean; pending for fl-c: stops at once, naming fl-c, the procedure with the command to
      run again; a failed poll then gone: success; time-out: 1, or 255 after a 255 poll;
      `unknown command`: the hint;
    - the campaign's start: `NODE` stopped, `$VOL` listed before another volume X, both on
      `NODE` (X through a moving chunk too): exit 1 naming X, no `FORMAT=1` start, no delete;
      only `$VOL` on `NODE`: removed;
    - the restore: no `drain` or `undrain` call;
    - `write_volume`: creates only once `NODE` is alive; a chunk without a replica there: stop;
    - `vol-confirm`: `c`, `fl-c` accepted; `a`, `all`, `cluster`, `fl-w` refused before any
      install; the install only when the hash differs; not listed: exit 0, no delete; a failed
      sweep check: no detach, no delete; a node still attached, or `not running` with
      `flaminio-io`: stop, no delete; the re-check failing: no confirm.
- **shellcheck** (`make lint`).
- **The lab:** as the spec lists, after `make meta-up` and S8.

## Risks

- **Procedure, not mechanism, keeps fl-c to one confirmer** (decisions, accepted risks).
- **Formatting a stopped campaign node** at a deletion loses its other replicas: the campaign's
  start has refused any other volume there, but one placed there during the campaign is lost
  at the next reset, as today.
- **A campaign with replicas on fl-c** needs an operator at every deletion.
- **The start of a stopped node** in `delete_volume` costs a reconciliation of its other
  volumes.
