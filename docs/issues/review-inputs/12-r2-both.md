# Plan review: issue #12, round 2 (incremental), focus: design-code and design-security

Scope: `git diff 6f70e98..082c8f0 -- docs/issues/12/ docs/adr/ ':!docs/issues/12/report.md'`. I
checked it against `origin/main` (8aaa7a3), the base the plan now names, and against the approved
plans of #74 (`/tmp/flaminio-plans-2026-11-12/74/docs/issues/74/`) and #16
(`/home/jsfakian/Documents/src/flaminio-16`, approved 2026-11-13). Line numbers refer to the files
at 082c8f0.

## Earlier findings

- R1-S1: mostly fixed. Device deletion, `control::bind`, `DATAPLANE=unit`, the follow-up and the
  successor ADR now agree with #74. #74's PKI modes are not covered (R2-C1), and the evidence
  command is not adapted (R2-C3).
- R1-S2: fixed. The `AdmissionConfiguration` is loaded through `kube-apiserver-arg`, and the
  server checks it with a dry run in `default`.
- R1-C1: fixed. The guard runs before any `del`, and "`up` runs `down` first" is kept.
- R1-C2: fixed. Main's `running()`/exit 3 and the lab.sh callers are restated, and script tests
  are named.
- R1-C3: fixed.
- R1-C4: fixed (grammars, `env`, constant `sh -ec` scripts, `ATTACH`/`VOLUME`, ServiceAccount in
  `rbac.yaml`). One remainder in `node.sh attach`: R2-S1.
- R1-C5: the mount is fixed. The new `/etc/flaminio/io-pki` conflicts with #74 and #16 (R2-C1).
- R1-C6: fixed.
- R1-S3: fixed.
- R1-S4: fixed.
- R1-C7: fixed.
- R1-C8: fixed.
- R1-C9: fixed as asked (refusal after SIGTERM). The other window it named, between preStop's
  `ok none` and SIGTERM, remains (R2-B1).
- R1-C10: fixed.
- R1-C11: fixed, but the guard in `fio-raw.sh` breaks `make bench-pod` on pods (R2-C2).
- R1-C12: fixed.
- R1-C13: fixed.
- R1-C14: fixed. ADR 0012 is cited consistently in docs/issues/12 and docs/adr/README.md.
- R1-C15: fixed.
- R1-C16: fixed.
- R1-C17: fixed.
- R1-C18: fixed.
- R1-C19: fixed.
- R1-S5: fixed, except `node.sh attach` (R2-S1).
- R1-S6: fixed.
- R1-S7: fixed.
- R1-S8: fixed for the three readers it names. `control()` is not covered (R2-S1).
- R1-S9: fixed.
- Code N1, N2, N3 and the security Nit: fixed.

### R2-C1 — High
**The new `/etc/flaminio/io-pki` contradicts #74's and #16's approved PKI layouts. With #74,
`make node-up` cannot start.**

What #12 now says:
- design.md:109-119 makes `/etc/flaminio/io-pki` root-owned and 0700, with `io.key` 0600.
- "The unit's `node.sh up` passes `--pki /etc/flaminio/io-pki` too".

What #74 says:
- Under `PRIVSEP=1`, its default, `node.sh up` runs the I/O process as the system user
  `flaminio-io` (74/design.md:200-207, the lab node).
- That process reads `--pki` itself. #74 makes `/etc/flaminio/pki` 0750 and `io.key` 0640, group
  `flaminio-io`, for this reason. Its `pki.sh:10` change is 0750, and 74/design.md:369 says
  "#16's renewal keeps `io.key` 0640".

What happens:
- With #12's directory, the unprivileged child cannot traverse `io-pki`. The unit exits on
  `io.crt: Permission denied`.
- `DATAPLANE=unit` was kept for #74's evidence ("`make node-up` under the helper, then
  `make csi-deploy`, `make csi-test`"). That evidence cannot run.
- The new "#74, the privileged helper" section (design.md:18-37) has no PKI bullet.

What #16 says:
- Its approved plan keeps `io` in `/etc/flaminio/pki` ("the rest in `/etc/flaminio/pki`",
  16/design.md:87-89, Contracts "Paths", :141-143).
- Its `pki.sh install` renews by rename and then removes the old layout's files.

What happens: #16's renewal writes `/etc/flaminio/pki/io.*`, not `io-pki`. Both the pod and the
unit keep the first certificate until it expires (24 h). R1-C5's own rationale ("so that #16's
rotation by rename reaches the pod", decisions.md "PKI") then does not hold.

Fix: add a PKI bullet under "#74, the privileged helper", and one rule for #16:
- Once #74 has landed, `io-pki` is `root:flaminio-io` 0750 with `io.key` 0640. The pod reads it as
  root in either case, and S2 still holds.
- Whichever of #12 and #16 lands second puts `io` in `io-pki`, in #16's one-directory-per-name
  `pki.sh`, as #16 does for `meta-pki` and `agent-pki`. #16's "Paths" contract then names it.
- Add script-test cases: `pki.sh csr|install io` writes into `io-pki`; `file ca.crt|token.pub`
  writes to both directories; `node.sh up`'s unit arguments carry `--pki /etc/flaminio/io-pki`.

### R2-C2 — Medium
**The new guard in `fio-raw.sh` (R1-C11) breaks `make bench-pod` on the default deployment, and
the plan does not say so.**

- `cmd_bench_pod` (main lab.sh:1342-1380) runs the CSI driver's fio pod. Then, if the raw disk was
  preconditioned, it runs `fio-raw.sh` with read-only jobs on the same node while the store is
  live. Its comment: "The raw disk, read only. Never precondition it here".
- With `DATAPLANE=pod`, the default, the guard refuses on fl-a and fl-b (design.md:323-327; lab 8;
  S8 "refuse on a pod node, the raw-disk ones included").
- The pipeline `run_on_vm ... fio-raw.sh ... | awk ... >raw.tsv` then fails under `pipefail`.
  `make bench-pod` dies after its 30-minute pod run.
- intent.md:166 and spec S8 say the benchmarks "keep the unit, which needs `make csi-undeploy`
  first". `bench-pod` cannot run after `csi-undeploy`, because it needs the driver.

Fix (one of):
- Let `fio-raw.sh` skip the guard when every selected job is a read. The guard exists for writes:
  "write the data disk without `O_EXCL`".
- State that `bench-pod` needs `DATAPLANE=unit`. The guard passes with the unit active
  (design.md:311). Put this in the Makefile help, `docs/lab.md` and S8.

Either way, add `bench-pod` to the lab plan.

### R2-C3 — Low
**`DATAPLANE` is a per-command variable, and the commands that follow cannot tell the mode.**

- design.md:25-27 and :525-527 say #74's evidence ("`make node-up` under the helper, then
  `make csi-deploy`, `make csi-test`") "still runs".
- Once #12 has landed, a plain `make csi-deploy` is pod mode. Its step 2 runs `node.sh down` on the
  helper units and replaces them with direct-mode pods (design.md:512-515). #74's S8 evidence would
  then silently test pods, not the helper.
- `csi_failover` (design.md:529-530) chooses by "`DATAPLANE=unit`". `make csi-failover` run without
  it on a unit deployment waits for a data-plane pod that never comes.

Fix:
- Read the mode from the cluster (the pod-node label, or the DaemonSet's existence) in
  `csi_failover`.
- Say that #74's evidence becomes `DATAPLANE=unit make csi-deploy`. Whichever of #12 and #74 lands
  second updates #74's lab plan and `docs/lab.md`.

### R2-C4 — Low
**Nothing creates `/etc/flaminio/io-pki` on an existing lab.**

- Only `cmd_pki issue` writes `io` there (design.md:115-117). Yet `node.sh up` passes `--pki
  /etc/flaminio/io-pki`, and the pod mounts it as a `Directory` hostPath, which must exist.
- On a lab issued before this change:
  - `make node-up` fails on `io-pki/io.crt`;
  - the DaemonSet's pods stay `ContainerCreating`;
  - `csi-deploy` times out at step 5 (300 s).
- Main's `cmd_csi deploy` already issues a missing identity: `agent.crt`, lab.sh:998-1000.

Fix: `cmd_csi deploy` (and `cmd_node up`) issue `io` into `io-pki` when `io-pki/io.crt` is
missing, as for `agent.crt`. Otherwise `node.sh up` should die naming `make pki-issue`. Say which
in `docs/lab.md`.

### R2-B1 — Low
**An attach between preStop's `ok none` and kubelet's SIGTERM is still taken.**

- R1-C9 named two windows. The fix closes the one after SIGTERM (design.md:187-191).
- preStop exits on `ok none` (design.md:100, :386-395). Kubelet sends SIGTERM only after that.
  An attach can arrive in between, for example a NodeStage retry on a node that is not cordoned
  (`csi-undeploy`, a removed label).
- That attach defers the exit. The new volume is then SIGKILLed when the 3600 s grace runs out.
  S4 calls this a crash. The plan neither closes the window nor records it as accepted.

Fix (one of):
- preStop signals first: `kill -TERM 1; until flaminio-agent status --none; do sleep 2; done`.
  `flaminio-io` is PID 1, and `begin_attach` then refuses from the start of the hook.
- Record the window as an accepted risk in decisions.md.

Either way, add a lab or unit case for it.

### R2-S1 — Low
**`node.sh attach`, now reaching a pod's data plane, keeps main's unvalidated `VOLUME`. `control()`
stays unbounded.**

- The Ids rule (design.md:258-259) covers `up`'s `ATTACH` and the `VOLUME` of `detach` and
  `delete`. On main, `attach` does no `volume_id` check (main node.sh:76-81), and `VOLUME` goes
  into the socket line `attach $VOLUME $TOKEN`. AGENTS.md: "Ids reaching ... socket lines are
  validated".
- `detach`, `delete` and `attach` (changed gates) read answers through `control()`, which reads an
  unbounded line (main common.sh:24-29). Their `die "detach $VOLUME: $answer"` prints it raw to the
  operator's terminal.
- "Reading the answers" (design.md:224-233) applies the bound and the grammar only to the agent,
  `node.sh socket` and the CSI plugin's `Status`.

Fix:
- `attach` calls `volume_id`.
- `control()` reads at most 4096 printable bytes.
- `die` prints `malformed` instead of an answer outside the grammar.
- Add script cases for both.

### R2-C5 — Low
**Some new rules have no test, and spec Tests overclaims.**

Rules without a test:
- `node.sh down`'s refusal while V is attaching, before any stop (design.md:269-273). The
  `down` cases (design.md:625-628) cover only "attached" and the guard.
- `k3s.sh server`'s stop when the dry run is admitted (design.md:341-344). Lab 1 only shows it
  passing (design.md:667-668), so a check that never stops would pass every test.
- `pki.sh`'s routing into `io-pki`, `cmd_pki issue`'s removal of `/etc/flaminio/pki/io.*`, and
  `node.sh up`'s `--pki /etc/flaminio/io-pki`.

The overclaim: spec Tests (spec.md:161-162) says the script tests cover "each shell rule of S3, S4,
S7 and S8". The design names no script case for any k3s.sh rule of S7 (the agent token,
the 0600 modes, the restart on change, the dry run) or for `DATAPLANE=unit`'s refusal.

Fix:
- Add node-scripts-test cases: `ok attaching v` with the unit active means no stop and exit 1; the
  unit's arguments carry `--pki`.
- Add a k3s.sh case with a stub `k3s kubectl` that admits the dry run: `server` exits non-zero.
- Add the pki.sh cases of R2-C1.
- Narrow the spec's sentence, or add the S7 cases.

### R2-C6 — Low
**The script tests for `node.sh up` and the token files cannot run as specified.**

- Under `FLAMINIO_SCRIPT_TEST=1`, only the `flaminio-io` binary, the `/dev/ublkc*` directory and
  `CONTROL_SOCKET` can be overridden (design.md:327-330).
- main's `node-scripts-test.sh` stubs `sudo` as `exec "$@"`. So it runs as the test user.
- The named cases include:
  - `up` writes the token file 0600;
  - `detach` deletes `<VOLUME>.token`;
  - `down` deletes a planted `.token` (design.md:625-633).
- Those cases would therefore write the real `/run/flaminio` (`install -d -m 700 /run/flaminio`).
  That fails off root, in CI and on macOS.
- `up` also reaches `sudo modprobe`, `data_disk` (`lsblk`), `systemd-run` and `journalctl` before
  or around the token write.

Fix: name a token-directory override, and the `modprobe`, `systemd-run`, `journalctl` and `lsblk`
stubs (or a `DEVICE` override), in the test section.

### Nits
- **N1:** ADR 0012:96-98 says the follow-up "must keep the rule that a crash's leftover never
  blocks an attach: deleting a dead device that a volume still holds waits for it". This reads
  as if the wait keeps the rule. decisions.md (R1 entry) says the wait is what breaks it
  ("would block the pod's start, against S4"), and intent.md:147-148 says "reconciled with". Use
  intent's wording: "must be reconciled with the rule ..., since deleting a dead device a volume
  still holds waits for it".
- **N2:** intent.md:25 says "`node.sh up` and `ublk.sh up` delete nothing first". On main,
  `node.sh up` runs `down`, which `del`s `/dev/ublkc1`, when the unit is active (main
  node.sh:44, :28-29).
- **N3:** design.md:262 says `down` "deletes every `/run/flaminio/*.token`". Written as a shell
  glob, it expands as the VM user, who cannot list the root 0700 directory. Nothing is then
  deleted, and the stubbed-sudo script test (readable directory) cannot see it. Say it is expanded
  as root, for example `sudo find /run/flaminio -maxdepth 1 -name '*.token' -type f -delete`.
- **N4:** `socket: error`'s reason set includes `refused` (design.md:298-300) but does not say
  what produces it. An implementer can map `ECONNREFUSED` to it, while the line above says that
  is `absent`. Name the case, presumably an `error ...` answer.

Reviewed commit: 74979ae6a0235c8ab1d2df49e85e5d0dfb542d7b
