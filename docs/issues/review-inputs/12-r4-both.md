# Plan review: issue #12, round 4 (incremental), design-code and design-security

Diff 0165a65..596b7cb. Design.md line numbers at 596b7cb.

## Round-3 findings
- R3-C1 (down on missing /run/flaminio): fixed. `[[ ! -d ]] ||` is not under sudo; /run is world-readable. Case added.
- R3-C2 (pki.sh modes): fixed (modes pki.sh sets; 0640/group moved to #74's node.sh case).
- R3-C3 (serve order): fixed in the text and by a unit case. Nothing in-repo relies on the dropped text: the only
  match for "attached here" outside the plan is `dataplane.go:119`, which matches "is not attached here"
  (detach), and spec.md:46 already names the new refusal. See M1 for what is left unsaid.
- R3-C4 (node.sh up without io.crt): fixed. Every `cmd_node up` caller (lab.sh 746, 770, 1172, 1273, 1369, 1439,
  1492, 1641, 1867), `cmd_data_node_up` (1219/1228) and `reconcile.sh`/`pvc-loop.sh` is covered; fl-c's first
  start issues `io` first. No path that worked before is broken, except a lab with no `io`, which is the intent.
- R3-B1, R3-N1..N4: fixed, with the Lows below.

## Findings

### R4-L1 - Medium: lab 12's setup is not runnable as written after lab 11
design.md:901-905 and :896-900. Lab 11 ends with `make node-up` units active on fl-a and fl-b (and a driver
deployed over units). Lab 12 starts "after `make csi-undeploy`" and only stops fl-b's unit, at the end.
- `make bench-raw VM=b` runs first, with fl-b's unit still active. The guard "passes with the unit active
  whatever the socket says" (design.md:~373), so `fio-raw.sh` preconditions (writes the whole disk) beneath a
  live store. Today's `bench-raw` has the same hole, but the plan now prescribes this order.
- The final default `make csi-deploy` refuses on fl-a: its unit from lab 11 is still active
  ("`make csi-deploy` over a unit ... refuses there", lab 5, design.md:846-847). Nothing in lab 12 stops it.
- Fix: begin lab 12's setup with `make node-down` (both VMs; the guard passes after `csi-undeploy`), then
  `bench-raw VM=b`, `FORMAT=1 make node-up VM=b`, `make node-down VM=b`, `make csi-deploy`.
- Reformatting fl-b destroys vol replicas that labs 13 and later might expect only if they reuse earlier volumes;
  they create their own (13 uses fl-a's `io-pki`). Nothing else needs fl-b's store; lab 1641/1867 already
  `FORMAT=1 cmd_node up`. State that earlier volumes lose their fl-b replica, so lab 12 runs last or on a fresh lab.

### R4-L2 - Low: the "overrides without the switch" case cannot fail through the stub for `node.sh down`
design.md:~745. `down` now starts with `[[ ! -d $RUN_DIR ]]`, a plain test, so with `RUN_DIR` ignored and the
real `/run/flaminio` absent on a runner, no `sudo find` runs and the stub's refusal is never reached: the script
exits 0. The case's "the stub's refusal fails the script" holds for `pki.sh csr io` (`sudo install -d
/etc/flaminio`) and `k3s.sh server`, not for `down`. Fix: for `down` assert instead that the stub records no
command naming the temp dir and that the planted token in the temp `RUN_DIR` survives; or drop `down` from that case.

### R4-L3 - Low: where in `up` the io.crt check sits is unstated
design.md:155-160 says "before starting the unit". In `up` (node.sh:36-41) `systemctl is-active && down` runs
before the arg setup. If the check follows `down`, a refusal for a missing `io.crt` stops a running unit and
starts nothing. Put it first, with the pod-node guard (a refusal must change nothing), and add to the script
case that a running stubbed unit stays active when it dies.

### R4-M1 - Low: step 2's refusal text is under-specified after `serve`'s check was removed
design.md:236. The old text carried the volume name (`V is attached here; one volume at a time`); the new
`already attached` does not. No caller parses it, but lab scripts and humans read it to see which volume holds the
node. Say the answers are `error: already attached to V` / `error: already attaching V` (spec.md:46 allows either),
or accept the loss in decisions. Also the new unit case pins only shutdown-before-attached; add one for
attached + malformed token -> `already attached` (state before token), which the text claims.

### R4-S1 - Nit: sudo stub scope
design.md:399-402. "any command other than the `systemd-run` stub that names a path under /run/flaminio,
/etc/flaminio or /etc/rancher" is checked by argument text. `sudo sh -c "...'$dir/...'"` (pki.sh:16) and `sudo env ...`
carry the path inside one argument, and `sudo "$bin" del` / `sudo systemctl` name none: every script case still
runs under it (all paths are overridden by temp dirs), but say the match is a substring of any argument, or
the sh -c form slips through. Why systemd-run is exempt is unclear, as its `--pki` now carries `$PKI_ROOT/io-pki`.

No new security defect: the stub and the `[[ -d ]]` skip widen nothing; the shutdown-first order only makes the
refusal earlier, and the lock-held state check closes the TOCTOU of the old check (which read the state outside the lock).
