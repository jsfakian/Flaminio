# Plan review: issue #12, round 3 (incremental), focus: design-code and design-security

Scope: `git diff 74979ae..cbe3d62 -- docs/issues/12/ docs/adr/ ':!docs/issues/12/report.md'`
(HEAD 0165a65 changes only report.md). Checked against `origin/main` (8aaa7a3), #74's plan
(`/tmp/flaminio-plans-2026-11-12/74`) and #16's (`/home/jsfakian/Documents/src/flaminio-16`).
Line numbers are design.md at HEAD unless a file is named.

## Earlier findings
- R2-C1: fixed (PKI bullet under #74, the #16 rule, script cases). The new pki.sh case expects
  modes pki.sh does not set: R3-C2.
- R2-C2: fixed. The read-only skip is sound: fio opens a block device `O_RDONLY` for read jobs,
  and the skip needs both conditions (every selected job's `rw` is a read, and the marker exists).
- R2-C3: fixed.
- R2-C4: fixed for lab.sh. Two direct callers of `node.sh up` are missed: R3-C4.
- R2-B1: fixed. **The `kill -TERM 1` claim holds.** In kernel/signal.c, `sig_ignored()` returns
  false for a signal in the target's `blocked` or `real_blocked` set before
  `sig_task_ignored()`'s `SIGNAL_UNKILLABLE`/`SIG_DFL` drop is reached. For `kill(2)` the target
  is the group leader, here the main thread that runs `install_sigterm()` (main.rs starts no
  thread before `node::main`). `do_sigtimedwait` dequeues it without `get_signal()`'s
  unkillable check. `force`, the only difference for a sender outside the namespace, matters
  only for SIGKILL/SIGSTOP, so the hook's signal and kubelet's take the same path. io-wq
  workers block every signal. Main has no signal code yet (no match for
  sigterm/sigwait/sigmask in `dataplane/crates`), so all of this rests on the designed
  `install_sigterm()`. The new lab probe depends on an unstated order: R3-C3.
- R2-S1: fixed.
- R2-C5: fixed.
- R2-C6: fixed.
- R2-N1, R2-N2, R2-N4: fixed. R2-N3: fixed, but the command as written fails on a missing
  directory: R3-C1.

### R3-C1 — Medium
**`down`'s `sudo find /run/flaminio ...` fails when `/run/flaminio` does not exist, and
`set -euo pipefail` then ends `down`.**
- Evidence: design.md:294-297 and :309 (step 4 always deletes the token files once the guard
  passes). `/run` is tmpfs. On main only the unit's `up` with `ATTACH`, the data plane and the CSI
  plugin's `DirectoryOrCreate` hostPath create the directory (driver.yaml:167, node.sh:61).
  GNU find exits 1 on a missing starting point.
- When it happens:
  - after any reboot of a node without the CSI plugin (fl-c, the demo), or before kubelet
    restarts the plugin, `make node-down` with the unit inactive fails. That is the crash
    cleanup step 4 exists for.
  - Once #74 has landed, "`node.sh up` runs `down` first, always" is kept (design.md:38). Then
    `make data-node-up` fails on its first run: `cmd_data_node_up` creates fl-c fresh (main
    lab.sh:1222-1236). That is a step of #74's evidence. `ensure_node_c` fails the same way
    after a reboot of fl-c.
- The script case plants a token in an existing `RUN_DIR`, so it cannot see this.
- Fix: skip the deletion when the directory is absent (`[[ ! -d $RUN_DIR ]] || sudo find ...`;
  `/run` is world-readable). Add a `down` case with `RUN_DIR` missing: exit 0, `del` still runs.

### R3-C2 — Low
**The new pki.sh script case expects `io.key` 0640 and group `flaminio-io` from pki.sh. Under the
design's own split, pki.sh does not set them.**
- design.md:31-34: "#74's `node.sh up` and `pki.sh:10` apply their modes". That is #74's split:
  `node.sh up` sets `io.key` 0640 and the group, and `pki.sh:10` sets only the directory to 0750
  (74/design.md:206, :216).
- `pki.sh csr` makes the key under `umask 077` (main pki.sh:15-16), so after `csr io` alone,
  `io.key` is 0600 and `io-pki` keeps its group.
- The case at design.md:700-703 says "0640 and 0750, group `flaminio-io`, once #74 has landed".
  It would fail, or push the `chgrp` into pki.sh, where the group may not exist yet. Nor can a
  CI runner, which runs as the test user under the `exec "$@"` sudo stub, assert a
  `flaminio-io` group.
- Fix: the pki.sh case checks modes only: `io.key` 0600, and `io-pki` 0700, or 0750 once #74
  has landed. Put the 0640 and the group in a `node.sh up` case that checks the `chgrp`/`chmod`
  the sudo stub records.

### R3-C3 — Low
**Lab 6's new probe expects "the node is shutting down" from an attach while attached. Main's
`serve` answers that case before any attach helper runs, and no unit case pins the order.**
- On main, `serve` checks the attached state itself and answers
  `error: <v> is attached here; one volume at a time` (control.rs:85-96). Only after that does
  it call `attach()`.
- The design has `begin_attach` "build every attach request: for `serve`", with the SIGTERM
  check first (design.md:213-219). It does not say that `serve`'s own check goes.
- If `serve`'s check stays, lab 6 (design.md:810-814) answers "vol1 is attached here" even when
  the hook's signal arrived.
- The unit case "after SIGTERM" (design.md:662-664) runs the attach after `detach v`, with the
  state `None`, so it cannot tell the order either.
- Fix: say that `serve`'s check is replaced by `begin_attach`'s step 2, after the SIGTERM check.
  Add a `control.rs` case: attached to `v`, flag set, `attach w` answers
  `the node is shutting down`.

### R3-C4 — Low
**"Every path that runs `node.sh up`" (design.md:142-147) misses two callers, and the tests
cover two of the four paths it names.**
- `scripts/checks/reconcile.sh:38` and `scripts/checks/pvc-loop.sh:21` run `node.sh up`
  directly, not through `cmd_node`.
- On a lab issued before this change, they fail with the unit's journal tail
  (`/etc/flaminio/io-pki/io.crt: No such file or directory`), not a pointer to the fix.
- lab-k8s-test (design.md:731-733) and lab 13 cover `cmd_csi deploy` and `cmd_node up`, but not
  `cmd_data_node_up` or `ensure_node_c`.
- Fix (one of):
  - `node.sh up` dies naming `make pki-issue` when `sudo test -f /etc/flaminio/io-pki/io.crt`
    fails. That covers every caller, with one script case.
  - Name the two checks in the rule.

  Either way, add `ensure_node_c` to the lab-k8s-test case.

### R3-B1 — Low
**Some of the revision's rules have no negative or abuse case.**
- **S2 and `file`'s routing:** only the positive case is named: `file ca.crt` and
  `file token.pub` write both directories (design.md:702).
  - Nothing checks that `file token.key` and `file etcd-ca.crt` stay out of `io-pki`.
  - An implementation that writes every `file` to both directories would pass. It would put
    the token signing key in the pod's mount, against S2's "no other key" (spec.md:36).
  - Add the case: after `cmd_pki issue`'s sequence, `io-pki` holds exactly the four files.
- **The new overrides' gating:** "honoured only under `FLAMINIO_SCRIPT_TEST=1`" (design.md:367)
  has no case for `RUN_DIR`, `PKI_ROOT` or `K3S_CONFIG_DIR`.
  - Main tests the same rule for `CONTROL_SOCKET` (node-scripts-test.sh:238-246).
  - `RUN_DIR` feeds a root `find -delete`.
  - Outside the tests only the operator's `ENV_VARS` can set them (run_on_vm forwards nothing
    else; the pod's scripts do not source common.sh), so this is a test gap, not a reachable
    bypass.
  - Add an "overrides without the switch" case per variable: nothing is written or deleted
    in the temporary directory.
- **R2-N4's `refused`** (design.md:333-335) is not pinned. The case "`error` on an `error x`
  answer" (design.md:677) passes with any reason. The case it is set against, a stale socket
  file (`ECONNREFUSED` gives `absent`, the crash path the guard relies on), has none.
  - Add `socket: error refused` on `error x`, and `socket: absent` on a socket file with no
    listener.

### Nits
- **N1:** design.md:133-134: "`pki.sh install io` then removes the old `/etc/flaminio/pki/io.*`".
  As a glob of the calling shell, this expands as the VM user, who cannot list the root 0700
  directory, so nothing is removed. The case's readable `PKI_ROOT` cannot tell, as R2-N3 found
  for the tokens. Name the two files (`sudo rm -f .../io.crt .../io.key`).
- **N2:** design.md:33 has no "whichever of #12 and #74 lands second" clause, unlike the
  bullets around it. If #74 lands second and follows 74/design.md:206 as written, its
  `node.sh up` would `chgrp` the removed `/etc/flaminio/pki/io.key` and die. Say that the second
  to land changes #74's lab-node line and `pki.sh:10` to `io-pki`.
- **N3:** with nothing attached, or after the detach, the hook's signal ends PID 1. The hook's
  own `sh` is then killed with the namespace, so kubelet records a `FailedPreStopHook` warning on
  every such deletion. Say so beside the preStop (design.md:108-110) and in lab 6, so the warning
  is not read as a failure.
- **N4:** main's `cmd_bench_pod` warns "make bench-raw first, before node-up" (lab.sh:1375). With
  pods, `make bench-raw` without the marker refuses on a pod node, and preconditioning destroys
  the store. Lab 12's "preconditioned earlier with `make bench-raw`" (design.md:854-855) needs
  the same precision. Name the step: before the first `make csi-deploy`, or after
  `make csi-undeploy`, on a disk whose store may be lost.

Reviewed commit: 0165a65b13a53662c9265c6d4af46e4eca47d31f
