# Issue #16 — report

## Review rounds

### Plan review round 1 — 89a6cc6

Scope: correctness against code and security of the plan, approved 2026-11-13; fixes are
changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High, R1-S1 — High:** rustls's defaults resume TLS sessions; a resumed handshake
  sends no certificate and the rustls server takes the stored chain without checking its expiry,
  so on every Rust link a reconnection after a renewal presents the old pair and an expired peer
  keeps being accepted, against S2, S5, S6 and the lab's Expiry check. Evidence: rustls 0.23.46
  `server/builder.rs:113`, `:123`, `server/tls13.rs:371-375`, `client/builder.rs:172`,
  `client/hs.rs:1136`; `flaminio-net/src/tls.rs:68-90`; `session.rs:113`, `:197`; spec.md S2,
  S5, S6; design.md:43-45, :67-73, :167-170, :202-203.
- **R1-C2 — Medium, R1-S2 — High:** #12's `/etc/flaminio/io-pki` and #74's `io.key` 0640 group
  `flaminio-io` were missing; `csr` makes every key root 0600 and `install` moves it into place,
  so a renewal would make `io.key` (and `meta-pki`'s keys) unreadable to their unprivileged
  readers. Evidence: design.md:87-99, :142-143, :240; `pki.sh:16`; 12/design.md:131-150;
  74/design.md:206, :369.
- **R1-C3 — Medium, R1-S5 — Low:** #74's follow-up "metadata clients check the `meta` URI SAN"
  had no owner; the clients check only the CA and the address, so an `io` key can answer as the
  metadata service at its node's address. Evidence: intent.md:87-88; 74/decisions.md:196-199;
  `pkg/pki/pki.go:300-310`; `flaminio-metaclient/src/lib.rs:85-92`.
- **R1-C4 — Medium:** the metaclient test reused one HTTP/2 connection, and the etcd test could
  not tell the new pair from the old, still-valid one: neither could fail without the change.
  Evidence: design.md:161-162, :169-170; `flaminio-metaclient/src/lib.rs:106-108`.
- **R1-C5 — Medium:** the lab plan neither deployed nor restarted the new data plane, left fl-c's
  pairs in the old layout (which `due` reported `absent`), and named no partition reaching fl-c.
  Evidence: design.md:95-97, :195-203; Makefile:311; `lab.sh:575`, `:803-821`, `:1228`.
- **R1-C6 — Low, R1-S3 — Low:** `install`'s check and two renames were neither locked against a
  second renewal nor safe against an ssh drop; "install refuses the loser's" was wrong.
  Evidence: design.md:90-93, :248; `listener.go:474`.
- **R1-C7 — Low, security N2:** the old layout's removal was a glob the VM user cannot expand in
  the 0700 directory, and read for every name it would delete pairs just installed there.
  Evidence: design.md:92-93; `pki.sh:10`; 12/design.md:140-142.
- **R1-C8 — Low:** with `NotBefore` 5 minutes back, a life of 5 minutes or less is always due.
  Evidence: design.md:96, :120; spec.md:96-97; `pki.go:124`.
- **R1-C9 — Low:** ADR number 0007 collided with the audit's ADRs. Evidence:
  `docs/adr/0007-certificates-rotate-by-file-replacement.md:1`; design.md:30, :140, :249,
  :256-258; decisions.md:30-32.
- **R1-C10 — Low:** settled answers 1 and 2 did not reach the docs plan, and no step filed the
  operator's CA issue. Evidence: decisions.md:14-20; spec.md:107-110; design.md:209-215.
- **R1-C11 — Low:** the design said approving it accepts the ADR, against decisions.md.
  Evidence: design.md:256-258; decisions.md:31-33.
- **R1-S4 — Low:** a client-side `kubectl apply` of the Secret copies its private key into the
  `last-applied-configuration` annotation. Evidence: design.md:103-104.
- **Code nit 1, security N1:** "(none yet)" was stale. Evidence: design.md:3.
- **Code nit 2:** "ten `certificate picked up` per process" included etcd, which logs none.
  Evidence: design.md:198.
- **Security N3:** names crossing the VM boundary were not validated. Evidence: design.md:87-97,
  :109-114.

Changes/resolution:
- R1-C1, R1-S1: no TLS session is resumed, in Go and in Rust (rustls `NoServerSessionStorage`,
  `send_tls13_tickets = 0`, `Resumption::disabled()`; Go `SessionTicketsDisabled`, no
  `ClientSessionCache`). Added to spec S6 and to ADR 0013's Decision. Tests that fail with
  resumption on: `TestNoResumption`; `reconnect_after_renewal_is_full` and
  `reconnect_after_expiry_refused` in `flaminio-net/tests/rotation.rs`.
- R1-C2, R1-S2: `pki.sh`'s list routes `io` to `io-pki` by #12's rule (whichever of #12 and #16
  lands second); `file` writes `ca.crt` and `token.pub` to both directories; `install io`
  removes the old pair by name. Each name has a directory, owner and mode; `install` gives the
  new key the owner, group and mode of the key it replaces (the list's for a first key) before
  the rename. New spec S9 bullet; Paths, Security, a "#12, #74" risk, script cases.
- R1-C3, R1-S5: taken here as new rule S15: the Rust connector and the Go clients (`DialMeta`,
  `flaminio-meta`'s client, through `pki.VerifyMeta`) require the server leaf's single URI SAN
  to be a node's `meta` in the client's trust domain. Intent What we want 6; tests in Go and
  Rust.
- R1-C4: the metaclient test restarts its server between the calls; the etcd test signs the
  renewed pair with a second CA that the restarted member alone trusts.
- R1-C5: "Before, once" adds `make pki-issue VM=c`, `make deploy` (and `VM=c`), `make node-up`
  and fl-c's node through `ensure_node_c`; `due` reports an old-layout pair as `due`; the Expiry
  check partitions fl-c (`partition.sh on`, `HOLD`, `off`).
- R1-C6, R1-S3: `csr` and `install` hold `flock` on the directory, each run has its own `.new`
  names (`RUN`), and `install` runs its check, modes and renames in one root shell that ignores
  SIGHUP; stale `.new` files over an hour old go. Risks corrected; script cases for interleaved
  runs and a SIGHUP mid-install.
- R1-C7, security N2: removal of the two old files by name, as root, for the names that moved
  only; the test makes the faked old directory unlistable.
- R1-C8: `--life` is at least 10 minutes (spec S11 narrowed, as the rule cannot hold below);
  test cases `--life 10m` and `9m`.
- R1-C9: ADR renamed to 0013 with `git mv`; its title, the plan's references, and a Proposed row
  in `docs/adr/README.md` (the branch had none).
- R1-C10: spec S14 and design Docs carry §15.2's amendment (question 2's answer) and followups
  §7's answers 1 and 2. Step 1 points to #43 ("The operator"), whose step 2 is CA bootstrap and
  issuing on CSR: by the orchestrator's decision no new issue is filed, as #43 already covers it
  and no outward step is needed (the first commit had said "to be filed"). Intent fact 8 notes
  #43.
- R1-C11: the sentence is deleted; ADR 0013 stays Proposed.
- R1-S4: `kubectl apply --server-side --force-conflicts`; script case.
- Code nit 1, security N1: "(none yet)" removed. Code nit 2: "per Flaminio process", etcd by
  `openssl s_client`. Security N3: `pki.sh` refuses unknown names and files and a bad `RUN`,
  passes values as positional parameters; the renewal acts only on exact `NAME due` lines for
  `identities VM`'s names.

Commits: `5fc68a3` (the fixes), then `7eaf4b7` (the operator's CA points to #43's step 2, by
the orchestrator's decision). Recorded in decisions.md as changes made after approval, plan
review round 1 (2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 46cb777

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 1's changes (`89a6cc6..7eaf4b7`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 1's findings were found fixed, except R1-C2 and R1-S2 (partly: the directory's mode was
not kept, R2-B1); the gaps found in R1-C1, R1-C3, R1-C5, R1-C6 and R1-C10's fixes are the items
below.

- **R2-B1 — Medium:** "keeps what it finds" covered the key, not its directory. With #74 landed
  before #12, `csr`/`install` would reset `/etc/flaminio/pki` from #74's 0750 group
  `flaminio-io` to 0700, as `pki.sh:10` does at every run, and the I/O process would refuse
  every renewal until the pair expired. Evidence: decisions.md:71-73; design.md:131-134,
  :144-145, :370-372; 74/design.md:206, :216; `scripts/guest/pki.sh:10`.
- **R2-C1 — Medium:** the lock left `.lock` in `io-pki`, against #12's contract and test that it
  holds exactly four files. Evidence: design.md:137-138, :143; 12/design.md:131-133, :750-755;
  12/decisions.md:402.
- **R2-C2 — Low:** the Expiry check could not see "its metadata calls are refused (… the
  service's logs)": grpc logs a failed handshake below its default level, and with no HTTP/2
  keepalive fl-c's open connection survives the partition. Evidence: design.md:314-317;
  `grpc@v1.85.0/server.go:1061`; `grpclog/loggerv2.go:67`.
- **R2-C3 — Low:** the resumption tests used the same configurations on both ends, so they
  passed with one side's setting missing. Evidence: design.md:254-262; ADR 0013 Decision.
- **R2-C4 — Low:** no test covered S15 for `flaminio-meta`'s client. Evidence: spec.md:121-129;
  design.md:85-86, :193-195, :244-251.
- **R2-C5 — Low:** the `pki.sh` tests stubbed `flock` away, could not create the owner and group
  as a non-root user, and relied on GNU-only `--reference` on macOS. Evidence:
  design.md:142-146, :275-282; `node-scripts-test.sh:12-13`.
- **R2-C6 — Low:** the #43 citation contradicted #43's issue ("2. CA bootstrap: the operator
  holds the internal CA. Issuing on CSR is #16."), so each issue pointed to the other for
  issuing on CSR. Evidence: decisions.md:118-119; spec.md:113-115, :175-176; 43/intent.md:80-81.
- **R2-S1 — Low:** no `.new` file or directory was synced, so a power cut could leave empty new
  files, a first key included, after the old files' removal. Evidence: design.md:146-151,
  :379-384.
- **R2-N1:** `flaminio-meta` has a `--trust-domain` (confirm only). Evidence: spec.md:127-128;
  `main.go:310`.
- **R2-N2:** `due`'s old-layout clause was not limited to the names that moved. Evidence:
  design.md:154.
- **R2-N3:** `RUN`'s parts must be concatenated with no separator. Evidence: design.md:136, :174.
- **R2-N4:** "Servers keep no session" read as covering etcd. Evidence: ADR 0013:34-35.

Changes/resolution:
- R2-B1: a directory gets the list's owner and mode only when `pki.sh` creates it; an existing
  one keeps its own, as a key does. Round 1's "needs no change when #74 lands" now holds for
  renewals, and the plan says that what is created first takes defaults that follow #74. Script
  case: `io`'s directory in either landing order, and `pki` at 0750, keep their mode, with no
  `chown`, `chgrp` or `chmod` recorded for them.
- R2-C1: one lock for every name, `/etc/flaminio/.pki.lock`, outside every mounted directory;
  `io-pki` holds #12's four files after every completed run. A run that dies between `csr` and
  `install` leaves its `.new` key until a later install removes it, as the Risks say.
- R2-C2: the check reads the peers' `session to fl-c failed` (`engine.rs:1772`) and a new
  metadata connection from fl-c, `metactl` with `AS=io`, refused naming the expiry; "the
  service's logs" is dropped.
- R2-C3: each side is tested against a peer that resumes (Go `TestNoResumption` in two halves;
  Rust `Pair::server_config()` against a default `ClientConfig`, `Pair::client_config()`
  against a default `ServerConfig`); the metaclient test's server keeps default tickets.
- R2-C4: `TestClientChecksMeta` in `meta/cmd/flaminio-meta/main_test.go`, added to Files
  touched.
- R2-C5: the tests run as a non-root user with recording stubs; `flock` marks its command, and
  the lock case fails if a step runs outside it; owner and group are asserted from the `chown`
  stub; modes are read for real; `pki.sh` reads the replaced key with `stat -c`, falling back
  to BSD `stat -f`, instead of `--reference`.
- R2-C6 (the orchestrator's decision, following #43, approved and review-clear): #43's issue is
  quoted; its step 2 bootstraps the CA, and issuing on CSR, with its TokenReview and question 2's
  answer, stays #16's step 1, built once that CA exists. In S14, Open questions 1, design Docs,
  intent and decisions, replacing round 1's "#43 already covers it".
- R2-S1: `install` syncs the `.new` files before the renames and `sync -f`s the directories after
  the removals; `file` syncs before its rename. The Risks' crash window is the gap between the
  renames.
- R2-N1: S15 says the check uses the certificate's trust domain, not `--trust-domain`. R2-N2:
  `due` limited to moved names, with an `etcd` case. R2-N3: `RUN` concatenated with no
  separator. R2-N4: the ADR says "Flaminio's servers"; etcd keeps its tickets and no etcd client
  resumes.

Commit: `34b4ad5`. Recorded in decisions.md as changes made after approval, plan review round 2
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — b40a661

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 2's changes (`46cb777..34b4ad5`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 2's findings were found fixed except R2-C2 (not fixed: R3-1) and R2-C5 (partly: R3-2);
gaps in R2-B1, R2-C1, R2-C3, R2-C4, R2-C6 and R2-S1's fixes are the items below.

- **R3-1 — Medium:** the Expiry check's replication observation did not exist in the code:
  `session to fl-c failed: <reason>` (`engine.rs:1770-1772`) carries only a `&'static str` such
  as "a request timed out" (`target.rs:825`, `:997`, `:1503`), never a TLS error, so the check
  would match the line the partition alone produces. A refused handshake is logged as
  `session to fl-c lost: <why>` (`engine.rs:828-831`, `net.rs:294`) and, on the accepting side,
  `flaminio-net: session from <peer>: <e>` (`net.rs:196`). Evidence: design.md:356-358.
- **R3-2 — Low:** the `pki.sh` stub list did not record `rm`, `find` or `stat`, so the lock case
  could not see the removals or the key check; the design did not say directories are made with
  `mkdir` plus the stubbed `chown` and `chmod`, nor how the test sets its root. Evidence:
  design.md:296-306; `pki.sh:9-10`.
- **R3-3 — Low:** `newFixture(t)` hard-codes the server identity, and the design did not say how
  `TestClientChecksMeta` gets an `io` server certificate. Evidence: `main_test.go:74`.
- **R3-4 — Low:** spec S9 lacked the rule that an existing directory keeps its mode and group,
  and the spec's tests did not name it. Evidence: spec.md:82.
- **R3-5 — Low:** "`io-pki` holds exactly #12's four files after every completed run" is false
  between a `csr io` and its `install`. Evidence: design.md:139.
- **R3-6 — Low:** `TestNoResumption`'s Go-client half cannot fail when a setting "goes": a Go
  client without a `ClientSessionCache` never resumes. Evidence: design.md:251-254.
- **R3-7 — Low:** `file` synced its temporary file but not the directory, the same window as
  R2-S1 for a first `ca.crt` or `token.pub` in a new directory. Evidence: design.md:164.
- **R3-N1:** intent's Out of scope still said the open questions record "an issue for the
  operator". Evidence: intent.md:85.

Changes/resolution:
- R3-1: the check reads the lines that carry the TLS error and requires rustls's expiry text:
  on a peer, `session to fl-c lost: <why>` with `invalid peer certificate: certificate expired`;
  on fl-c, `flaminio-net: session from <peer>: <e>` with `received fatal alert:
  CertificateExpired` (rustls 0.23.46, `ExpiredContext` and its alert). The timeout line is
  dropped. `reconnect_after_expiry_refused` asserts both texts.
- R3-2: the stubs also record `rm`, `find`, `stat` and `mkdir`; a missing directory is made with
  `mkdir`, then `chown` and `chmod`, never `install -d`; every path, the lock's included, is
  under `FLAMINIO_ETC` (default `/etc/flaminio`), set only by the test.
- R3-3: `newFixtureAs(t, serverID string)`, with `newFixture(t)` wrapping it with today's `meta`
  identity; with an `io` server identity the error contains `<id> is not the metadata service`,
  the text `pki.VerifyMeta` and the Rust connector now name.
- R3-4: spec S9 adds "So does its directory"; the spec's S9–S11 test list names the case.
- R3-5: "after every `install` and `file`"; the `.new` key between `csr io` and `install` is in
  the Risks. Staging outside `io-pki` was not taken: the rename would cross directories.
- R3-6: the Go-client half guards against a `ClientSessionCache` being added; only the server
  half fails if `SessionTicketsDisabled` goes.
- R3-7: `file` runs `sync -f` on its directory after the rename; the script case checks the
  order.
- R3-N1: intent's Out of scope says the open questions settle where the CA comes from (#43's
  step 2).

Commit: `3659ed2`. Recorded in decisions.md as changes made after approval, plan review round 3
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 3647c13

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 3's changes (`b40a661..3659ed2`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 3's findings were found fixed (R3-2 mostly; its remaining gaps are R4-2 and R4-3).

- **R4-1 — Low:** in TLS 1.3 a client whose certificate the server refuses finishes `connect`
  and sees the alert only at its first read, so `reconnect_after_expiry_refused` asserted the
  texts on the wrong side when the expired side connects; in the lab the texts swap with the
  direction, so the per-side lines held only for sessions peers open to fl-c. Evidence:
  `session.rs:194-205`; rustls 0.23.46 `error.rs:701`, `:985`, `:987`; design Tests, Lab.
- **R4-2 — Low:** `FLAMINIO_ETC` was an unconditional override of a root-owned tree, and `sudo`'s
  `env_reset` would drop it for the locked root shell. Evidence: design Installing.
- **R4-3 — Low:** directory creation ran outside the lock and could briefly leave a directory
  wider than its final mode; `file` was not stated to hold the lock, and the lock test did not
  cover it. Evidence: design Installing, tests.
- **R4-N1:** the spec's test line said "mode and group", S9 "owner, group and mode". Evidence:
  spec Tests.

Changes/resolution:
- R4-1: the test reads a frame (the hello's reply) before asserting in the half where the
  connecting peer's pair expired, and asserts each side's text per direction: the verifying side
  `invalid peer certificate: certificate expired`, the other `received fatal alert:
  CertificateExpired`. The lab check matches either text in either line
  (`session to <node> lost:`, `flaminio-net: session from <peer>:`) on any host.
- R4-2: the override is `PKI_ROOT`, the name #12 uses, honoured only under
  `FLAMINIO_SCRIPT_TEST=1` (the switch `common.sh` already honours); chosen over refusing it for
  a root caller as the one rule the scripts already follow. It reaches the root shell as a
  positional parameter. The test's `sudo` stub refuses any command naming `/etc/flaminio`, as
  #12's does, and a case sets `PKI_ROOT` without the switch: nothing is written and the script
  fails on the refusal.
- R4-3: directories are made with `mkdir -m 0700`, then `chown`, then `chmod 0750` for a group
  directory, never wider than final; a directory found made in between is left alone. Only
  `/etc/flaminio` is made before the lock; every name's directory, and `file`, run under it.
  Script cases: the order for a new directory, and `file` under the lock. `due` writes nothing
  and takes no lock, as the design says.
- R4-N1: the spec's line says "owner, group and mode".

Commit: `410d667`. Recorded in decisions.md as changes made after approval, plan review round 4
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 7f24552

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 4's changes (`3647c13..410d667`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 4's findings were found fixed (R4-3 mostly); the residual gaps are the items below.

- **R5-B1 — Low:** a run killed between `mkdir -m 0700` and `chmod 0750` left a group directory
  at 0700, which "an existing directory keeps its own" then made permanent, so the group could
  never read its key and `meta.sh up` would fail with no renewal fixing it. Evidence: design
  Installing.
- **R5-B2 — Low:** the pre-lock `mkdir` of `/etc/flaminio` stated neither `sudo` nor a concurrent
  run; a bare `mkdir` would bypass the `sudo` stub and, where the tests run as root, create the
  real directory, and a plain `mkdir` fails when another run made it first. Evidence: design
  Installing, tests.
- **R5-B3 — Low:** in the half with the server under test, the expired connector sends hello
  after `connect` returns, and a hello that reaches the server's closed socket can make the read
  return ECONNRESET instead of the alert. Evidence: `session.rs:204`; design Tests.

Changes/resolution:
- R5-B1: a new directory is built under the lock as `DIR.RUN.new` (the pid for `file`):
  `mkdir -m 0700`, `chown`, `chmod 0750` for a group directory, then `mv -T` to its name, so a
  crash leaves no directory or the finished one. `install`'s step 6 removes temporary
  directories over an hour old. The test's `mv` wrapper checks that the destination is absent
  where BSD `mv` lacks `-T`. Script cases: the order, and a run killed after the temporary
  directory's `chown`.
- R5-B2: `sudo mkdir -p -m 0755 -- "$root"` before the lock, with `$root` resolved (so under
  `PKI_ROOT` in tests); every command touching the tree goes through `sudo`, so the stub's
  refusal keeps a root test run off the real directory, and the no-switch case asserts no
  `mkdir` is recorded.
- R5-B3: the expired peer is a bare `TlsConnector` that sends nothing and reads, so both texts
  stay asserted; chosen over tolerating `connection reset`, which would leave the connector's
  text unchecked.

Commit: `35b7a52`. Recorded in decisions.md as changes made after approval, plan review round 5
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — 05ea0e1

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 5's changes (`7f24552..35b7a52`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 5's findings were found fixed (R5-B3 mostly; its remaining claim is R6-2).

- **R6-1 — Low:** `install`'s step 6 named neither the command nor the match for aged temporary
  directories: its `find DIR` looks inside one name's directory, while the temporary
  directories sit beside it in the root, and a bare `*.new` there or a recursive delete would be
  too wide; the case did not check that a fresh temporary directory or a final one survives.
  Evidence: design Installing, step 6; tests.
- **R6-2 — Low:** "no reset can replace the alert" was stronger than the facts: the server can
  close with part of the client's flight unread, and Linux then sends a reset. Evidence: design
  Tests, `reconnect_after_expiry_refused`; rustls 0.23.46.

Changes/resolution:
- R6-1: step 6 removes this name's `.new` files with `find DIR -maxdepth 1 -type f -name
  'NAME.*.new' -mmin +60 -delete`, and temporary directories with a scan of the root anchored to
  `pki`, `meta-pki`, `agent-pki` and `io-pki` followed by `.[0-9a-z]*.new`, `-type d`,
  `-mmin +60`, removed with `rmdir` (empty by construction; a refusal is logged, not fatal). The
  case checks that the same `install` keeps a fresh temporary directory, every final directory,
  and an aged `.new` file of another name.
- R6-2: the text says the bare connector removes the known cause (the hello after the close).
  The test certificates are one small leaf each, and the test keeps a `try_clone` of the
  server's socket, so after `accept` fails it shuts down writing and drains before dropping it:
  no unread byte at the close. Both texts stay asserted.

Commit: `126b7e2`. Recorded in decisions.md as changes made after approval, plan review round 6
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 3c27987

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental over
round 6's changes (`05ea0e1..126b7e2`); fixes are changes made after approval. No runtime
implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Round 6's findings were found fixed; the drain added for R6-2 is R7-1.

- **R7-1 — Low:** the drain lacked what makes it work: awaited before the connector is polled or
  dropped, it waits for an end of file that never comes; `try_clone` shares the runtime's
  `O_NONBLOCK`, so a blocking read returns `WouldBlock`; and it had no timeout. Evidence: design
  Tests, `reconnect_after_expiry_refused`.
- **R7-2 — Low:** `find … -exec rmdir {} +` and `find -delete` exit nonzero on a refused removal,
  so "logged, without failing the install" did not hold as written; no case covered a refused
  `rmdir`. Evidence: design Installing, step 6.

Changes/resolution:
- R7-1: the connector runs in its own task (`smol::spawn`); the test awaits the server half
  first (`accept`'s error, then the drain), then the connector's task; the clone is read through
  the runtime (`smol::Async`); the drain has a 5 s timeout.
- R7-2: each cleanup `find` ends in `|| echo … >&2`; a script case: an aged temporary directory
  that holds a file stays, the `install` succeeds, and the line is logged.

Commit: `2d18860`. Recorded in decisions.md as changes made after approval, plan review round 7
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — ba823dc

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- Both round-7 findings fixed (R7-1 the drain specified; R7-2 `|| echo` after each cleanup `find`).
- **R8-1 — Low:** the design did not say what the drain does with a read error; an implementer who
  unwraps the read makes the test flaky on the reset it was meant to remove. Evidence:
  design.md:336–341.

Changes/resolution: R8-1: the drain only delays the close; end of file, any read error or the 5 s
timeout ends it, and none is asserted. Recorded in decisions.md as round 8, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 46fec33449fe49042ab4f457e89efc1a49ebd5dc

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
