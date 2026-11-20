# Issue #16 plan review, round 1, code focus

Scope: docs/issues/16/ (intent, spec, design, decisions) and docs/adr/0007-certificates-rotate-by-file-replacement.md, at the HEAD of issue/16-short-lived-certificates. On origin/main only `dataplane/Cargo.lock` differs (crc32c). The scripts are as the plan describes them.

### R1-C1 — High
**TLS session resumption skips the new pair and the expiry check on every Rust link, so S2, S5 and S6 fail and the design's own tests cannot pass.**
The design keeps rustls's defaults. A rustls server keeps 256 sessions and sends 2 TLS 1.3 tickets per handshake. A rustls client caches tickets for each server name (an IP counts). On a resumed handshake neither side sends a certificate. The server copies the stored `client_cert_chain` into `peer_certificates`, and `can_resume` checks only the suite, EMS and SNI, never `NotAfter`. Each resumed handshake then issues new tickets that carry the same old chain, valid for 24 h each. So:
- a replication or metadata client that reconnects after a renewal presents nothing new (S2);
- a node whose certificate expired keeps getting replication sessions accepted by its rustls peers, chain after chain (S5, S6: "refused, by the Go and the Rust sides alike");
- the lab's Expiry check ("after a partition they are refused") would fail.

Go servers do check the stored leaf's `NotAfter` when resuming, and Go clients cache no sessions. So only the Rust links are affected. `a_session_outlives_a_renewal` ("the next session shows the new serial, each way") would fail on the shared `Arc<ClientConfig>`.

Evidence:
- spec.md:40-41, :55-57, :59-63, :134-136; design.md:43-45, :67-73, :167-168, :202-203.
- rustls 0.23.46: `server/builder.rs:113,123`; `server/tls13.rs:309-317`, `:371-375`, `:1311`, `:1359`; `server/hs.rs:39-58`; `client/builder.rs:172`.
- Go 1.27: `crypto/tls/handshake_server_tls13.go:370`.
- `flaminio-net/src/session.rs:112-113`, `:194-197` (identity read from `peer_certificates`).

Fix:
- Server: in `Pair::server_config`, `session_storage = Arc::new(NoServerSessionStorage {})` and `send_tls13_tickets = 0`.
- Client: in `Pair::client_config`, `resumption = Resumption::disabled()`. Optionally set `SessionTicketsDisabled` in Go's `(*Pair).ServerConfig` too.
- Say all of this in ADR 0007/0013's Decision.
- Tests in `flaminio-net/tests/rotation.rs` and `flaminio-metaclient`, using the same configs throughout. First, a valid handshake, a renewal, then a reconnection: the server sees the new serial. Second, a valid handshake with a pair that expires seconds later, then a reconnection after `NotAfter`: refused.

### R1-C2 — Medium
**#12's `/etc/flaminio/io-pki` (and #74's modes for it) are missing from #16.**
#12 (review-clear) moves the data plane's pair to `/etc/flaminio/io-pki`, with copies of `ca.crt` and `token.pub`, mounted whole into the pod. Whichever of #12 and #16 lands second must route `io` there in #16's per-name `pki.sh`.

#16 still says:
- `io` lives in "the rest in `/etc/flaminio/pki`" (design.md:87-89, :142-143);
- `identities`, `install`'s old-layout removal, the Security note ("the three directories", design.md:240) and the Paths contract never name `io-pki`;
- `file` does not write `ca.crt` and `token.pub` to both directories.

#74 (review-clear) sets `io.key` to 0640, group `flaminio-io`, so its unprivileged I/O process can read it. #74 says "#16's renewal keeps `io.key` 0640". But #16's `csr` makes every key under `umask 077`, root:root (`pki.sh:16`), and `install` `mv -f`s it into place. Under `PRIVSEP=1`, every renewed `io.key` would be unreadable to the I/O process: refused (case 2) until it expires.

Evidence: design.md:87-99, :109-110, :142-143, :240; 12/design.md:131-150; 12/decisions.md:196-208; 74/design.md:206-207, :369; `node.rs:278-282`; `node.sh:46`.

Fix:
- Add `io` → `/etc/flaminio/io-pki` to the map, gated on #12 having landed, or state the "whichever lands second" rule.
- `file ca.crt|token.pub` writes to both directories.
- `install io` removes `pki/io.crt` and `pki/io.key` by name.
- Name `io-pki` in Paths, Security and docs.
- `csr`/`install` give `io.key` 0640 group `flaminio-io` once #74 has landed, so they keep the mode and group they find.
- Script cases for each.

### R1-C3 — Medium
**#74's follow-up "metadata clients check the `meta` URI SAN" has no owner.**
#74 says the follow-up is not closed there because "the key's usages are #16's". #16's intent puts "Which identities exist, their names, SANs and usages" out of scope. The follow-up is neither taken nor handed on.

Today the Rust metadata client checks only the CA and the endpoint's IP (`domain_name(host)`), and so does Go's `ClientConfig`. The `io` certificate carries the node's IP and the server usage, so `io.key` can answer as the metadata service at that address. #16 rewrites exactly this client path (the new rustls connector, `DialMeta`) without saying so.

Evidence: intent.md:87-88; 74/decisions.md:197-199; 74/design.md:333-339; `flaminio-metaclient/src/lib.rs:90-92`; `pkg/pki/pki.go:300-310`; design.md:62, :77-83.

Fix: choose one.
- (a) Take it: the metadata clients (the Rust connector, `DialMeta`, `flaminio-meta`'s client) require the server leaf's single URI SAN to be `spiffe://<td>/flaminio/node/<n>/meta`. Add a rule, a test (an `io` certificate at the right IP refused) and a line in the Contracts.
- (b) Record it in intent's Out of scope and followups §7 with a named owner or issue.

### R1-C4 — Medium
**Two tests cannot fail without the change, or cannot pass with it.**
- **`flaminio-metaclient/tests/rotation.rs`, "two calls around a renewal".** The client keeps one lazy channel per endpoint for its life (`lib.rs:106-108`), so the second call reuses the HTTP/2 connection and records the old serial whatever the change. The spec asks for "a reconnection after a renewal" (spec.md:121). It also meets R1-C1's resumption.
- **`TestEtcdClientAcrossRenewal`, "the next call presents the new serial".** It names no way to see the client's serial at an embedded etcd. Without the change the old, still-valid pair is accepted and the test passes.

Evidence: design.md:161-162, :169-170; spec.md:117-121.

Fix:
- Metaclient: the server closes its connections between the two calls, or the test uses `call_blocking`, which makes a new client each time. Assert the new serial.
- etcd: issue the renewed pair from a second CA that the restarted member alone trusts, or let the old pair expire before the restart, so that only the new pair can succeed.

### R1-C5 — Medium
**The lab plan does not run as written.**
- **The data plane keeps its old binary.** "Before, once: `make pki-issue meta-up csi-deploy`" never deploys or restarts the new data plane (`make deploy`, `node-up` and fl-c's node). The data plane keeps the pair it read at start, and "ten `certificate picked up` per process" fails.
- **fl-c is not migrated.** `make pki-issue` issues for `cluster` (a, b, w) only (Makefile:311; `lab.sh:575`; `lib.sh:44-45`). fl-c's `agent`, `meta` and `meta-etcd` (issued by `pki issue c`, `lab.sh:1228`) stay in `/etc/flaminio/pki`. The new `due` looks only in the new directories and reports them `absent`. `pki renew` skips them silently and they expire, against S10's "on … fl-c".
- **The Expiry check names no partition that reaches fl-c.** `partition-test` cuts fl-a (`lab.sh:803-821`).

Evidence: design.md:95-97, :112, :195-203; spec.md:85-87, :134-136.

Fix:
- Before, once: `make pki-issue`, `make pki-issue VM=c`, `make deploy`, `node-up` (and fl-c's), `meta-up`, `csi-deploy`.
- Have `due` report an old-layout pair as `due`, not `absent`, so a missed migration is renewed rather than skipped.
- Name the Expiry partition: `partition.sh on` on fl-c, held past expiry, then `off`.

### R1-C6 — Low
**`install`'s two renames are neither crash-safe nor safe against a second renewal, and "install refuses the loser's" is wrong.**
- **A second renewal.** Both runs write the same `NAME.key.new`. If B's `csr` lands between A's key check and A's `mv`, a mismatched pair is installed. Otherwise both runs are refused.
- **A crash.** An ssh drop (SIGHUP) between the two `mv`s leaves the new key beside the old certificate.

The Go and Rust processes keep their pair. etcd, which loads at every handshake, refuses every new client and peer connection until the next pass, up to an hour with `EVERY=1h`.

Evidence: design.md:90-93, :248; `listener.go:474`.

Fix: run `csr` and `install` under one `flock` on the directory. Do `install`'s check and both renames in a single `sudo sh -c`, immune to SIGHUP. Use per-run `.new` names. Correct the Risks line.

### R1-C7 — Low
**The old-layout removal ("`NAME.*` in `/etc/flaminio/pki`") would be a glob expanded by the VM user, who cannot list that 0700 directory.**
The glob would match nothing, and `rm -f` would hide the failure. The planned script test's temporary root is readable by the test user, so it would not catch this. #12 found the same thing (R3-N1).

Evidence: design.md:93; `pki.sh:10`; 12/design.md:140-142.

Fix: remove `NAME.crt` and `NAME.key` by name, as root. Make the test's `sudo` stub refuse globs, or make the directory unreadable to the test user.

### R1-C8 — Low
**The half-life rule counts the 5-minute backdating, so short lives are always due.**
`NotBefore` is now − 5 min (`pki.go:124`), so a life L spans L + 5 min. "Due past `NotBefore` plus half the life" becomes due (L − 5 min)/2 after issue. Any `--life` of 5 minutes or less is due at once, and is renewed on every pass. Yet S11 promises lives "in minutes", and the ADR's floor is "installed at least a minute before" expiry. Neither `--life` nor `CERT_LIFE` has a minimum.

Evidence: design.md:96, :120; spec.md:96-97; ADR 0007 Consequences.

Fix: give `--life` a stated minimum (for example 10 min, refused below), or measure the half-life from the issue time (`NotBefore` + 5 min). Add a test case.

### R1-C9 — Low
**ADR number collision.**
0007–0011 are kept by the audit's Proposed ADRs, and #12 holds 0012. By the orchestrator's decision #16's ADR becomes 0013.

Evidence: `docs/adr/0007-certificates-rotate-by-file-replacement.md:1`; design.md:30, :140, :249, :256-258; decisions.md:30-32.

Fix: rename the file to `0013-…` and its title to "0013.", and update every link and mention.

### R1-C10 — Low
**Settled answers 1 and 2 are not carried into the docs plan.**
decisions.md says the operator's CA gets a new issue, and that answer (a) "amends ARCHITECTURE §15.2": first pairs by hand, renewal by the current certificate, re-issue by hand after a long outage, no renewal for a removed node. S14 and design's Docs only add "a renewer replaces files" to §15.2, and keep followups §7's "step 1 and its questions open". No step files the new issue.

Evidence: decisions.md:14-20; spec.md:107-110; design.md:209-215.

Fix: in S14 and Docs, §15.2 records answer 2. Followups §7 records answers 1 and 2 and links the new operator issue. Say who files that issue and when.

### R1-C11 — Low
**The design contradicts itself on ADR acceptance.**
design.md:256-257 says "Approving this design accepts it; its index row comes then". design.md:258 and decisions.md:31-33 say it stays Proposed until accepted in its own step. The design is approved, so the first sentence would mark it Accepted.

Evidence: design.md:256-258; decisions.md:31-33.

Fix: delete the first sentence.

## Nits
1. design.md:3: "Settled choices: decisions (none yet)" is stale; decisions.md now lists six.
2. design.md:198: "ten `certificate picked up` or more per process" includes etcd, which logs no such line. Say "per Flaminio process"; etcd is covered by the `openssl s_client` check on 2379 and 2380.

Reviewed commit: 89a6cc69ce67091be816a81a514fcc05a786086a
