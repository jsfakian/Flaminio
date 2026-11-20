# Issue #71 plan review — round 2 (incremental), security focus

Scope: `git diff 76cf82f..6a9d1a3 -- docs/issues/71/ ':!docs/issues/71/report.md'`. The plan was
cut to read-only step 1, so the changed documents were read in full and checked against the
worktree `/tmp/flaminio-plans-2026-11-12/71` and #69's draft
(`/home/jsfakian/Documents/src/flaminio-69/docs/issues/69/design.md`). These claims hold in the
code: the open reads and the role rules (`meta/service/auth.go:58–83`); the free-form component
and the usage bits (`pkg/pki/pki.go:111–117`, `234–235`); Go servers enforce client EKU
(`RequireAndVerifyClientCert`, `pki.go:285–296`), and so do replicas (`WebPkiClientVerifier`,
`flaminio-net/src/tls.rs:71`) on top of `node_with("io")` (`session.rs:116`); the control socket
is 0600 and serves attach, detach, delete and status (`control.rs:46–52`, `83–111`). #69's
four-method reader allowlist matches intent.md:44–47.

## Earlier findings
- R1-S1: fixed (mutating verbs and preconditions cut).
- R1-S2: fixed (separate socket, non-root observer, server-only `observer` identity). How that
  credential is issued and read is new: R2-S1.
- R1-S3: fixed (pull; `node` from the scrape target; ≤ 4 targets; `sample_limit`). Residue: R2-S3.
- R1-S4: fixed (push cut).
- R1-S5: fixed (Events deferred, with an envtest check before the kinds' scope is chosen).
- R1-S6: fixed (plans cut).
- R1-S7: fixed (no cluster binding remains).
- R1-S8: fixed (traces deferred).
- R1-S9: fixed (no collector; each metadata instance exports its own view, and rules take the max).
- R1-S10: fixed (`recover` cut; reader rights match #69).
- R1-S11: fixed (summaries cut).
- R1-S12: fixed (exact allowlists, 403 tested; the reader's breadth is a stated open question).
- Nit 1 (`decisions.md` link): fixed. Nit 2 (`node/<n>/meta` Push): fixed (push cut).

### R2-S1 — Medium
**The non-root observer's credential has no issuance, file access or deployment check, and
Prometheus cannot check its identity.**
- The lab makes every key as root in `/etc/flaminio/pki`. `pki.sh` re-applies mode 0700 to that
  directory at every call and creates keys with umask 077 (`scripts/guest/pki.sh:8–10`, `15–16`).
  The same directory holds the capability-token signing key (`scripts/lab.sh:579`). The design
  runs the observer as `flaminio-observer` (design.md:81–84) but never says where its key and CA
  bundle live or who owns them. As written, the observer cannot read its key, and lab step 1
  fails. Both obvious workarounds undo R1-S2: running the observer as root, or opening that
  directory.
- No issuance line is given. lab.sh is only listed as a file to change (design.md:135). The CLI's
  "usual CA and host checks" (design.md:108–110) need an IP SAN for the `Node.address` host, as
  `meta` and `io` get (`lab.sh:583–584`).
- Prometheus checks only the CA and a DNS or IP SAN. It cannot check the URI SAN. The ADR's
  "every series is bound to an authenticated target" (adr-proposal.md:44–46) and decisions.md:22–23
  therefore rest on the node-IP SAN and on `prometheus.example.yaml` keeping verification on
  (design.md:117–119 says nothing about its `tls_config`). Even then, any workload server
  certificate for that IP passes. A compromised observer on a metadata node can serve :7402 while
  the metadata listener is down. Prometheus then sees `up=1` and a forged
  `flaminio_meta_view_ok 1`, which hides that instance's `FlaminioTelemetryLost`.
- The startup check "exits 1 unless exactly `node/ID/observer` with server usage only"
  (contracts.md:103–105) has no test (design.md:153). The lab never checks the deployed
  observer's privileges.

Fix:
- Issue `observer` with `--server --ip "$ip"` into a directory the observer user owns. For
  example, add an owner and directory option to `pki.sh` for `/etc/flaminio/observer`, mode 0700.
  Leave `/etc/flaminio/pki` root-only.
- State that `prometheus.example.yaml` verifies the CA and the IP SAN, with no
  `insecure_skip_verify` and no `server_name`. Record the residual: a node-IP certificate stands
  for any role on that node.
- Add `TestStartupRefusesIdentity`: client or dual usage, another node, another component.
- Add a lab step 1 assertion: as `flaminio-observer`, connecting to `node.sock` and reading
  `/etc/flaminio/pki` both fail, and connecting to the telemetry socket succeeds.

### R2-S2 — Low
**The telemetry socket's directory is unspecified, and a group-writable choice would hand the
group a root chmod/chown.**
- The lab path is `/run/flaminio/telemetry.sock` (contracts.md:8–11). That directory is 0700
  whenever `node.sh up` runs with `ATTACH`, and it then holds the bearer token file
  (`scripts/guest/node.sh:61–62`). It is also the CSI node plugin's hostPath
  (`deploy/csi/driver.yaml:132`). In the 0700 case the observer cannot reach the socket.
- The pattern `control.rs` uses (unlink, bind, then a path-following `set_permissions`,
  `control.rs:49–52`), plus the chown to root:GID this socket adds, follows a symlink. If the
  socket's directory is writable by the group, a member can swap the socket for a symlink between
  bind and chmod/chown. That makes root grant mode 0660 and group GID on any file.
- `mode_and_group` (design.md:151) checks only the socket file.

Fix: specify a root-owned directory that the group can search but not write, for example
`/run/flaminio-telemetry` as root:GID 0750, and keep tokens out of it. Create the socket 0660
under a umask and use `lchown`. Make the daemon refuse a parent directory that is group- or
world-writable, and test that refusal.

### R2-S3 — Low
**The alert rules do not select a job, so one observer can raise a cluster-level alert that no
node is blamed for.** decisions.md:22–23 says "a node cannot write or hide another node's series".
- `FlaminioLostRedundancy` is `max by (volume) (flaminio_volume_under_replicated_chunks)`, with
  no `job` selector (contracts.md:203).
- A compromised observer, an unprivileged process, can export that family with any volume label.
  `max by (volume)` drops `node`, so the critical alert names no source. `flaminio_meta_view_ok`
  in `FlaminioTelemetryLost` (contracts.md:209) is open to the same injection.
- The design relies on `honor_labels: false` without stating it (design.md:92, 117–118).

Fix: put `{job="flaminio-meta"}` on the metadata families and `{job="flaminio-observer"}` on the
node families. State `honor_labels: false` in `prometheus.example.yaml`. Add a promtool case: an
observer-job series of a metadata family fires nothing.

### R2-S4 — Low
**The CLI prints observer-supplied strings that its output validation does not cover.**
- Doctor items print `evidence` (contracts.md:178), and the `mtu` evidence embeds
  `network.interface` (contracts.md:142). Both arrive from `/v1/node` as free strings
  (contracts.md:120–122).
- The CLI rejects a reply only for size, keys, schema and node (contracts.md:125–128). Its output
  rule admits "validated ids, UUIDs, enums, numbers and addresses" (contracts.md:183–185), which
  covers neither field.
- A compromised node can therefore put terminal control sequences into an operator's doctor
  table. #69 forbids this for its own output.

Fix: have the CLI check `evidence` against the fixed grammars in contracts.md:140–143 and
`interface` against `[a-zA-Z0-9._-]{1,15}`, and treat a mismatch as `invalid`. Add a test where an
observer returns control bytes in both fields.

### R2-S5 — Low
**After an engine timeout, `observe` can make the engine build many snapshots at once.**
- On a cache miss, the socket thread sends `Input::Observe` and gives up after 2 s
  (design.md:74–76). It keeps nothing pending.
- The engine channel is an unbounded `mpsc::channel` (`flaminio-io/src/node.rs:288`). While the
  engine stalls, each later poll queues another `Observe`.
- When the engine resumes, it builds them back to back, in the queue pump that holds the engine
  mutex. This breaks S3's "at most once a second" (spec.md:42). `the_engine_part_is_cached_for_a_second`
  covers only a live engine.

Fix: keep the outstanding reply receiver, and send no new `Observe` while one is pending. Answer
from the cache or with the timeout error instead. Test that, with a silent engine, N requests
leave at most one `Observe` queued.

## Nits
- contracts.md:168 `volumes_recovering` "counts reported progress" from any observer, while
  `volume` takes progress only from the metadata target's observer (design.md:112). Say "from
  each volume's target observer". Otherwise a non-target node's entry is counted.

Reviewed commit: da0177afcf1ee397976f3ba904714105d671695d
