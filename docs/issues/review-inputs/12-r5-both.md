# Plan review: issue #12, round 5 (incremental), design-code and design-security

Diff 3008b06..1d200c9. No findings of Low or worse.

## Round-4 findings
- R4-L1 (lab 12 setup): fixed. `make node-down` (Makefile:241, `cmd_node down` over all data VMs) runs after
  `csi-undeploy`; the guard passes (no pod, unit state irrelevant); `bench-raw VM=b` then writes under no live
  store (fio-raw.sh:30-35 preconditions only without the marker); `FORMAT=1 make node-up VM=b` has `io-pki`
  (lab 13 removes it later); `make csi-deploy` then finds no unit. The "runs after labs that use earlier volumes"
  caveat is stated.
- R4-L2 (down override case): fixed. With `RUN_DIR` honoured, `[[ -d tmp ]]` is true and `sudo find tmp -delete`
  would remove the planted token, so the case can fail; the stub refusal stays the check for `pki.sh` and `k3s.sh`.
- R4-L3 (check order): fixed. io.crt and the pod guard come before `systemctl is-active && down`
  (node.sh:41 today); the case asserts the unit stays active (no stop, no `del`). A refusal leaves a running unit
  untouched.
- R4-M1 (refusal text): fixed. New text `already attaching V` / `already attached to V`; the added unit case
  (attached, non-base64 token) pins state-before-token, which matches "checks come first, before the token's
  base64" (design.md:233). Callers: the only readers of control answers are `dataplane.go:119` (matches
  "is not attached here", detach only) and the usage-line fallback (`dataplane.go:110`); `node.go:68` works from
  `status`. Nothing in scripts, Go tests, or docs matches the old `is attached here; one volume at a time`
  (only `control.rs:95`, which the plan replaces).
- R4-N1 (sudo stub): fixed. Substring match over every argument catches `sudo sh -c` and `sudo env`. Every named
  case still runs: `systemd-run`'s arguments (`--pki $PKI_ROOT/io-pki`, `--token $RUN_DIR/...`, `--device` from the
  stubbed `lsblk`) name only temporary paths; `sudo "$bin" del`, `systemctl`, `journalctl`, `tee`, `install`, `test`
  take overridden paths.

## What the revision opens
Nothing material. One thing to watch when #74 lands: its `node.sh up` must not put a real path (for example a
`--property=ReadWritePaths=/run/flaminio` or `RuntimeDirectory=`) in the `systemd-run` arguments, since the stub
no longer exempts it; the #74 case would then fail loudly, not silently, so it is self-policing.
