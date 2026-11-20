# Plan review: issue #46 (Scrubbing and repair), round 3, focuses: design-code and design-security

Scope: `git diff 086b789..5493a62 -- docs/issues/46/ ':!docs/issues/46/report.md'` in
`/home/jsfakian/Documents/src/flaminio-46`, and the code and clear plans the changed lines depend
on (#15 and #74 on `origin/main`). Paths are under `dataplane/crates/` unless they start with
`docs/`, `scripts/`, `.github/` or `Makefile`. Line numbers of `design.md` and `spec.md` are at
HEAD.

Counts: High 0, Medium 0, Low 3, Nit 2.

## Earlier findings

- R2-B1: fixed (a duplicate of the store's descriptor; S1 narrowed to a buffered store; the test under the helper; #74 not changed). What it opens: R3-B1.
- R2-B2: fixed (`Engine::scrub_step` from `turn`, `flush` and `ScrubDone`; `due` in `next_wakeup` only with a pass running and no read out; the 1 ms rescan; the hand-off with #15).
- R2-B3: fixed (the first ticket's cluster and checksum carried; a changed entry skipped with no read; the A, B, A, B schedule does fail under `ignore_marks` and is skipped without it).
- R2-B4: fixed (a slot per holder; the test of a lease and a ticket on one cluster).
- R2-B5: fixed in substance (the rule, the doctored copies, the measurement's first step). What it opens: R3-B2, R3-B3.
- R2-B6: fixed (both kernel claims and the buffered case are cut).
- R2-N1, R2-N2, R2-N4: fixed. R2-N3: left on purpose, not reported.

## New findings

### R3-B1 — Low

**The reader's three calls need `unsafe` in `flaminio-io`, which forbids it, and the design
says `flaminio-uring` is not touched.**
The design takes the duplicate with "`AsRawFd`, then `BorrowedFd::try_clone_to_owned`", checks
`O_DIRECT` with `F_GETFL`, reads with `pread`, and says "`flaminio-uring`, `BlockDevice` and
`flaminio-repl` are not touched. Reuse existing libc" (design.md:121-128, :369-371).
- `UringDevice` gives only `AsRawFd` (`flaminio-uring/src/uring.rs:287-291`). A `RawFd`
  becomes a `BorrowedFd` only through `BorrowedFd::borrow_raw`, an `unsafe fn`. `libc::fcntl`
  and `libc::pread` are `unsafe` too.
- The workspace sets `unsafe_code = "forbid"` (`dataplane/Cargo.toml:39-40`), `flaminio-io`
  takes the workspace lints (`flaminio-io/Cargo.toml`, `[lints]`) and has no `unsafe` today.
  `flaminio-uring` is "the one crate that may use `unsafe`" (`flaminio-uring/Cargo.toml:19-23`;
  `docs/ARCHITECTURE.md:828`). `forbid` cannot be lifted by an `allow` in a module.
So the path as written does not compile, in direct mode or under `--privsep`.

The questions asked about the duplicate hold otherwise: the store reads and writes through
io_uring with an explicit offset on an unregistered descriptor (`uring.rs:176-200`, `:308`), as
the queue threads do (`flaminio-io/src/ublk.rs:1001`, `:1309`), the only seek is in `open`
before the duplicate exists (`uring.rs:137`), and #74's helper closes its copy, so one open
file description is shared by nobody outside the process.

**Fix.** Give the duplicate from `flaminio-uring`, which holds the `File`: `impl AsFd for
UringDevice` (safe, a few lines) or a `try_clone_fd() -> io::Result<OwnedFd>`, and list the
crate as touched. Take the mode from `UringDevice::is_direct()` (`uring.rs:163-165`; #74's
`from_file` sets it from the flag) instead of `F_GETFL`. Read with
`std::os::unix::fs::FileExt::read_at` on a `File` made from the `OwnedFd`, into an
`AlignedBuf`. Then `flaminio-io` gains no `unsafe`.

### R3-B2 — Low

**The lint's build-line rule fails on the tree it is added to, and it shuts the feature's
tests out of CI.**
The rule: "in the Makefile, `scripts/` and `.github/`, no line has `lab-tools` ... but the ones
this design adds: `deploy` under `LAB_TOOLS=1`, and the `cargo test` and `cargo clippy` runs of
`make test`, `make lint` and `lab.sh lint`" (design.md:269-273).
- **Its own lines.** The lint is `scripts/tools/lab-tools-lint.sh`, "run by `make lint`"
  (design.md:262): the Makefile's line that runs it has the string. So have
  `scrub-test.sh`, which runs it and writes the doctored lines (design.md:362), and the lint's
  own patterns. None is among the exceptions, so "passes on the tree" cannot hold as written,
  and the implementer chooses the exemptions of a check that is the only guard (design.md:401).
- **CI.** The Rust test job is `cd dataplane && cargo test --locked`
  (`.github/workflows/ci.yml:40`), not `make test`. The run with
  `--features flaminio-io/lab-tools` that `make test` gains (design.md:361) is therefore in no
  CI job: the fixture's tests, and R2-B4's new test, which needs the lease
  (design.md:356, :258), run only on a developer's machine. The rule forbids the workflow
  line that would run them, and `ci.yml` is not in Touched (design.md:365-369). `make lint`
  is a CI job, so only clippy sees the feature.

**Fix.** Name the lint so that its path lacks the string (for example
`fixture-feature-lint.sh`), and exempt by path exactly the lint and its script test. Add to the
exceptions, to Touched and to the S11 (fixture) row: `ci.yml`'s Rust test job gains
`cargo test --locked --features flaminio-io/lab-tools`. Keep the doctored workflow case on a
`cargo build` line.

### R3-B3 — Low

**The measurement's first step has a script test but no script.**
New: "Its first step: every node answers `fixture status` with the usage error; any other
answer stops it" (design.md:377-379), tested in `scrub-test.sh` as "the measurement stops on a
node that answers `fixture status` with anything but the usage error" (design.md:362). The
measurement is prose (design.md:377-383; spec.md:178-180): no command, Make target or script
runs it, and none is in Touched (design.md:365-369). The test has nothing to call, and "stops
it" has no mechanism: the risk it answers ("a running node tells the two builds apart only by
`fixture status`", design.md:402-403) stays a step someone must remember.

**Fix.** Name the command: for example `lab.sh scrub-build`, with its Make target, which asks
every data node for `fixture status` and fails unless each answers the usage error. The
measurement's procedure in docs/lab.md starts with it, and `scrub-test.sh` tests that command.
Or cut the script test and state the step as a manual one, with that said under Risks.

## Nits

- **N1.** design.md:357 and spec.md:164-165 ("no two consecutive wakes pass without a read issued or entries scanned"; "progress at every wake"): a completion wake followed by a turn or heartbeat wake before `due` is two such wakes on correct code. State the test as design.md:146-148 does: after every step `next_wakeup` is later than now, and a wake at `due` issues a read or scans.
- **N2.** design.md:358: `child_cannot_open_paths` is `flaminio-helper`'s root-only `tests/kernel.rs` test, not part of `flaminio-io/tests/privsep.rs`'s harness, and that harness's node blocks on `token.pub` before `build` starts the reader (`origin/main:docs/issues/74/design.md:288`). Say the pass test needs a real PKI directory and an `O_DIRECT` file, and drop the test's name.

Reviewed commit: be8f4ddc144eb771eb6ac1ccfe83d8e6e6867f10
