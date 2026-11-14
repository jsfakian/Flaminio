# Issue #16 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-14) fixes, made after approval: see below.
- Plan review round 8 (2026-11-14) fix, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

1. **What this issue builds without an operator:** (a), step 2 and the lab's renewal, as
   specified. Step 1 waits, on a new issue for the operator's CA. No issuing service now.
   `spec.md`, Open questions 1; `design.md`, Risks. (After approval, rounds 1 and 2: the CA is
   #43's step 2, and issuing on CSR stays this issue's step 1; no new issue; see below.)
2. **Step 1, for a process outside Kubernetes:** (a), an administrator issues a node's first
   pairs by hand; the node renews by presenting its current, valid certificate; a node down for
   longer than a life is issued by hand again; the CA stops renewing a removed node. It amends
   ARCHITECTURE §15.2. Not built here. `spec.md`, Open questions 2 and S12.
3. **May a connection outlive its peer's certificate:** yes, in this step: open connections are
   not closed at expiry, and that is recorded as an accepted risk. `spec.md`, Open questions 3,
   S3 and S5; `design.md`, Risks.
4. **Where the lab's CAs live for an unattended run:** where they are, on the workstation
   (`.lab/pki`), with the schedule there; the workstation stays on for the week. `spec.md`, Open
   questions 4 and S10; `design.md`, The lab's renewal.
5. **The default life, and the week:** 30 days stays the default until step 1; the checks set a
   shorter life. The week with the lab's renewal proves step 2; the issue closes only when the
   operator's CA issues. `spec.md`, Open questions 5 and S11; `design.md`, Lab.
6. **ADR 0013's open choices** (numbered 0007 at approval; renumbered in round 1, R1-C9): a poll,
   not inotify; every 15 s; the identity as the leaf's URI SANs. `design.md`, The reload rule and
   Risks. ADR 0013 (`docs/adr/0013-certificates-rotate-by-file-replacement.md`) itself stays
   Proposed; accepting it is a separate step.

## Changes after approval: plan review round 1 (2026-11-14)

Made after the approval of 2026-11-13, in round 1 of this batch's plan review of commit `89a6cc6`
(plan-reviewer (design-code), plan-reviewer (design-security)). Where two findings share a root
cause, one change closes both. Nothing is implemented or run; no approval or accepted risk is
added.

**Narrowed, because the finding shows the rule cannot hold as stated:**
- **R1-C8 (Low):** S11's lives "in minutes" are now at least 10 minutes; `flaminio-ca sign`
  refuses a shorter `--life`. `NotBefore` is 5 minutes back (`pki.go:124`), so a life L spans
  L + 5 minutes, and with L of 5 minutes or less the half-life rule finds every pair due at once.
  Chosen over measuring the half from the issue time: that would tie `pki.sh` to the CA's
  backdating, which other issuers need not share; one bound and one test case do it. Spec S11
  and Tests; design Lives, Contracts, Tests, Docs.

**Fixed without narrowing a rule:**
- **R1-C1 (High), R1-S1 (High):** no TLS session is resumed, by servers or clients, in Go and in
  Rust. rustls: `NoServerSessionStorage`, `send_tls13_tickets = 0`, `Resumption::disabled()` in
  all four configurations; Go: `SessionTicketsDisabled` in both server configurations, and no
  client sets a `ClientSessionCache`. rustls's defaults resumed sessions with no certificate and
  no expiry check, so S2, S5 and S6 failed on every Rust link. Added to spec S6 ("No TLS session
  is resumed"), which S2, S5 and S6 need as stated, and to ADR 0013's Decision, Consequences and
  Confirmation. Tests that fail with resumption on: `TestNoResumption` (Go);
  `reconnect_after_renewal_is_full` and `reconnect_after_expiry_refused` in
  `flaminio-net/tests/rotation.rs`, with the same configurations throughout. Design facts, The
  reload rule, Go, Rust, Contracts, Tests.
- **R1-C2 (Medium), R1-S2 (High):** #12's `io-pki` and #74's modes adopted. `pki.sh`'s list routes
  `io` to `/etc/flaminio/io-pki` by #12's rule (whichever of #12 and #16 lands second; until #12 has
  landed, `io` stays in `/etc/flaminio/pki`, where `node.sh` reads it); `file` writes `ca.crt` and
  `token.pub` to both directories, and `token.key` and `etcd-ca.crt` to `pki` only; `install io`
  removes `pki/io.crt` and `pki/io.key` by name; `identities` lists `io`, which the list places.
  Each name has a directory, owner and mode, and a first key's owner and mode. `install` gives the
  new key the owner, group and mode of the key it replaces (the list's for a first key) before the
  rename, so `io.key` keeps #74's 0640, group `flaminio-io`, in either landing order, and the
  metadata service's keys stay 0640 group `flaminio-meta-pki`. Keeping what it finds was chosen over
  the list alone, so that a renewal needs no change when #74 lands (round 2, R2-B1: the directory
  too; what is created first still takes the list's defaults, which follow #74). New bullet in spec
  S9; design Installing, Contracts (Paths), Risks (Security, "#12, #74"), Docs, script tests.
- **R1-C3 (Medium), R1-S5 (Low):** taken here (option a), as #74 (approved) hands it to #16. New
  rule S15: the data plane's client and the Go clients (`DialMeta` for the CSI driver's three
  roles, `flaminio-meta`'s client) accept a server only if its leaf's single URI SAN is a node's
  `meta` identity in the client's trust domain (the data plane's `--trust-domain`; for the Go
  clients, their own certificate's). Go: `pki.VerifyMeta` as `VerifyConnection`; Rust: the
  connector checks after the handshake, as `session::connect` does for `io`, with the
  workspace's `flaminio-net` as a new dependency and `Config.trust_domain`. Tests:
  `TestVerifyMeta`, `TestDialMetaChecksMeta` (Go), `refuses_a_server_that_is_not_meta` (Rust).
  Intent's What we want 6 and Out of scope; spec S14, S15, Tests; design Go, Rust, Contracts,
  Tests, Docs, Risks.
- **R1-C4 (Medium):** the metaclient test restarts its loopback server between the two calls,
  so the second reconnects, and asserts the new serial in a full handshake.
  `TestEtcdClientAcrossRenewal` signs the renewed client pair with a second CA that the
  restarted member alone trusts for clients, so only the new pair succeeds. A second CA rather
  than letting the old pair expire: no wait on the clock. Design Tests.
- **R1-C5 (Medium):** "Before, once" now issues fl-c's pairs (`make pki-issue VM=c`), deploys
  the new binaries (`make deploy`, `make deploy VM=c`) and restarts the data plane (`make
  node-up`; fl-c's through `ensure_node_c`). `due` reports a pair left in the old layout as
  `due`, so a missed migration is renewed, not skipped. The Expiry check names its partition:
  `partition.sh on` on fl-c for `HOLD`, then `off`. Spec Tests (Expiry); design Installing
  (`due`), Lab, Risks (Migration), script tests.
- **R1-C6 (Low), R1-S3 (Low):** `csr` and `install` hold `flock` on the directory's `.lock`;
  each run has its own `.new` names (`RUN`, made by `issue()`); `install`'s check, the mode, both
  renames and the old layout's removal run in one root shell that ignores SIGHUP and SIGPIPE.
  `install` removes `.new` files over an hour old, left by a run that died before its install.
  The Risks line is corrected: the last install wins, with a matching pair; a power cut between
  the renames is refused by the processes and renewed at the next pass (`due`: not matching).
  Design Installing, Risks, script tests (interleaved runs, a SIGHUP during `install`).
- **R1-C7 (Low), R1-S N2:** the old layout's removal is `rm -f` of the two files by name, as
  root, for the names that moved only (`meta`, `meta-etcd`, `agent`, and `io` once in
  `io-pki`), never a glob, so it can neither miss in an unlistable directory nor delete a pair
  just installed in `pki`. The script test makes the faked `/etc/flaminio/pki` unlistable
  (0300), so a glob would match nothing. Design Installing, script tests.
- **R1-C9 (Low):** the ADR is renumbered 0013 (`git mv`, by the orchestrator's decision: 0007
  to 0011 are the audit's, 0012 is #12's). Its title, the design's links and mentions, and
  Settled at approval 6 follow. The ADR index on this branch had no row for it; a row is added,
  status Proposed, as #12 lists its 0012.
- **R1-C10 (Low):** spec S14 and design Docs carry the settled answers: ARCHITECTURE §15.2
  records question 2's answer (first pairs by hand, renewal by the current certificate, re-issue
  by hand after a long outage, no renewal for a removed node); followups §7 records answers 1
  and 2 and links #43's step 2 for the operator's CA (see the next entry). Intent's fact 8 is
  corrected: #43 (the operator, opened 2026-11-12) exists, and its step 2 is the CA.
- **R1-C10, the operator's CA issue (orchestrator's decision, 2026-11-14):** no new issue is
  filed, so no outward step is needed. This replaces this round's first commit (`5fc68a3`),
  which said "to be filed (a new issue for the operator)" and had the maintainer file it before
  ship-spec's pull request. What #43 covers is restated in round 2 (R2-C6): its step 2
  bootstraps the CA, and issuing on CSR stays #16's step 1. Spec S14 and Open questions 1;
  design Docs; intent fact 8.
- **R1-C11 (Low):** "Approving this design accepts it; its index row comes then" is deleted.
  ADR 0013 stays Proposed; approving the plan does not accept it. Design ADRs.
- **R1-S4 (Low):** the Secret is updated with `kubectl apply --server-side --force-conflicts`, so
  its key is not copied into the `last-applied-configuration` annotation. Script case: the
  applied command. Design Installing, script tests.
- **R1-S N3:** `pki.sh` refuses a name or file not in its list and a `RUN` not matching
  `^[0-9a-z]{1,32}$`, and passes values to its root shells as positional parameters, never in
  the script text. The renewal acts only on a line that is exactly `NAME due` with `NAME` among
  `identities VM`'s names. Script cases for both. Design Installing, The lab's renewal.
- **R1-C Nit 1, R1-S N1:** the design's "(none yet)" is gone.
- **R1-C Nit 2:** the many-rotations pass counts `certificate picked up` per Flaminio process;
  etcd, which logs none, is covered by the `openssl s_client` check on 2379 and 2380. Design Lab.

## Changes after approval: plan review round 2 (2026-11-14)

Made after the approval of 2026-11-13, in round 2 of this batch's plan review of commit `46cb777`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R2-B1 (Medium):** a directory gets the list's owner and mode only when `pki.sh` creates it;
  an existing one keeps its own, as a key does. So #74's `/etc/flaminio/pki` 0750, group
  `flaminio-io`, while `io` lives there before #12 lands, survives every `csr` and `install`,
  and so does `io-pki` once `io` has moved. Today's `pki.sh:10` reapplies 0700 at every run,
  which would have taken the I/O process's access away. Round 1's "needs no change when #74
  lands" is corrected: it holds for renewals; what is created first still takes the list's
  defaults, which whichever of #16 and #74 lands second sets. Script case: `io`'s directory, in
  either landing order, and `pki` at 0750 keep their mode, with no `chown`, `chgrp` or `chmod`
  recorded for them. Design Installing, Contracts (Paths), Risks ("#12, #74"), tests.
- **R2-C1 (Medium):** the lock is one file for every name, `/etc/flaminio/.pki.lock`, outside
  every directory mounted into a pod, so `io-pki` holds exactly #12's four files after every
  completed run and #12's test still holds. Chosen over `/run/flaminio`, which #12 mounts into
  the pod. The `.new` files stay beside their targets for the rename; a run that dies between
  `csr` and `install` leaves its key there until a later install removes it (step 6), which
  the Risks say. Design Installing, Contracts, Risks; script case: no `.lock` in any directory.
- **R2-C2 (Low):** the Expiry check reads what it can see: the peers' refusal of new
  replication sessions (`session to fl-c failed`, `engine.rs:1772`), and a new metadata
  connection from fl-c, `metactl` with `AS=io`, refused naming the expiry. "The service's logs"
  is dropped: grpc logs a failed handshake below its default level, and with no HTTP/2
  keepalive fl-c's open metadata connection can outlive the partition. A new connection was
  chosen over adding keepalives, which would change the client for the check's sake. Spec
  Tests (Expiry); design Lab.
- **R2-C3 (Low):** each resumption test pairs the side under test with a peer that resumes, so
  each side's setting is tested on its own: Go `TestNoResumption` in two halves (a client with
  a `ClientSessionCache` against `(*Pair).ServerConfig()`; `(*Pair).ClientConfig()` against a
  server with tickets); Rust `Pair::server_config()` against a default `ClientConfig`, and
  `Pair::client_config()` against a default `ServerConfig`, the side under test renewed; the
  metaclient test's server keeps rustls's default tickets. Spec Tests; design Tests.
- **R2-C4 (Low):** `TestClientChecksMeta` in `meta/cmd/flaminio-meta/main_test.go`: the
  fixture's server with an `io` certificate at 127.0.0.1, `f.run(admin, "nodes")` fails naming
  the identity; with its `meta` certificate, answered. The file joins Files touched. Spec Tests
  (S15); design Tests, Files touched.
- **R2-C5 (Low):** the `pki.sh` tests run as a non-root user with recording stubs: `flock` runs
  its command with a marker, and the `chown`, `chmod`, `mv`, `openssl` and `sync` stubs record
  whether they ran under it, so the lock case fails without the lock; `chown`, `chgrp` and
  `groupadd` only record, so no root and no `flaminio-*` group are needed, and the owner and
  group are asserted from the record; modes are read for real. `pki.sh` reads the replaced
  key's owner, group and mode with `stat -c`, falling back to BSD `stat -f`, and applies them
  with `chown` and `chmod`: no GNU-only `--reference`, so the cases run on macOS. Design
  Installing, tests.
- **R2-C6 (Low), the orchestrator's decision:** following #43 (approved, review-clear), whose
  issue reads "2. CA bootstrap: the operator holds the internal CA. Issuing on CSR is #16.",
  #43's step 2 bootstraps the CA, and issuing on CSR, with its TokenReview and question 2's
  answer, stays #16's step 1, built once that CA exists. This replaces round 1's "#43 already
  covers it"; still no new issue. Intent fact 8 and Out of scope; spec S14 and Open questions
  1; design Docs; Settled at approval 1's note.
- **R2-S1 (Low):** `install` syncs the two `.new` files before the renames and `sync -f`s the
  name's directory, and `/etc/flaminio/pki` when it removed files there, after the removals;
  `file` syncs its temporary file before the rename. A first key in a new directory replaces
  nothing, so no rename-over flush covered it. The Risks' crash window is now the gap between
  the two renames only. Design Installing, Risks, tests.
- **R2-N1:** S15 says `flaminio-meta`'s `--trust-domain` serves `confirm` only; the check uses
  its certificate's trust domain.
- **R2-N2:** `due`'s old-layout clause applies to the names that moved only, as `install`'s step
  4 does; an `etcd` pair in `pki` is `fresh` in the script case.
- **R2-N3:** `RUN` is the time in seconds and the pid concatenated with no separator, which the
  pattern `^[0-9a-z]{1,32}$` requires.
- **R2-N4:** ADR 0013 says "Flaminio's servers" keep no session; etcd's own servers keep Go's
  default tickets, and no etcd client resumes.

## Changes after approval: plan review round 3 (2026-11-14)

Made after the approval of 2026-11-13, in round 3 of this batch's plan review of commit `b40a661`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R3-1 (Medium):** the Expiry check's replication observation is corrected against the code.
  It reads the lines that carry the TLS error and requires the rustls expiry text in them: on a
  peer, the connecting side, `session to fl-c lost: <why>` (`engine.rs:828-831`, `<why>` from
  `net.rs:294`) with `invalid peer certificate: certificate expired`; on fl-c, the accepting
  side, `flaminio-net: session from <peer>: <e>` (`net.rs:196`) with `received fatal alert:
  CertificateExpired`. The `session to fl-c failed` line (`engine.rs:1772`) carries only
  `a request timed out` and kinds like it, which the partition alone produces, and is dropped.
  The texts are rustls 0.23.46's (`ExpiredContext`, alert `CertificateExpired`), and
  `reconnect_after_expiry_refused` asserts them. Design Lab, Tests.
- **R3-2 (Low):** the `pki.sh` tests also record `rm`, `find`, `stat` and `mkdir`, so the lock
  case sees the key check, the read, the removals and the renames under the lock. A missing
  directory is made with `mkdir`, then `chown` and `chmod`, never `install -d`, so the recorded
  commands are the only ones that set a directory's owner and mode. Every path, the lock's
  included, is under `FLAMINIO_ETC` (default `/etc/flaminio`), which only the test sets. Design
  Installing, tests.
- **R3-3 (Low):** `TestClientChecksMeta` uses a new `newFixtureAs(t, serverID string)`;
  `newFixture(t)` wraps it with today's `meta` identity, so the other tests are unchanged, and
  `writeIdentity` already signs for 127.0.0.1. With an `io` server identity the call's error
  contains `<id> is not the metadata service`, the text `pki.VerifyMeta` (and the Rust
  connector) now names. Design Go, Rust, Tests.
- **R3-4 (Low):** spec S9 carries the directory rule: an existing directory keeps its owner,
  group and mode, and gets its default only when created. The spec's S9–S11 test list names
  it. Spec S9, Tests.
- **R3-5 (Low):** "`io-pki` holds exactly #12's four files after every `install` and `file`";
  between a `csr io` and its `install` the run's `.new` key sits there too, as the Risks say.
  Chosen over staging the key outside `io-pki`: the rename would then cross directories, and
  exact wording suffices. Design Installing, Risks.
- **R3-6 (Low):** `TestNoResumption`'s Go-client half guards against a `ClientSessionCache`
  being added to `(*Pair).ClientConfig()`, the only way a Go client resumes; only the server
  half fails if its setting (`SessionTicketsDisabled`) goes. Design Tests.
- **R3-7 (Low):** `file` runs `sync -f` on its directory after the rename, as `install` does, so
  a first `ca.crt` or `token.pub` in a new directory survives a power cut. Design Installing;
  the script case checks the order.
- **R3-N1:** intent's Out of scope says the open questions settle where the CA comes from
  (#43's step 2), not "an issue for the operator".

## Changes after approval: plan review round 4 (2026-11-14)

Made after the approval of 2026-11-13, in round 4 of this batch's plan review of commit `3647c13`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R4-1 (Low):** in TLS 1.3 a client whose certificate is refused finishes `connect` and sees
  the server's alert only at its first read. `reconnect_after_expiry_refused` therefore reads a
  frame (the hello's reply) before asserting in the half where the connecting peer's pair
  expired, and asserts each side's text for each direction: the verifying side
  `invalid peer certificate: certificate expired`, the other `received fatal alert:
  CertificateExpired`. The lab's Expiry check matches either text in either line
  (`session to <node> lost:`, `flaminio-net: session from <peer>:`) on any host, as the texts
  swap with the direction. Design Tests, Lab.
- **R4-2 (Low):** the root override is renamed `PKI_ROOT`, the name #12 uses for `pki.sh`, and is
  honoured only under `FLAMINIO_SCRIPT_TEST=1`, the switch `common.sh` already honours for
  `CONTROL_SOCKET`; otherwise it is ignored. Chosen over refusing it for a root caller: one
  rule the scripts already follow, shared with #12. The root reaches the root shell as a
  positional parameter, since `sudo` resets the environment. The test's `sudo` stub refuses any
  command naming `/etc/flaminio`, as #12's does, and a case sets `PKI_ROOT` without the switch:
  nothing is written in the temporary directory and the refusal fails the script. Round 3's
  `FLAMINIO_ETC` is gone. Design Installing, Contracts (CLI), tests.
- **R4-3 (Low):** a directory is made with `mkdir -m 0700`, then `chown`, then `chmod 0750` for a
  group directory, so it is never wider than its final mode; a `mkdir` that finds it made by a
  run in between leaves it alone. Only `/etc/flaminio` is made before the lock, which lives in
  it; every name's directory is made under the lock. `file` holds the lock too, as it may make a
  directory, and the lock case covers it; `due`, which writes nothing, takes no lock (a pair
  caught between the renames reads as not matching, so at worst it is renewed once more). Script
  cases: the record's order for a new directory, and `file` under the lock. Design Installing,
  tests.
- **R4-N1:** the spec's test line says "an existing directory's owner, group and mode kept", as
  S9 does. Spec Tests.

## Changes after approval: plan review round 5 (2026-11-14)

Made after the approval of 2026-11-13, in round 5 of this batch's plan review of commit `7f24552`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R5-B1 (Low):** a new directory is built under the lock under a temporary name beside it,
  `DIR.RUN.new` (the pid for `file`): `mkdir -m 0700`, `chown`, `chmod 0750` for a group
  directory, then `mv -T` to its final name. A crash leaves either no directory, which the next
  run builds, or the finished one, never a half-set one that "an existing directory keeps its
  own" would make permanent. `install`'s step 6 removes temporary directories over an hour old,
  as it does `.new` files. The test's `mv` wrapper keeps the cases running on macOS, whose `mv`
  lacks `-T`, by checking that the destination is absent. Script cases: the record's order, and
  a run killed after the temporary directory's `chown` (no final directory; the next run builds
  it whole; an aged temporary directory removed). Design Installing, tests.
- **R5-B2 (Low):** the root is made before the lock with `sudo mkdir -p -m 0755 -- "$root"`, a
  no-op when it exists or a concurrent run made it first; `$root` is the resolved root, so the
  tests keep it under `PKI_ROOT`. Every command that touches the tree goes through `sudo`, so in
  the no-switch case the `sudo` stub's refusal of the real path also keeps a test run as root off
  the real `/etc/flaminio`, and the case asserts no `mkdir` is recorded. Design Installing,
  tests.
- **R5-B3 (Low):** in the half with the server under test, the expired peer is a bare
  `TlsConnector` that sends nothing after the handshake and then reads, so no reset can replace
  the alert and both texts stay asserted. Chosen over tolerating `connection reset`, which would
  leave the connector's text unchecked; the server under test still runs `session::accept`.
  Design Tests.

## Changes after approval: plan review round 6 (2026-11-14)

Made after the approval of 2026-11-13, in round 6 of this batch's plan review of commit `05ea0e1`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R6-1 (Low):** `install`'s step 6 names its commands. This name's `.new` files:
  `find DIR -maxdepth 1 -type f -name 'NAME.*.new' -mmin +60 -delete`. Temporary directories,
  which sit beside the directories: a scan of the root, anchored to the known directory names
  (`pki`, `meta-pki`, `agent-pki`, `io-pki`, then `.[0-9a-z]*.new`, which covers `RUN` and the
  pid form `file` uses), `-type d`, `-mmin +60`, removed with `rmdir`. `rmdir`, not a recursive
  delete as root on a pattern: the directory is empty by construction, and a refused `rmdir`
  leaves it, logged. The script case also checks that the same `install` keeps a fresh
  temporary directory, every final directory, and an aged `.new` file of another name. Design
  Installing, tests.
- **R6-2 (Low):** the bare connector "removes the known cause" (a hello written after the
  server closed), as worded now. The other cause the reviewer names, the server closing with
  part of the client's flight unread, is removed as well, since the harness allows it simply:
  the test certificates are one small leaf each, and the test keeps a `try_clone` of the
  server's socket, so after `accept` fails it shuts down writing and reads to the end before
  dropping it. Both texts stay asserted. Design Tests.

## Changes after approval: plan review round 7 (2026-11-14)

Made after the approval of 2026-11-13, in round 7 of this batch's plan review of commit `3c27987`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added. No rule is narrowed.

- **R7-1 (Low):** the drain in `reconnect_after_expiry_refused` is specified: the connector runs
  in its own task (`smol::spawn`), so the drain's end of file, which comes when the connector is
  dropped, cannot wait on the server half; the test awaits the server half first (`accept`'s
  error, then the drain), then the connector's task; the clone, which shares the runtime's
  `O_NONBLOCK`, is read through the runtime (`smol::Async`); the drain has a 5 s timeout, so a
  failure is an error, not a hang. Design Tests.
- **R7-2 (Low):** each cleanup `find` in `install`'s step 6 ends in `|| echo … >&2`, so a refused
  removal is logged and the install still succeeds, whatever the root shell's `set -e`. Script
  case: an aged temporary directory that holds a file stays, the `install` succeeds, and the
  line is logged. Design Installing, tests.

## Changes after approval: plan review round 8 (2026-11-14)

Made after the approval of 2026-11-13, in round 8 of this batch's plan review of commit `ba823dc`
(reviewer: plan-reviewer (design-code and design-security, one agent)). No rule is narrowed.

- **R8-1 (Low):** the drain in `reconnect_after_expiry_refused` only delays the close. End of
  file, any read error (`connection reset` included) or the 5 s timeout ends it, and none of
  them is asserted; the assertions stay on `accept`'s error and the connector's error text.
  Design Tests.
