# Plan review: issue #12, round 1, code focus

Scope: `docs/issues/12/` (intent, spec, design, decisions) and `docs/adr/0007-privileged-data-plane-pod.md`
at `6f70e98`, against the worktree's code (base `44b2c87`) and, where the plan will land, `main`.

### R1-C1 — Medium
**#12 and #74 set contradictory rules for deleting ublk devices in the unit scripts.**
- #12: `node.sh down` / `ublk.sh down` delete a device only while their own unit is active. `ublk.sh up`
  refuses if `/dev/ublkc0` exists and prints the `del` to run (design.md:166-197).
- #74: `down` stops the unit, then deletes a remaining `/dev/ublkc$id`; `node.sh up` always runs `down`
  first; `ublk.sh up`, with its unit inactive, deletes a leftover `/dev/ublkc0` (74/design.md:209-214).
  #74 needs this because its libublk patch stops a failed add from deleting the id through `Drop`.
- In a pod, the kernel picks the id (decisions.md:58-61), so the pod's first device is usually id 0.
  Merged in the wrong order, #74's `ublk.sh up` deletes a live pod device on fl-a or fl-b.
- #12's plan never mentions #74.

Fix: in #12, name #74 and state the merged rule. A unit script deletes `/dev/ublkcN` only after the
pod-node guard passes (the socket is absent and no data-plane sandbox is ready); then any device left is
dead and may be deleted. Add this case to the script tests (R1-C2).

### R1-C2 — Medium
**The plan is written against a base two weeks behind `main`, and `main` changed what it relies on.**
- On `main`, `node.sh detach` and `delete` refuse when the unit is inactive: they print `not running`
  and exit 3 (`running()`, main node.sh:95-120). `delete` also validates `VOLUME`. `control()` moved to
  common.sh.
- `main`'s lab.sh acts on that exit 3:
  - `detach_all` (:1687);
  - `detached_everywhere`, which dies when `pgrep -x flaminio-io` finds a process (:1703-1716);
  - `vol-delete`, which starts the unit to delete replicas: "its node is not running; starting it"
    (:1743).
- On a pod node:
  - `make vol-delete` reads the pod's node as "not running";
  - it then starts the unit, which #12's guard refuses, or dies on `pgrep`.
- The intent's "`node.sh delete` does nothing without it" (intent.md:19-20) is false on `main`.
- The design's list of commands that test the socket instead of the unit is `attach`, `delete`,
  `status` (design.md:158). It omits `detach`, which `main` gates on the unit.
- `main` has `scripts/tools/node-scripts-test.sh` (`FLAMINIO_SCRIPT_TEST=1`). The plan names no script
  test for its shell rules: the `socket` sentinel, the host's `grep -qx` match, the guard, the refusals
  of `down`, and the token files. They have lab tests only.

Fix: rebase the plan on `main`. Restate the `node.sh` rules there: `detach` included, and what exit 3
means on a pod node. Cover `vol-delete`, `detach_all` and `detached_everywhere` on pod nodes. Name cases
in `node-scripts-test.sh` for each new shell rule.

### R1-C3 — Medium
**`make csi-undeploy` does not implement S4, and its lab test cannot fail.**
- S4 says undeploying the driver refuses unless every pod node's socket, asked from the host, reports
  nothing attached (spec.md:75-79). The design checks VolumeAttachments instead (design.md:251-252).
- That misses an attach made through the socket, such as `make vol-attach` on a pod node. The
  DaemonSet is then deleted, and the volume is crashed when the 3600 s grace runs out.
- It also refuses spuriously: right after `delete namespace --wait`, the VolumeAttachment usually still
  exists until ControllerUnpublish runs.
- The step list (design.md:350-354) drops the driver's own deletion, which `undeploy` does today
  (lab.sh:1051-1057).
- Lab 6 expects `csi-undeploy` to refuse with `make csi-test KEEP=1` (design.md:465, :472). But that
  test pod lives in `flaminio-test`, which `csi-undeploy` deletes first. The refusal never happens, and
  the later steps of lab 6 lose their attach.

Fix:
- Delete the test namespace.
- Wait, bounded, for its VolumeAttachments to go.
- Run the same ssh `node.sh socket` check as `k8s-down`.
- Then delete the DaemonSet, the driver and the labels.
- In the test, hold the attach outside `flaminio-test`: the upgrade load, or `make vol-attach`.

### R1-C4 — Medium
**Template values go unquoted into shell strings, against a Principle, and #43's settled dependency is
unmet.**
- `--serial {{SERIAL}}` is in the init script (design.md:34-38). `--meta {{META_ENDPOINTS}}` and
  `--trust-domain {{TRUST_DOMAIN}}` are inside `sh -c '...'` (design.md:44-49).
- `render` substitutes the raw value (lib.sh:199-208). A serial regexp such as `a|b`, or a quote,
  becomes shell syntax.
- AGENTS.md: "Ids reaching shell commands ... are validated and quoted."
- `node.sh detach` deletes `/run/flaminio/<VOLUME>.token` (design.md:161) with an unvalidated `VOLUME`:
  `../` reaches outside the directory under `sudo rm`.
- #43 settled that #12 puts the ServiceAccount `flaminio-dataplane` in `deploy/csi/rbac.yaml` and bounds
  its own values (43/decisions.md:75-78; 43/design.md:21-24). #12's design creates the account but
  says neither where (design.md:17-18).

Fix:
- Run the claim in exec form (`[flaminio-agent, claim, --serial, ...]`), with no `sh -c`.
- Pass the data plane's values as env vars, quoted in `sh` (`"$META"`), or give `flaminio-io` a
  `--device-file`.
- Validate each value's grammar in lab.sh before rendering.
- Validate `VOLUME` (`main`'s `volume_id`).
- Put the ServiceAccount in `rbac.yaml`.
- Add these to the manifest test.

### R1-C5 — Medium
**The data-plane pod's PKI mounts break #16's rotation.**
- The design mounts `io.crt` and `io.key` as single-file `File` hostPaths (design.md:59;
  decisions.md:167).
- A file replaced by a rename stays the old inode inside the pod.
- #16's proposed ADR rotates certificates by replacing files, and says "a directory, not a file, must be
  mounted into a pod" (16/docs/adr/0007, Options 1). #16 changes only the CSI node plugin to a
  `Directory` (16/design.md:101), since #12 is not in its tree.
- With 24-hour certificates, the data plane's replication identity expires in a running pod.

Fix: mount one read-only directory holding `io.crt` and `io.key` (and keep `ca.crt` and `token.pub`
as files, as #16 does), or record that #16 must convert this mount. Assert it in the manifest test.

### R1-C6 — Medium
**Lab 5's expected steps do not follow the upgrade order.**
- The order is: nodes without a VolumeAttachment first, then the others, by name (design.md:263-264).
- **RF 1 "with no load pod"** (design.md:455-456): the volume has no VolumeAttachment, so fl-a goes
  first. fl-a passes step 2, since none of the chunks are there. It then refuses at fl-a's step 4,
  after 300 s, and never reaches "fl-b's step 2". The RF 1 rule stays untested.
- **The `minIsr` case** (design.md:457): if it is served by `make vol-attach`, fl-a goes first and
  refuses at fl-a's step 2.
- **"ISR not full"** (design.md:458-459): it refuses only if fl-b's container is killed before fl-a's
  target detaches. After the detach, no live target shrinks the ISR, and the load's new target on fl-b
  is crashed instead.

Fix: serve the RF 1 and `minIsr` volumes from a controller-owned PVC pod on fl-a, so that fl-b goes
first. Kill fl-b's container while the load pod is still Terminating on fl-a.

### R1-C7 — Low
**The init script does not stop on a failed check.**
- The three lines run under `sh` (design.md:34-38) with no `set -e`.
- A failing `flaminio-agent check` is ignored, and the claim decides the exit code. S2's kernel gate
  depends on that line.

Fix: `sh -ec` or `&&`, asserted by the manifest test.

### R1-C8 — Low
**The euid check cannot be written as specified.**
- The design checks the directory's owner against the euid with "safe `std` only" (design.md:109-116).
- `std` has no process euid. `libc::geteuid` needs `unsafe`, which the workspace forbids
  (dataplane/Cargo.toml:40), and `forbid` cannot be allowed locally.

Fix: name the source. Either a safe `euid()` in `flaminio-uring`'s new module, or the owner of
`/proc/self` (or of a file the process just created).

### R1-C9 — Low
**Attaches are still accepted after SIGTERM.**
- `begin_attach` does not refuse once `requested()` is true.
- An attach can arrive between V's teardown and the main loop's check (up to 50 ms, ublk.rs:755-761),
  or between preStop's `none` and SIGTERM. It then defers the exit: "exits after the next detach"
  (spec.md:71-72) fails, and the new volume is SIGKILLed when the grace runs out.

Fix: refuse with "the node is shutting down" once SIGTERM is requested. Add a unit test.

### R1-C10 — Low
**Who is asked is unspecified.**
- `served_isr_full` asks "each data node's status" (design.md:270-272). It must include fl-c when it
  exists (`main`'s `data_vms`), and must refuse when a node cannot be asked. Otherwise an RF 1 chunk on
  fl-a served from fl-c counts as unserved.
- `k8s-down` asks "each pod node" without Kubernetes (design.md:243-249), but no VM list is given. With
  `vms_of all`, a unit serving a volume blocks `k8s-down` needlessly.

### R1-C11 — Low
**Raw-disk benchmarks are not guarded.**
- `fio-raw.sh` ("DESTROYS everything on the data disk", opened without `O_EXCL`) and `store-bench.sh`
  (`FORMAT=1` by default) use `data_disk` with no guard.
- Pods now start at boot, so on fl-a and fl-b a store is always live. The plan guards only the units'
  `up` (design.md:182-191).

Fix: run the pod-node guard in both scripts.

### R1-C12 — Low
**The base image's digest must be the index digest.**
- The base is pinned by digest for both images (design.md:73-74).
- `image.sh` also builds the demo's arm64 CSI image (lab.sh:1018). A per-platform manifest digest
  breaks the Mac demo, which S7 keeps unchanged.

Fix: say it is the multi-arch index digest.

### R1-C13 — Low
**No plan owns adopting #74's helper in the pod.**
- #74 says "#12 packages the helper" and "#12 runs it in the data-plane container"
  (74/decisions.md:93-94, :115). It also says the pod stays privileged "until #12 adopts the helper"
  (74/intent.md:29-32).
- #12 puts privilege separation out of scope (intent.md:124-125), and ADR 0007 waits for an unnamed
  superseding ADR (adr:87-88).
- #12's SIGTERM design also assumes `flaminio-io` is PID 1 (design.md:44-45). Under the helper it is
  not: #74 asks for an init.

Fix: record a follow-up that adopts the helper in the pod, with #74's list of needs:
- capabilities;
- a fixed id with leftover deletion;
- an init that forwards SIGTERM to `flaminio-io`.

ADR 0007 should name #74's ADR as its successor.

### R1-C14 — Low
**ADR number 0007 is taken four times.**
- This plan's `docs/adr/0007-privileged-data-plane-pod.md` collides with the audit's
  `0007-takeover-readiness-...` (with 0008-0011 on audit/2026-11-12-correctness), #16's
  `0007-certificates-...` and #17's `0007-maintenance-...`.
- #74 cites "#12's proposed ADR 0007". intent, spec and decisions cite "ADR 0007" eight times.

Fix: renumber at merge, after 0011, and cite the ADR by title or by issue (as #43 does) until then.

### R1-C15 — Low
**The Makefile and docs are not aligned with the new commands.**
- The design updates only `docs/lab.md` (design.md:355).
- The `csi-deploy` help still says "(needs node-up)" (Makefile:366).
- The plan gives no Makefile target, help line or `usage()` entry for `make node-upgrade`, and no update
  to `docs/followups.md` §9.
- AGENTS.md asks for these to stay aligned.

### R1-C16 — Low
**S3 understates the protocol change.**
- S3 says the commands are unchanged "except that `status` also reports an attach in progress"
  (spec.md:44).
- The design also changes:
  - the attach refusal: "already attaching" / "already attached", instead of "V is attached here; one
    volume at a time" (control.rs:95);
  - `detach V` during an attach, which now answers `error: V is attaching`.

Fix: list both in S3.

### R1-C17 — Low
**`detach` waits for the wrong state.**
- `detach`'s loop waits for `None` (design.md:137).
- A `begin_attach` of W, set right after V's device stops, makes `detach V` wait through W's attach.
  `detach` then answers "did not stop within 30 s" (control.rs:205-210) for a detach that succeeded.

Fix: wait until the state no longer names V.

### R1-C18 — Low
**Token files outlive a crashed unit.**
- S3 says token files go "when ... the unit stops" (spec.md:52).
- `down` deletes them only when "this invocation stopped an active unit" (design.md:162-163). After a
  unit crash (`Restart=no`) they remain.

Fix: delete them whenever `down` finds the unit inactive and the socket absent.

### R1-C19 — Low
**Untested branches.**
- The startup retry thread's new handling: `Attaching` is retried, `Attached` stops the thread
  (design.md:134-136; ublk.rs:723-742).
- `ublk.sh up`'s refusal of a leftover `/dev/ublkc0`, with no socket, which prints the `del`
  (design.md:195-196).

Fix: a control.rs or ublk.rs unit test, and a script-test case.

### Nits
- **N1:** On exit the main loop "sends `Input::Stop`" (design.md:235). The main thread itself drives the
  engine, and `drive_until` ignores `handle`'s return (node.rs:361-366; engine.rs:630). The step does
  nothing.
- **N2:** `cmd_csi deploy` step 3 says to "read its digest" (design.md:340). Nothing uses it: the
  template names the tag.
- **N3:** A `sigterm` module in `flaminio-uring` contradicts ARCHITECTURE §15.5 ("unsafe ... io_uring,
  ublk and crypto FFI modules") and `flaminio-uring/src/lib.rs:10-11`, which lists the modules. Name
  both updates.

Reviewed commit: 6f70e984153f9e68bb815b28969a8ad7b6fcf20a
