# Issue #16 plan review, round 2 (incremental), design-code and design-security

Scope: `git diff 89a6cc6..7eaf4b7 -- docs/issues/16/ docs/adr/ ':!docs/issues/16/report.md'` (5fc68a3, 7eaf4b7) in /home/jsfakian/Documents/src/flaminio-16. HEAD 46cb777 adds only report.md. Cross-plan: #12 (flaminio-12 @314e82f), #74 (/tmp/flaminio-plans-2026-11-12/74), #43 (flaminio-43 and the cached issue).

## Earlier findings

- R1-C1 (High): fixed. The design disables resumption in all four rustls configurations and both Go server configurations. No Go client sets a `ClientSessionCache`: none in the repository, in etcd's client/server/pkg 3.7.3, or in grpc 1.85's credentials. There is no steady-state cost: the node keeps one lazy channel per endpoint (`lib.rs:106-108`), `call_blocking` is for tools and tests only (`lib.rs:204`), and replication sessions are long-lived. One test gap remains: R2-C3.
- R1-C2 (Medium): partly fixed. The key keeps its owner, group and mode, but the directory does not (R2-B1). The new `.lock` contradicts #12's io-pki contract (R2-C1).
- R1-C3 (Medium): fixed (S15). `flaminio-meta`'s client is untested: R2-C4.
- R1-C4 (Medium): fixed.
- R1-C5 (Medium): fixed. The new Expiry wording is not observable: R2-C2.
- R1-C6 (Low): fixed. Its test cannot fail without the lock: R2-C5.
- R1-C7 (Low): fixed.
- R1-C8 (Low): fixed.
- R1-C9 (Low): fixed. 0013 is used everywhere and the index row says Proposed.
- R1-C10 (Low): fixed. The #43 citation is inconsistent with #43's issue: R2-C6.
- R1-C11 (Low): fixed.
- R1-C Nit 1: fixed.
- R1-C Nit 2: fixed.
- R1-S1 (High): fixed.
- R1-S2 (High): partly fixed. Key modes are kept, directory modes are not: R2-B1.
- R1-S3 (Low): fixed.
- R1-S4 (Low): fixed. The Secret is made by `kubectl create` without `--save-config` (`lab.sh:911-918`), so server-side apply finds no last-applied annotation to keep updating.
- R1-S N1: fixed.
- R1-S N2: fixed.
- R1-S N3: fixed.

### R2-B1 — Medium
**"Keeps what it finds" covers the key, not its directory, so #74's group access to the directory is lost when #74 lands before #12.**
- **The claim.** decisions.md says the rule "needs no change when #74 lands" and holds "in either landing order". design.md says the same of `io.key`.
- **#74's rule.** #74 (approved) makes `/etc/flaminio/pki` 0750, with `io.key` 0640 group `flaminio-io`. It changes `pki.sh:10` to 0750 and `chgrp`s at each `node.sh up`, because `io` lives in `pki` until #12 lands.
- **#16's list** gives `/etc/flaminio/pki` "root 0700" (design.md:131). Its `flaminio-io` 0750 case covers only `io-pki` (:133-134). The design never says whether a directory's owner and mode are applied only at creation or on every run. Today `pki.sh:10` applies them on every run, and `install -d -m 700` resets an existing 0750 directory to 0700 (checked).
- **The effect.** With #74 landed and #12 not, every `csr`/`install` takes the unprivileged I/O process's access to `pki` away. Its watcher then refuses every renewal (not readable) until the next `node.sh up`, and the pair expires: S1 breaks under `PRIVSEP=1`, which is #74's default.

Evidence: decisions.md:71-73; design.md:131-134, :144-145, :370-372; 74/design.md:206, :216; `scripts/guest/pki.sh:10`.

Fix:
- Create a directory with the list's owner and mode only when it is missing, and keep an existing one's, as for keys. Or add the #74-before-#12 case: `pki` is `root:flaminio-io` 0750 while `io` lives there.
- Correct the "no change" claim.
- Script case: an existing `pki`/`io-pki` at 0750 group G is unchanged after `csr` and `install`.

### R2-C1 — Medium
**`csr`/`install` leave a `.lock` in `io-pki`, against #12's rule that `io-pki` holds only its four files.**
- **#16's design.** It locks "the directory's `.lock`" (design.md:137-138), so `/etc/flaminio/io-pki/.lock` stays for good.
- **#12 (review-clear).** `io-pki` "holds only the four files `--pki` reads", and its script test asserts that after `cmd_pki issue`, `io-pki` "holds exactly `io.crt`, `io.key`, `ca.crt` and `token.pub`".
- **The effect.** Whichever lands second fails #12's test or breaks its contract, and neither plan says which yields. The directory is mounted whole into the pod.

Evidence: design.md:137-138, :143; 12/design.md:131-133, :750-755; 12/decisions.md:402.

Fix: keep the lock outside the mounted directories, for example one `/run/lock/flaminio-pki.lock` (or `/etc/flaminio/.pki.lock`) for every name, taken by `csr` and `install`. Say that #12's four-file test then still holds. The `.new` files must stay beside their targets for the rename, and are gone after a completed run.

### R2-C2 — Low
**The Expiry check cannot see "its metadata calls are refused (… the service's logs)".**
- **The service logs nothing.** grpc-go logs a failed server handshake at INFO (`grpc@v1.85.0/server.go:1061`, `channelz.Info`), and its default level is ERROR (`grpclog/loggerv2.go:67`). Flaminio sets neither grpclog nor `GRPC_GO_LOG_SEVERITY_LEVEL`.
- **No new handshake happens.** Neither the service nor the Rust client sets HTTP/2 keepalives (no `keepalive` in meta/control/pkg; `lib.rs:85-95`). So fl-c's open HTTP/2 connection survives a 30 s iptables DROP: TCP retransmits, and the per-call timeout does not close the connection. When the partition heals, fl-c's heartbeats go on over the old session with the expired pair.

The peers' refusal of new replication sessions is observable (`flaminio-node: …: session to fl-c failed`, engine.rs:1772).

Evidence: design.md:314-317.

Fix: drop "the service's logs". Prove the metadata refusal with a new connection from fl-c, for example `metactl` with `AS=io` (fl-c's expired `io` pair): refused, naming the expiry. Or drop the metadata clause and keep the replication one.

### R2-C3 — Low
**The resumption tests cannot fail when only one side's setting is missing.**
- **The flaminio-net tests.** `reconnect_after_renewal_is_full` and `reconnect_after_expiry_refused` use the same `Pair` configurations on both ends, so "both fail with rustls's default resumption" holds only when both ends are default. If an implementer sets only the server's `send_tls13_tickets = 0`, the client cannot resume either, and both tests pass while `Resumption::disabled()` is missing. The reverse holds too.
- **The metaclient test.** Its loopback server's configuration is unspecified. If it is `Pair::server_config()`, it cannot see the metaclient's client setting.
- **What is promised.** The ADR and Contracts promise the setting on every configuration, each side.

Evidence: design.md:254-262; ADR 0013 Decision.

Fix:
- Test each side against a peer that resumes: `Pair::server_config()` with a rustls default `ClientConfig`, and `Pair::client_config()` with a default `ServerConfig` (tickets on). Each reconnection is `HandshakeKind::Full`.
- The metaclient test's server keeps rustls's default tickets.

### R2-C4 — Low
**S15 names `flaminio-meta`'s client, but no test covers it.**
- The S15 tests are `TestVerifyMeta` (pkg), `TestDialMetaChecksMeta` (csi) and the Rust test. Nothing fails if `main.go:333` forgets `VerifyConnection`: the interop's admin calls still succeed.
- `meta/cmd/flaminio-meta/main_test.go` already runs the client against an mTLS service fixture (`newFixture`, `f.run`), yet Files touched does not list that file.

Evidence: spec.md:121-129, Tests (S15); design.md:85-86, :193-195, :244-251.

Fix: add `TestClientChecksMeta`. The fixture's server gets an `io` certificate at 127.0.0.1, and `f.run(admin, "nodes")` fails naming the identity. With the `meta` server, it is answered. Add the file to Files touched.

### R2-C5 — Low
**The planned `pki.sh` tests cannot prove the lock, or the owner and group.**
- **The lock.** `flock` is stubbed, and the "interleaved" runs are whole commands in sequence. They pass without any lock, yet the lock is what prevents an `install` A/B step interleaving from producing key B with certificate A.
- **Owner and group.** As a non-root user with `sudo` as `exec "$@"`, the test can neither create an `io.key` "found 0640 group `flaminio-io`" nor `chown root:flaminio-meta-pki` (no such group on the host).
- **macOS.** `chown`/`chmod --reference` are GNU-only, and these script tests also run on macOS (`node-scripts-test.sh:12-13`). #12 checks "modes only" for this reason.

Evidence: design.md:142-146, :275-282.

Fix:
- The `flock` stub records its arguments. Assert that `csr` and `install` run under the lock, and that `install`'s check, chmod/chown and renames all happen inside the locked command.
- Stub `chown`/`chgrp` (recording their arguments) and assert the recorded owner and group. Check modes for real.
- Use a portable way to read the replaced key's mode, or guard the case to GNU.

### R2-C6 — Low
**The #43 citation contradicts #43's issue, so nobody clearly builds CSR issuance.**
- **#16's text.** It says #43's step 2 is "CA bootstrap, and issuing on CSR (#16 step 1)" and that "#43 already covers it" (decisions.md:118-119; spec.md:113-115, :175-176).
- **#43's issue** says: "2. CA bootstrap: the operator holds the internal CA. Issuing on CSR is #16." #43's plan puts the same work out of scope as "(#16 step 1)", and leaves fl-c's first certificate to "#16's question 2" (43/intent.md:80-81).
- **The effect.** Each issue points to the other for issuing on CSR, the TokenReview and question 2's rule. #16 closes only "when the operator's CA issues" (decisions 5).

Fix: quote #43's issue as written. State that #43's step 2 bootstraps the CA, and that issuing on CSR (with its TokenReview and question 2's answer) stays #16's step 1, built once that CA exists. Say this in S14, Open questions 1, design Docs and decisions.

### R2-S1 — Low
**The crash window is wider than "a power cut between the renames".**
- **No fsync.** No `.new` file is synced before its rename, and neither directory is synced after the renames and the old layout's `rm -f`.
- **The first-key case.** For a first key in a new directory (the migration of `meta`, `meta-etcd`, `agent`, and of `io` into `io-pki`), nothing is replaced, so ext4's rename-over flush does not apply. A power cut within the writeback delay can leave empty new files after the old files' removal is already journaled.
- **The effect.** After the reboot every unit restarts, and S7 refuses them until the next pass (`due`: not readable).

Evidence: design.md:146-151, :379-384.

Fix: `sync` the two `.new` files before step 3, and `sync -f` both directories after step 4. Or state the wider window in Risks.

## Nits

1. spec.md:127-128: "the Go clients, which have no such flag". `flaminio-meta` has `--trust-domain` (`main.go:310`, used by confirm only). Say that its check uses its certificate's trust domain, not that flag.
2. design.md:154: `due`'s "a pair still only in the old layout (`/etc/flaminio/pki`)" must be limited to the names that moved, as `install` step 4 is. Read literally, `etcd`, `admin`, `csi-controller` (and `io` before #12) live there and would be renewed on every pass.
3. design.md:174: `RUN` is "the time in seconds and the shell's pid". Say they are concatenated: a separator (`-`, `.`) fails `^[0-9a-z]{1,32}$` (:136), and the faked `run_on_vm` test would not catch it.
4. ADR 0013:34-35: "Servers keep no session and send no ticket" also reads as covering etcd, which the ADR says "already fits" but which sends Go's default tickets (no `SessionTicketsDisabled` in etcd 3.7.3). Say "Flaminio's servers"; no etcd client resumes.

Reviewed commit: 46cb7776c594d54c808a9080ea6b94a7ffc5a97f (plan content identical to 7eaf4b7)
