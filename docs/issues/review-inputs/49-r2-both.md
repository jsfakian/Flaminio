# Plan review: issue #49 (Chaos test suite), round 2, design-code and design-security

Scope: `git diff dde1d79..4a33651 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`, checked against `scripts/lab.sh`, `scripts/lib.sh`,
`scripts/guest/{chaos,node,partition,write-load}.sh`, `Makefile`, `.github/workflows/ci.yml`,
#9's `node.sh attached` (docs/issues/9/design.md:170-216), and `origin/main` #82, #83, #84.

Counts: Medium 2, Low 4, Nit 3.

## Earlier findings

- R1-C1: fixed (S2, S7, contracts "Opening reads", design.md:42-48, tests in both tables).
- R1-C2: fixed as a mechanism (SIGUSR1, back-to-back bursts, `kill-ready`, mark, SIGKILL on one VM); its delivery and its evidence are R2-C1 and R2-C2.
- R1-B1: fixed (heal ledger in contracts.md:76-98, workstation copy only, RUN_DIR validated).
- R1-S1: fixed (S9 bullet, design.md:169-174; `node.sh attached` matches #9's `volume ID` form; node.sh fixes `/dev/ublkb1`, node.sh:13-14).
- R1-B2: fixed (no block trace).
- R1-C3: fixed (segment id in the high 32 bits of op and value ids; checker rejects others).
- R1-C4: fixed (`end` under `flock`, torn-tail truncation, `seal`).
- R1-C5: fixed (kinds defined, `kind` in the JSON, fault marks on the target's clock); see Nits.
- R1-C6: fixed (kill mark in End; `status` read twice).
- R1-C7: fixed (`scripts/dev/history-lies.sh` under `make contracts`, tag `historylies`).
- R1-C8: fixed (design.md:253-254, decisions; matches #84's settlement).
- R1-C9: fixed (library module, hand-written CRC32C, SHA-256 replaced by sealed length).
- R1-C10: fixed (`control/cmd/flaminio-history`, `pkg/blockhistory`, `go.mod`, `history_build`).
- R1-C11: fixed.
- R1-C12: fixed (artifact cap only); its enforcement across segments is R2-C4.
- R1-C13: fixed.
- R1-C14: fixed.
- R1-C15: fixed (files named); the new script test is not wired into `make lint`, R2-C5.
- R1-C16: fixed (intent Out of scope lists #76-#84, #82's and #83's lab conditions).
- R1-S2: fixed (S14 sentence, design.md:221-224, token grep test).
- Nit 1 (Q1-Q5 "remain proposed"): fixed. Nit 2 (clock name): fixed. Nit 3 (frame bound): fixed.

### R2-C1 — Medium
**The signals go to the client's unit, but under `run_detached` the unit's main process is a
bash wrapper: SIGUSR1 stops the client instead of starting the kill phase, and a SIGKILLed
unit hangs the harness's poll.** The harness starts the client through `run_detached` with a
unit name (design.md:214-215). That unit's main process is `bash -c '…; env … bash
~/.flaminio-lab/$unit.sh …; echo $? > …rc'` (`scripts/lib.sh:171-173`); the client runs below
it (with `sudo`, as `write-load.sh:9` does, for the device). The kill helper "sends SIGUSR1 to
that unit" (design.md:199). `systemctl kill` signals every process of the unit by default;
bash's default action for SIGUSR1 is to terminate, so the main process exits, systemd stops the
unit and sends SIGTERM to what is left, and the client takes SIGTERM as "stop issuing, drain and
exit" (design.md:74-75). `kill-ready` never appears and every kill round fails after 10 s.
Separately, at a kill the harness SIGKILLs the client's unit (design.md:73-74): the wrapper
never writes `.rc`, and `run_detached` polls for it with no other exit (`lib.sh:177-194`), so a
backgrounded `run_detached` for the client spins forever. The script test fakes the VMs and
checks only the helper's order (design.md:233), so neither defect would show before the lab.
**Fix:** the client writes its PID to the segment directory after it takes the `flock`; the
helper signals that PID alone, after checking that it is a live `flaminio-history` process that
holds that file's lock; say how the harness stops waiting on a client unit that ends without
`.rc` (inactive unit counts as ended). Script test: the fake unit's wrapper survives the
helper's SIGUSR1 and only the client's PID is signalled.

### R2-C2 — Medium
**`in_flight_at_kill` can be one when nothing was in flight at the SIGKILL, and the kill phase
has gaps the design calls an instant.** It counts "the operations invoked before the kill mark
and not answered before it" (design.md:115-116). The mark is taken before the kill: the helper
runs `flaminio-history clock`, writes `kill-mark`, then sends SIGKILL through `systemctl`
(design.md:200-201), milliseconds to tens of milliseconds, while a 4 KiB burst completes in a
few. An operation answered with success in that window is counted, so `in_flight_at_kill >= 1`
does not show that the kill cut an operation; the spec's evidence is "the unanswered writes at
the kill" (spec.md:157-158). The design also says an operation is outstanding "at every moment
but the instant between a checkpoint's answer and the next issue" (design.md:57-58), but every
issue waits for its batch's Invoke fdatasync (design.md:49), and the checkpoint read follows the
drain (design.md:47-48): each burst has two fdatasync windows with nothing outstanding, and a
kill in one fails the run (decisions.md:114-115), more often than "an instant" suggests.
**Fix:** define `in_flight_at_kill` as operations invoked before the mark with no successful
Return (none, or an error answered after the mark); state the two fdatasync windows per burst
and that a kill landing in one fails the run; a `sim/history.rs` case where the only operation
outstanding at the mark is answered with success after it gives zero.

### R2-C3 — Low
**`--node` accepts only `^fl-[a-z]+$`, the lab's default names.** design.md:128. VM names are
`lab.conf` defaults that an operator may override (`scripts/lab.conf:31,40,51,61`), and S17
says the tool names no lab node and #56's check runs it as it is (spec.md:126-128). The
contract bounds the field at 128 UTF-8 bytes (contracts.md:11).
**Fix:** validate `--node` as a bounded id (the `valid_id` grammar, at most 128 bytes), and add a
non-`fl-` name to `main_test.go`'s accepted cases.

### R2-C4 — Low
**The 128 MiB per-run cap is now the only bound on the workload, but no client can enforce it.**
design.md:66-68 says the client stops issuing before a record that would pass the per-run total;
each segment's client sees only its own file (design.md:34), and the CLI takes no budget
(design.md:126-128). The checker is not said to refuse a run over the cap either.
**Fix:** the harness passes each client `--max-bytes` (128 MiB less the sealed lengths so far and
the other kept files), and the checker refuses, as undecided, a manifest whose sealed lengths
exceed 128 MiB; a test for each.

### R2-C5 — Low
**`chaos-history-test.sh` is not added to `make lint`.** The S9-S15 tests are its cases
(design.md:233-235; spec.md:149 "script tests, `make lint`"). `make lint` runs script tests by
explicit name (`Makefile:92-94`), and the Touched list names only the `contracts` change to the
Makefile (design.md:243). As written, the new test never runs in CI.
**Fix:** add "`lint` runs `scripts/tools/chaos-history-test.sh`" to the Touched list.

### R2-S1 — Low
**`chaos.sh slow on` still deletes whatever root qdisc wg0 has.** The revision makes the delay's
netem the root qdisc with handle `49:` and narrows `slow off` (design.md:185-187,
contracts.md:90), but `slow on` begins with `sudo tc qdisc del dev wg0 root` unconditionally
(`scripts/guest/chaos.sh:7`), against "never delete unrelated qdiscs" (design.md:216).
**Fix:** `slow on` adds the `49:` netem only when wg0's root is the default or already that netem,
and otherwise refuses (the fault is not confirmed, so the run fails); a case in the script test.

## Nits

- design.md:142-144: the kinds' bullets overlap (W0 unanswered and W1's value read again after R
  match both "resurfaced" and "stale read"). Say that the first matching bullet wins, so the kind
  is deterministic (S8).
- design.md:146-147: "a value whose write was invoked after the read was answered" is called S6's
  foreign content, but S6 (spec.md:55-56) covers only values not written to that block in this
  run. Say it is reported as foreign content, as an addition to S6.
- contracts.md:84: ledger lines carry no round or sequence, so repeated `restart fl-b` actions
  are matched to their `healed` lines only by order. Say so: an `intend` is healed by the next
  `healed` line of the same kind and VM.

Reviewed commit: b026fe8fb305432f0061169198d938d7d75dde1d
