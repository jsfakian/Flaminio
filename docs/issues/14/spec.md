# Issue #14: Tombstones on nodes outside Kubernetes — specification

Step 2 of the issue ([intent](intent.md)).

## Terms

- **Tombstone:** `/flaminio/v1/tombstones/<volume>`. The nodes the deleted volume's chunk
  records named (`nodes`), and those that have confirmed (`confirmed`). Keyed by the name alone.
- **Pending:** named on a tombstone, not confirmed.
- **Confirming for a node:** recording that the node holds no replica of the volume any more.
  The last confirmation removes the tombstone, and the id is free.
- **Confirmer:** whatever confirms for a node: its CSI node plugin's sweep, or an operator.
- **Node without a sweep:** a data node that is not a k3s agent (`k8s up` makes agents of
  `vms_of all`, fl-a and fl-b). In the lab, fl-c.
- **Administrator caller** (S2): a client certificate whose SPIFFE ID is an administrator's, or
  no client certificate at all (a server without authentication lets it do anything).

## The invariant

While a node is pending on a tombstone, only a confirmer for that node can remove the tombstone,
and so let the id be created again (`createVolume`'s tombstone check and claim; `ConfirmDeletion`).
Every hazard below needs two confirmers for one node: one that confirms while the other has yet
to delete, or confirms a tombstone the other's deletion was not for. **Exactly one confirmer per
node:** the CSI sweep on fl-a and fl-b; an operator, through `vol-confirm` (S10), on fl-c. The
harness never confirms.

## Behaviour

### The commands

**S1. `flaminio-meta tombstones [node]`.**
- **With a node:** the tombstones pending for it (`ListTombstones`).
- **Without:** every tombstone pending for a registered node (`ListNodes`, then `ListTombstones`
  for each), each once. A tombstone pending only for a node that is not registered is not
  listed, and fails safe: it still keeps its id from being created again.
- **Output,** JSON as the other commands: `{"tombstones": [{"volume", "nodes", "pending"}]}`,
  by volume. None: `{}`. On an error, nothing on stdout.
- It changes nothing.

**S2. `flaminio-meta confirm <volume> <node> --replicas-deleted`.**
- **Arguments:** exactly the volume, the node, and the token, exactly as written, before or after
  the positionals. Without it, or with anything else (a third argument, another flag,
  `--replicas-deleted=false`, `-replicas-deleted`, a misspelling): it refuses, exit 1, says what
  the token asserts, and calls nothing. The flags before the command are unchanged.
- **An administrator caller** may confirm only for a node that is not alive (`ListNodes`): the
  node gone for good. For a live node it refuses, exit 1, and points to `vol-confirm` or the
  node's agent. When it does confirm, it warns on stderr that its certificate is the only gate.
  A node's agent (its certificate's node is the argument) has neither rule.
- **First it reads** the node's listing (S1). On stderr: the caller's identity (the SPIFFE ID in
  its certificate; without one, "no client certificate"), the volume, the node, and what the read
  found: the tombstone's nodes and pending nodes, or none pending for the node.
- **Pending for the node:** it calls `ConfirmDeletion`, and on success exits 0. Stdout: the
  tombstone as the read found it, with `pending` less the node.
- **Not pending** (no tombstone, the node not on it, or confirmed already): it calls nothing,
  prints `{}`, says on stderr that there is nothing to confirm, and exits 0.
- **The output comes from the read:** `ConfirmDeletionResponse` is empty (`meta.proto`). Between
  the read and the call, another confirmer may act, or the volume be created and deleted again;
  the command cannot see it. The service's log (S5) is the only witness.
- **The usage text** (doc comment and usage line) says: use it only for a node whose replicas of
  the volume were deleted after this deletion, in the same sequence, or that is gone for good
  and will not come back with its disk; never for a node with a sweep; never from shell history,
  nor while anything else may delete or create the same id. A node confirmed while it holds
  replicas keeps them, and the next volume with that id adopts them and serves the old data
  (S6).

**S3. Who may run them.** Unchanged (`Authorize`, `meta/service/auth.go`): an administrator for
any node; a node's agent (component `agent`) for that node only; anyone else `PermissionDenied`.
`tombstones` without a node needs an administrator. S2's rule for administrators is the CLI's.

**S4. Errors and parsing.** As the other commands: the service's error on stderr (`rpc error: code
= <code> ...`), exit 1; an unknown command too (`unknown command "tombstones"`, from an older
binary). The client's deadline, 30 s, covers the whole command. The client takes its arguments
and its output and error streams, and returns an error; only `main` exits. Its flag sets use
`ContinueOnError` and write to the given error stream.

**S5. The service, as relied upon.** Unchanged (`meta/service/service.go`), but for one log line:
- `ListTombstones` returns the tombstones that name the node and that it has not confirmed.
- `ConfirmDeletion` records the node; the last confirmation deletes the tombstone. Its write is
  conditional on the tombstone's revision and retried, so concurrent confirmations all count.
- `CreateVolume` refuses an id with a tombstone: `AlreadyExists`.
- **New:** a `ConfirmDeletion` that newly confirms a node (named, not confirmed before) logs one
  line (`slog.Info`), once, after its write succeeds, not per attempt: the caller's identity,
  the volume, the node, the nodes still pending (none: the tombstone is gone), and the write's
  etcd revision. Its write goes through `TxnRev` instead of `Txn`, which returns no revision.
  Every caller, agents included. Without authentication (`--insecure`) the identity is
  "unauthenticated". A call for no tombstone, or for a node confirmed already (which rewrites the
  same value), logs nothing.

### The lab

**S6. Why the replicas go first.** A volume created again gets the same store identifier on every
node (`volume_uuid`, `flaminio-node/src/lib.rs`: a hash of the name), and its target's `hello`
creates only missing chunks (`replica/mod.rs`). Old replicas fenced at the new target's epoch or
below become the new volume's chunks: it serves the old data. Old replicas fenced above depose
its target (`delete_wbench`'s comment).

**S7. `node.sh delete` says what it did:** on stdout one line, `deleted <n>`, exit 0, when the
control socket answered `ok <n> replicas deleted`. Otherwise exit 1 as now, with `not running`
on stderr when flaminio-node is not running. The socket's path is a variable of `node.sh`, passed
to `python3` as an argument (`sudo` resets the environment), so that a test can point it
elsewhere.

**S8. One deletion: `delete_volume <volume> [format-node]`.** Shared by `delete_wbench` (the
bench node as format node), `bench-writes`' removal of `$VOL` at its start, and `vol delete`
(`make vol-delete`, which deletes the metadata only today). It never confirms. In order:
1. **Detach** it on every data node (fl-a, fl-b, and fl-c once it exists). Failures are
   tolerated, as today, but for ssh's 255.
2. **Delete** it from the metadata service. `NotFound` (exit 1, `code = NotFound` on stderr) is
   fine: a run before did. Any other error stops the harness.
3. **Start** every data node that is not running: the format node with `FORMAT=1` (the campaign
   resets its disk anyway, and a raw run has overwritten it), the others without. A start that
   fails stops the harness.
4. **Clean** every data node (`node.sh delete`), logging `deleted <n>`. Any failure stops the
   harness, naming the node.
5. **Pending on a node without a sweep:** if `tombstones` (as the administrator) shows the volume
   pending for fl-c, the harness stops at once, and prints S10's procedure for that volume.
6. **Wait** until no node lists the volume, polling every 5 s, for at most `TOMBSTONE_WAIT`
   seconds (default 120), in its own loop (`wait_for` cannot name the pending nodes). A poll that
   fails, 255 included, is retried until the time-out. On time-out it stops, naming the pending
   nodes (their CSI node plugin may not be running: `make csi-deploy`).

- **Exit status:** ssh's 255 from any step is the harness's, so that `RETRIES` resumes; so is a
  time-out whose last poll failed with 255. Any other stop: 1.
- **`unknown command`** from `metactl` stops the harness with a hint: `make meta-up`.
- **Step 6 comes before the harness stops a node or formats its store** (`write_reset`,
  `write_whole`, the restore): a stopped node's sweep cannot delete.
- **When step 5 stops a campaign:** whenever wbench or `$VOL` has a replica on fl-c: `RF=2`
  (its second replicas go to fl-a or fl-c by load, `domain.Place`), `NODE=c`, or a `$VOL` with a
  chunk there. Then every `write_reset` and `write_whole`, or the removal of `$VOL`, stops there
  until `vol-confirm` runs. A normal campaign (RF 1, `NODE=b`, `VOL=wbench`) never does.

**S9. Not changed: concurrent runs.** Two harness runs at once can delete each other's replicas,
as `delete_wbench` can today. No workflow deletes volumes through `bench-writes` or the `vol`
commands (`.github/workflows/lab.yml` runs `vol show` only); two operators could. S13 records a
lab lock as a follow-up.

**S10. `lab.sh vol-confirm <volume> <node>`,** run by an operator, never by the harness, with no
`bench-writes` or `vol` command running. In order; any step that fails, or cannot tell, stops it
and confirms nothing:
1. **Not a cluster member:** a node of `vms_of cluster` (fl-a, fl-b, fl-w) is refused, before
   anything is installed.
2. **Install** `flaminio-meta` on the node, built as `meta up` does, when its hash differs from
   the one there.
3. **Listed:** `tombstones <node>`, as the node's agent (its certificate, from `cmd_pki issue`),
   lists the volume. Not listed: it stops, with nothing deleted ("not listed: nothing done").
4. **No sweep on the node:** no process named `flaminio-csi` or `flaminio-agent` (`pgrep -x`);
   `k3s` and `k3s-agent` each `inactive`, `disabled` or `not-found` (`systemctl is-active`,
   `is-enabled`); anything else stops. Issue #13's agent sweep unit, when it comes, extends this
   check.
5. **Detached everywhere:** it detaches the volume on every data node, then every one must
   answer `not attached` (a node not running counts as not attached). An old target that
   reconnected would create the replicas again.
6. **The right disk,** before the delete: `flaminio-agent disks`, as root, without `--serial`,
   with `agent.sh`'s `--disk-tool`, shows exactly one `ours` disk, and its `readlink -f` is the
   running node's `--device` (`systemctl show` of `flaminio-node`), so that no replicas are left
   on another disk.
7. **Delete:** `node.sh delete` of the volume prints `deleted <n>`.
8. **Still listed:** `tombstones <node>` lists the volume again.
9. **Confirm:** `confirm <volume> <node> --replicas-deleted`, as the node's agent.
10. **Witness:** it prints S5's log lines for the volume from the metadata service on every
    cluster member (fl-a, fl-b, fl-w) since step 3. A line from another caller for this node, or
    none from this run, shows a second confirmer.

Steps 3, 4, 6 to 9 run on the node through one guest script, `scripts/guest/confirm.sh`, with
two parts (before and after the detach), which takes the paths of `flaminio-meta`,
`flaminio-agent` and the disk tool as variables. **Left open:** a sweep started on fl-c between
step 4 and step 9 (seconds), and a creation and deletion of the id in the same window, would be a
second confirmer; S13 records it with the lock.

**S11. The stale tombstones, once,** by an operator, before the lab check, recorded in the PR:
`make meta-up` first; then `vol-confirm <volume> fl-c` for every tombstone that `tombstones fl-c`
lists (wbench, volb, any `smoke-*` or `resync-*`; `resync-*` can hold data on fl-c); then
`metactl tombstones` lists none of them.

**S12. Unchanged.**
- The metadata service but for S5's log line and `TxnRev`; the proto, comments included (a
  comment change regenerates `api/gen`, which `make contracts` compares); the caller rules.
- The CSI sweep: delete, then confirm, every 5 s.
- `DeleteVolume`, and which nodes its tombstone names (docs/issues/8, S1).

**S13. Docs, help and lint.**
- `docs/lab.md`: the prerequisites of `bench-writes` and `vol delete`, in order: `make meta-up`
  (the new commands); fl-c's VM and node; Kubernetes and the CSI node plugins on fl-a and fl-b
  (`make csi-deploy`). The recommended campaign settings (RF 1, `NODE=b`, `VOL=wbench`), and
  when a campaign stops at fl-c (S8). Then the two commands, S2's rules, S8, `vol-confirm` and
  S9.
- `make vol-delete`'s help and `cmd_vol`'s usage: it deletes the replicas too; `vol-confirm` in
  `lab.sh`'s usage.
- `make lint` runs the new script tests (it names each one).
- Follow-ups §14: step 2 done, with the lab check's result. Later steps: step 1 (the agent's
  sweep, planned in issue #13) removes `vol-confirm`'s use for fl-c in the same change, or there
  are two confirmers; a lab lock (S9, S10's window); the tombstone incarnation; the paths that
  keep leaving fl-c tombstones; fl-c's token key and etcd certificate (intent, out of scope).

## Tests

- **`meta/service/auth_test.go`:** an administrator lists and confirms for any node; an `io`
  identity may not confirm.
- **`meta/service/service_test.go`:**
  - two confirmations of different nodes of one tombstone, interleaved after the first's read
    (`interleaveStores`): both count, the tombstone is gone, and each logs exactly once;
  - S5's log line, with its fields and the revision, for an administrator and for an agent (the
    identity set in the context, as `Authorize` sets it), "unauthenticated" without one; nothing
    for no tombstone, nor for a second confirmation of the same node. The test captures `slog`'s
    default logger, safe as the package's tests do not run in parallel.
- **The commands** (`meta/cmd/flaminio-meta`, no tests yet), through S4's client, against an
  in-memory service on a localhost gRPC server. Without TLS the caller counts as an
  administrator, so those cases confirm for nodes that are not alive (the store's clock past the
  liveness TTL):
  - two tombstones over three nodes: `tombstones` lists each once, by volume; `tombstones
    <node>` the node's; a node with none: `{}`; a tombstone written directly for a node that is
    not registered: not listed, and `create` of its id still refused;
  - `confirm` without the token, with `--replicas-deleted=false`, `-replicas-deleted`, a
    misspelling, a third argument: exit 1, nothing changed;
  - `confirm` of a pending node, the token before and after the positionals: `pending` without
    it, and stderr names the identity, the volume, the node and the read. Of the last one: no
    `pending`, and `create` of the id succeeds;
  - `confirm` again, of an unknown volume, of a node not on the tombstone: `{}`, exit 0, no call;
  - **with TLS** (certificates as `auth_test.go`'s `writeIdentity` makes them, copied): as an
    agent, `tombstones` without a node: `PermissionDenied`, exit 1, nothing on stdout; `confirm`
    for another node: `PermissionDenied`; for itself while alive: confirmed. As an
    administrator, `confirm` for a live node: refused, exit 1, nothing changed; for a node that
    is not alive: confirmed, with the warning.
- **The scripts, locally** (`scripts/tools/`, named in `make lint`, so CI; each with
  `bench-reference-test.sh`'s bash 4.4 guard, and its fake socket in a short temporary directory,
  under macOS's 104 bytes):
  - `node.sh delete`, with `common.sh` prepended as `run_on_vm` does, `sudo` and `systemctl`
    stubbed on the `PATH`, and a real `python3` against a fake socket server at the path given:
    `deleted 2` for `ok 2 replicas deleted`; exit 1 and `not running` for an inactive unit; exit
    1 for an error answer;
  - `confirm.sh`, the same way, with `pgrep` stubbed and fakes at the given paths of
    `flaminio-meta`, `flaminio-agent` and the disk tool: it confirms only when every check
    passes; not listed: no delete call; a `flaminio-csi` or `flaminio-agent` process, or `k3s` or
    `k3s-agent` active or enabled: stop; the disk check: no `ours` disk, two, one other than the
    unit's `--device`, a symlink to the right one (passes); a delete error, not listed any more
    after the delete: stop, no confirm;
  - `vol-confirm` and `delete_volume`, extracted from `lab.sh` (as `scripts/checks/resync.sh`
    extracts `metactl`), with fake `run_on_vm`, `vssh`, `metactl`, `cmd_node` and `sleep` that
    record the calls, and `TOMBSTONE_WAIT=1`:
    - `vol-confirm`: a cluster node is refused before any install; a node still attached
      elsewhere after the detach: stop, no delete;
    - `delete_volume`: no `confirm` call in any case; detach failures tolerated, 255 not;
      `NotFound` at step 2 goes on, another error stops; fl-b's node stopped, with wbench in the
      metadata: started with `FORMAT=1` as the format node, then cleaned; a failing start stops;
      a clean failing stops; pending for fl-c: stops at once, printing the procedure; a poll
      that fails, then the tombstone gone: success; pending for fl-b past the time-out: exit 1,
      naming fl-b, or 255 if the last poll failed with 255; `unknown command`: the hint; the
      `$VOL` removal at `bench-writes`' start goes through it.
- **shellcheck** covers the changed scripts (`make lint`).
- **The lab, a later step,** after `make meta-up` and S11, with fl-c's node, Kubernetes and the
  CSI node plugins on fl-a and fl-b:
  - `make bench-writes`, one job, two repetitions (`JOBS=randwrite-4k-qd1 REPS=2`, `VOL=wbench`,
    short `IDLE`, `SETTLE` and `ROUND`): every Flaminio run deletes and creates wbench twice
    (`write_reset`, `write_whole`), and all pass. Afterwards `metactl tombstones` lists no
    wbench;
  - the `$VOL` removal: `VOL=volb make bench-writes` with volb on fl-b and fl-c stops at step 5
    with the procedure; `vol-confirm volb fl-c`; run again, it goes on;
  - `VOL=volb make vol-delete`, then `vol-create`, twice, with `vol-confirm` whenever step 5
    stops;
  - **fl-c's node stopped:** create wbench at RF 2 with fl-c preferred (`create wbench 16G 2 1
    256M fl-c`), attach it and write to it (chunks exist on a replica only after a target's
    `hello`), detach it, stop fl-c's node, then `VOL=wbench make vol-delete`: step 3 starts it,
    step 4 prints `deleted <n>` for fl-c, n > 0, and step 5 stops with the procedure. Stop
    fl-c's node again: `vol-confirm wbench fl-c` stops before its delete, nothing confirmed.
    Start it: `vol-confirm` confirms, and no tombstone is left.
