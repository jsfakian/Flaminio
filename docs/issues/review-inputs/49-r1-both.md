# Plan review: issue #49 (Chaos test suite), round 1, design-code and design-security

Scope: `docs/issues/49/` (intent, spec, design, contracts, decisions) in
`/home/jsfakian/Documents/src/flaminio-49`, against the scripts and code as on `origin/main`
(the checkout's scripts are identical to `origin/main`; only lockfiles and `go.mod` differ).
The intent's and the design's facts about today's code hold (`scripts/lab.sh:1386-1474`,
`scripts/guest/chaos.sh`, `write-load.sh`, `sim/check.rs:29-58`, `.github/workflows/lab.yml`).
The defects are in the mechanisms the design leaves open.

Counts: High 2, Medium 9, Low 9, Nit 3.

### R1-C1 — High
**A later segment's first operation is not specified, so a write lost at the failover can pass.**
Spec S2 (`spec.md:33`) says "each block's first operation is a write, acknowledged before
anything else on it"; the design repeats it as what the per-segment command does ("Initialize
each with an acknowledged unique write before any read/overlap", `design.md:33`; the CLI is
`flaminio-history run … --segment`, one invocation per segment, `design.md:80`). Neither says
this holds for the run's first segment only. If every segment initializes, the new target's
first operation overwrites each block before anything reads it: an acknowledged write the
failover dropped is never observed. Even if only segment 1 initializes, a later segment starts
with a burst of two writes and two reads issued together (`design.md:33-34`): a read is allowed
to return either new value, so the loss is seen only when a read happens to be served first.
The only mandatory read is the run's final one (S9, `spec.md:73-74`; S7, `spec.md:58`), which
covers the last failover's state after more writes. This is the gap the intent exists to close
(Why 4, reconciliation after a target crash). Protocol §9's deadline rule
(`docs/replication-protocol.md:352-353`, `sim/mod.rs:1385-1392`) is unaffected: a read's answer
closes the old writes' window as a write's does.
**Fix:** a rule in S2/S4 and the design: only the run's first segment initializes; every later
segment first reads every block, each read answered before that segment's first write to the
block; S7 makes a later segment without those reads undecided; contracts say how the checker
recognizes them. Add the hand-written history (acknowledged write in segment k, older value read
first in segment k+1: violation) and the S16 lie across two segments.

### R1-C2 — High
**Nothing makes the kill land while history operations are in flight; with the stated pacing it
almost never will.** The workload issues one burst of 16 operations per second, drains, then
reads a checkpoint (`design.md:33-35`; 20 operations/s is also what keeps a 20-30 min run under
the 100,000-operation cap, `design.md:44`). A burst is in flight for milliseconds. The kill is
`run_on_vm … chaos.sh kill-node` from the workstation over SSH (`scripts/lab.sh:1455`), with no
synchronization to the workload. The design then requires "at least one history operation
outstanding throughout the measured kill-call interval" and fails the run otherwise
(`design.md:104-107`). So S9's "the kill comes under load" (`spec.md:71-72`) holds by chance,
and `make chaos` fails that requirement in most rounds, or the requirement gets weakened in
implementation.
**Fix:** specify the mechanism: for example the guest-side kill helper, on the target's VM,
waits for the recorder's signal that an Invoke batch is durable and issued, then sends the
signal at once; or the client runs back-to-back bursts from "ready to kill" until it dies,
within a stated operation budget. State which, and a test with the controlled blocking backend
that the kill helper fires only with operations outstanding.

### R1-B1 — Medium
**Two different things are called the run manifest, and `chaos-heal`'s input has no format.**
The design persists "an intended action/owned resource in run manifest" before each fault
(`design.md:97`) and `chaos-heal RUN_DIR` "validates manifest paths/resources" and uses a
"retained remote run manifest" after a lost connection (`design.md:112-116`). The only manifest
the contracts define is the binary `FLRUN001` (`contracts.md:48-55`): nonce, segment
descriptors, hashes; it has no place for a fault, a unit or a rule, and the contracts call the
fault log "separate contextual". So the file that decides which units `chaos-heal` stops, which
firewall and qdisc rules it removes and which services it starts is unspecified: its format,
where it lives (which VM and path holds the "remote" copy, who writes it, which copy wins), and
how each field is validated before it reaches a remote command (S15).
**Fix:** name it apart from `FLRUN001` (a heal ledger), give its format in `contracts.md`, and
state the validation: VM names through `vms_of`, unit names matched whole against the run's own
pattern, rule tags and qdisc handles from a fixed set with the run nonce, nothing else accepted.
Say where the remote copy is, or drop "remote" and heal from the fixed set. Script test: a
ledger with a foreign unit name, VM or path is refused before any remote command.

### R1-S1 — Medium
**No rule says which volume the run may overwrite, and nothing checks that the device is that
volume.** The workload writes block 0 and three more blocks of the device it is given, validated
only as `/dev/ublkb[0-9]+` (`design.md:21-22`); the harness keeps today's flow, which attaches
`$VOL` (`scripts/lab.conf:108`, any id the caller sets) and writes `/dev/ublkb1`
(`scripts/lab.sh:1431,1435`). `VOL=pvc-… make chaos` detaches that volume everywhere
(`lab.sh:711-714`, before the attach at `:715`), then overwrites its first block (a filesystem's superblock) and 4 GiB.
The clear plans guard their own writers: #9 S7 refuses any `VOL` not named `bench-…` before it
detaches, attaches or writes; #84's check creates its own `bc-…` volume. S15 covers quoting,
not what may be destroyed; S17 makes the tool reusable on any ublk device.
**Fix:** a spec rule and test: before anything else `make chaos` refuses a `VOL` that is not a
valid id or is a `pvc-…` or `bench-…` name (or accept only a stated set), and before each
segment's client and each fio job starts, the harness confirms from the node's control socket
(#9's `node.sh attached`) that the device serves `$VOL`; otherwise it stops, writing nothing.

### R1-B2 — Medium
**The tracefs capture traces to no spec rule, is not cleaned up, and its details are open.**
`design.md:105-108` adds a block-trace capture on the target's VM (root, tracefs) as the proof
of in-flight I/O. The spec asks only that operations be in flight (S9) and names the evidence:
the unanswered writes at the kill (`spec.md:142-143`), which the history already shows as
Invokes with an error Return or none. The capture is a thing the run starts, yet `chaos-heal`
lists only clients, fio, firewall and netem rules and services (`design.md:112-114`), and S13
(`spec.md:93-97`) does not name it: an interrupted run leaves a trace instance enabled. #84
records that a left instance makes its next run refuse
(`origin/main:docs/issues/84/design.md:233-235`); the instance name, events and buffer size
here are unspecified, so a clash with #84's cannot be ruled out.
**Fix:** drop the capture and take the evidence from the history (an operation invoked before
the kill helper's start mark and not answered with success), or keep it with a spec rule: the
instance's name (not #84's), events, buffer bound, its removal in S13 and `chaos-heal`, and a
script test of an interrupt while it is enabled.

### R1-C3 — Medium
**Values are not unique across segments as designed.** A value is the run nonce, the block index
and a "monotone unique write ID", then bytes derived from them (`design.md:25-27`). Each segment
has its own client process on another node (`design.md:21`); nothing says how segment k+1's
client continues the write IDs, and it cannot read segment k's file. Two clients that both
start at 1 produce byte-identical values for the same block, which breaks S1 and lets the
checker match a read to the wrong segment's write. The same holds for `op_id`, "unique within
run" (`contracts.md:27`) with no allocation rule. The S1 test runs one process on one file.
**Fix:** put the segment id in the id (for example the high 32 bits of `write_value_id` and
`op_id`), say so in `contracts.md`, have the checker reject an id whose segment bits differ
from its file's header, and test two segments' clients on one file.

### R1-C4 — Medium
**Who writes End, and how a segment is sealed, is unspecified and unsafe as sketched.**
- The End record is a CRC-framed binary record with the next gapless sequence number
  (`contracts.md:17-34`), "captured by the same-VM supervisor" (`design.md:48-49`,
  `contracts.md:41-42`). The only commands named are `flaminio-history run` and
  `check_history --manifest` (`design.md:80-81`): nothing writes End, builds `FLRUN001` or
  computes `sealed_segment_sha256`.
- End may be written once the target is confirmed dead while the client still lives
  (`design.md:48`), and late Returns are "retained diagnostically" (`design.md:51`). That is
  two writers on one file with one gapless sequence, and records appended after the seal change
  the sealed hash (`contracts.md:49,52`).
- A killed client may leave an incomplete trailing frame (`contracts.md:43-44`). An End
  appended after it is misaligned: the decoder reads the torn bytes and End as one frame, the
  CRC fails, and the segment is undecided. Every segment whose client is killed can end so.
**Fix:** one rule: the client is stopped first where it can be; a named subcommand (for example
`flaminio-history end`) truncates the file to the last whole frame, appends End and fsyncs;
late Returns go to a side file outside the sealed hash, or are dropped; a named step seals and
writes `FLRUN001`. Tests: a torn tail followed by End decodes; End while the client still
appends.

### R1-C5 — Medium
**S8's verdict content has no approach: the kind is undefined, and the faults have no data path.**
- S8 requires the violation's kind and S16 requires each lie to be named as its kind
  (`spec.md:61-63,107-113`). The design says the checker reports "observed patterns" and that
  a lost write and a stale read "can be observationally indistinguishable" (`design.md:83-85`),
  and gives no decision rule; `check_register` returns only the unplaced operations
  (`sim/check.rs:46-57`). The verdict JSON lists no kind field (`design.md:81-82`). The S16 test
  cannot assert a name that has no definition.
- S8 also requires "the faults in effect around them". The checker reads only the manifest;
  fault times are on other clocks, and the history holds only each node's boot-time
  nanoseconds, with no anchor (`contracts.md:29`). Nothing says how a fault is related to an
  operation for the report.
**Fix:** define the four kinds as properties of the history (for example: stale = an older value
read after a newer write's answer, with a newer value read later; lost = the same, never read
again through the final read; resurfaced = an optional write's value read after its deadline;
foreign = the flag), add `kind` to the JSON, and say which component adds the faults: the
harness, from marks it samples on the target's clock at each confirmed inject and heal.

### R1-C6 — Medium
**S10's two new failures have no mechanism.**
- "An error while its segment's target was alive" (`spec.md:80-81`): after the kill, errors on
  in-flight operations are expected, so the rule needs the kill's start on the target's own
  clock. The design says only that S10 "independently fails live-target errors"
  (`design.md:71`); the record has no kill mark (`contracts.md:22`), and S11 says no verdict
  depends on fault times (`spec.md:88`).
- "No operation answered between the last fault's healing and the kill" (`spec.md:82`): the
  heal is confirmed on the replica's node or fl-w, the answers are on the target's clock, and
  the invariant forbids comparing them (`spec.md:22`). "Workload progress since healing"
  (`design.md:104`) does not say how.
**Fix:** state both: the kill helper records its start on the target VM's boot-time clock
before it signals, and an error Return earlier than that fails the run; after a heal is
confirmed, the harness samples the target's answered-operation count twice through the
harness's own ordering and requires an increase. Name the script tests.

### R1-C7 — Medium
**S16 "in CI" has no job that can run it.** The design places the Go fake device with the Rust
checker "in make test" (`design.md:128`). CI does not run `make test`: Rust tests are
`cd dataplane && cargo test --locked` and Go tests are `make race`, in separate jobs
(`.github/workflows/ci.yml:40,50`; `Makefile:63-65,121-122`). A Go test that needs the Rust
`check_history` binary would run under `make race` without it, and must then skip or fail.
The one cross-language job is `contracts`, through `scripts/dev/meta-interop.sh`
(`Makefile:124-128`).
**Fix:** say where it runs: a script beside `meta-interop.sh` under `make contracts`, or Go
tests that write histories to a fixture directory which `cargo test` checks (with a
`contracts`-style regeneration diff). Either way the test fails, not skips, when the other half
is absent, and `docs/ci.md` names the job (S19).

### R1-C8 — Medium
**The lab plan does not say how it runs once #83 lands.** #83 (clear) refuses `flaminio-io node`
on virtual hosts with no bypass and calls the lab VMs "deliberately unqualified"
(`origin/main:docs/issues/83/design.md:32-42`); its Lab section asks a future #49 campaign for
"a qualified disposable host/profile" (`:172-176`). #84 settled the same clash for its own
check: on the lab VMs before #83 lands, then only on a host #83 admits
(`origin/main:docs/issues/84/design.md:224-227`). This design plans four rounds on the lab VMs
"later" and says only that #83's clock is "not certified by this slice" (`design.md:137-142`).
**Fix:** state the order as #84 does (before #83 lands on the lab VMs, afterwards only on a host
#83 admits, no bypass), in the design's Lab section and `decisions.md`.

### R1-C9 — Low
**"std only" and "no new dependency" do not fit the contract, and tests in an example do not
run.** The checker must verify CRC32C on the header and every frame and SHA-256 of each sealed
segment (`contracts.md:9,17,49,55`); `std` has neither, and `flaminio-repl` has no dependencies
(`dataplane/crates/flaminio-repl/Cargo.toml`). The design also leaves "example/bin" open
(`design.md:61`); `cargo test` builds examples but does not run their tests.
**Fix:** say which: hand-written CRC32C and SHA-256 with test vectors, or dev-dependencies
(`crc32c` is in the workspace, a SHA-256 crate is not). Put the decoder and slicer in a library
module with its tests, and make the example a thin `main`.

### R1-C10 — Low
**The Go tool's place, lint coverage and deployment are not the repository's.** `cmd/` and
`internal/` do not exist at the root (Go lives in `meta/`, `control/`, `pkg/`, `api/`), and
`make lint` runs gofmt only on `api meta pkg control` (`Makefile:88`), so the new code would
not be checked. `golang.org/x/sys` is an indirect requirement today (`go.mod:63`), not an
"existing" direct one: `go mod tidy -diff` in `make hygiene` (`Makefile:119`) needs the change.
The tool runs in the VMs, and the design does not say how it gets there; `meta_build`
(`scripts/lab.sh:501-505`, cross-compile and copy) is the idiom.
**Fix:** place it under an existing root (for example `control/cmd/flaminio-history`,
`pkg/blockhistory`) or extend the gofmt list; list `go.mod`; name the build-and-copy step.

### R1-C11 — Low
**Header fields without a source or a use.** The header carries `volume_epoch` and `node`
(`contracts.md:10-11`), "identities must match the run manifest" (`:12-13`), but `FLRUN001`
holds neither, the CLI takes neither (`design.md:80`), and no rule reads the epoch. The CLI's
`--run-id` is presumably the 32-byte `run_nonce`; the design does not say.
**Fix:** add the flags and say what checks each field, or drop `volume_epoch`; name the nonce's
encoding on the command line.

### R1-C12 — Low
**The 100,000-operation cap fails long runs for no finding.** At 20 operations/s the cap
(`design.md:44`) is reached after about 83 minutes. `ROUNDS` is free (`Makefile:279`), and each
round may wait up to 900 s twice for full ISRs (`scripts/lab.sh:788-794`). Slicing at
checkpoints makes the per-run cap unnecessary for the search.
**Fix:** bound `ROUNDS` (refused above the bound, with a test), or pace from the remaining
budget, or replace the per-run cap with the artifact-size cap alone.

### R1-C13 — Low
**A run can pass with the faults never touching the checked chunks.** Faults hit the node the
harness calls the replica; the history blocks sit in chunks 0, 32, 48 and 63
(`design.md:54-56`). Nothing checks that those chunks' replicas are the target and that node:
with fl-c registered when the volume was created, or after a failed `make drain-test`
(`scripts/lab.sh:1264-1305`), some are on fl-c.
**Fix:** at each round's start the harness fails unless every history block's chunk has the
target and the faulted node as its replicas (`chunks_on`, `lab.sh:1256-1258`, is the idiom), or
it picks the blocks from such chunks.

### R1-C14 — Low
**"Lacks a segment" (S7) is not decidable from the contract.** `FLRUN001` lists whatever
descriptors the harness wrote (`contracts.md:48-49`); segment ids need not be contiguous and
no expected count is recorded, so the checker cannot see that a round's segment is absent.
**Fix:** ids 1..n contiguous in the contract; the harness asserts n = `ROUNDS` + 1, plus one
with the move back (`lab.sh:1470`); a script test with a segment left out.

### R1-C15 — Low
**The test plan names one file.** Only `scripts/tools/chaos-history-test.sh` is named, for
S11-S13 (`design.md:126`). The "fake harness" of S9/S10, the "fixtures" of S14/S15 and the
"standalone tool test" of S17 have no file; the Go and Rust tests have no names. S18 has no
test for the one shared script the design changes: that `write-load.sh` without `OFFSET_BYTES`
builds the same fio command for `node-fault` and `drain-test` (`lab.sh:753,773,1275,1291`).
**Fix:** name the test files and cases per rule, and add the `write-load.sh` default case to
`scripts/tools/node-scripts-test.sh` or the new script test.

### R1-C16 — Low
**The issue's audit follow-up is not recorded in full.** The issue's comment of 2026-11-12 asks
#49 for campaigns on #76-#84. The intent's Out of scope names none; the design names #83, #84,
#80, #81 and "metadata audit" (`design.md:141-143`). #78, #79 and #82 are missing, and #82
queues a live-partition against power-off test for #49
(`origin/main:docs/issues/82/design.md:153-155`, `spec.md:93-95`).
**Fix:** list them in the intent's Out of scope as later steps of #49, naming #82's test.

### R1-S2 — Low
**Nothing says the kept and uploaded files hold no credential.** S14 keeps the history, fault
log, seed and verdict; the design adds "raw artifacts with seed/binaries" and a Lab workflow
upload (`design.md:132,146`). The harness handles the attach capability token on the same path
that yields the epoch (`metactl attach` prints both, `scripts/lab.sh:715-718`), and passes it
in `ENV_VARS`. The contract excludes tokens from the segment header only (`contracts.md:13`).
**Fix:** a rule that the run directory and the uploaded artifact hold no token, key or journal
excerpt, and a script test that greps the kept files for the fixture token.

## Nits

- `design.md:149-150` says "Q1–Q5 remain proposed"; line 152 and `decisions.md` say they are
  settled.
- `spec.md:37` says "monotonic clock"; `design.md:37` and `contracts.md:29` use
  `CLOCK_BOOTTIME`. Use one name.
- `design.md:43` bounds a frame at 1 KiB; `contracts.md:17` fixes it at exactly 128 bytes of
  body, 136 on disk.

Reviewed commit: dde1d79dbb15a68c406b8ed00a0442e2850b49e9
