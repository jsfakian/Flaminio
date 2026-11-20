# Issue #16 plan review, round 1, design-security

Scope: docs/issues/16/ (intent, spec, design, decisions) and docs/adr/0007 on
issue/16-short-lived-certificates. Cross-plan: #12 (flaminio-12), #74 (/tmp/flaminio-plans-2026-11-12/74).

### R1-S1 — High
**Rust TLS session resumption lets an expired or pre-rotation identity reconnect, so S5, S6 and the Expiry lab check are false on the replication path.**
The design keeps rustls's defaults. With those defaults the server keeps a stateful session cache and sends TLS 1.3 tickets, and the client resumes. A resumed handshake sends no certificate. The rustls server copies the stored client chain into `peer_certificates()` and checks neither its signature nor its expiry. The rustls client says outright that it does not re-verify. Every resumed handshake also issues new 24 h tickets that carry the same old chain. So a node whose certificate has expired, or one the CA "stops renewing" (decisions 2, the step-1 revocation), keeps opening replication sessions indefinitely, as long as it reconnects within 24 h of its last handshake. `session::accept` then takes the identity from that stale chain. The same applies after a renewal: the reconnecting side resumes, and its peer sees the old serial. That breaks S1/S2 ("every handshake … presents the new pair").
Go is not affected: it re-checks `NotAfter` on resumption both ways. The capability token still gates writes, but the intent's "a short life is the revocation" does not hold for Rust.
None of the planned tests can catch this:
- The Rust expired-peer case starts with an already-expired pair, so its first handshake is always a full one.
- `flaminio-metaclient/tests/rotation.rs` makes "two calls around a renewal" on one kept-open HTTP/2 connection, so it never reconnects.
- `a_session_outlives_a_renewal` would fail as designed, with no mechanism named to fix it.
Evidence:
- rustls 0.23.46 (Cargo.lock): `server/builder.rs:113`, `:123`; `server/tls13.rs:372-375`, `:1311`, `:1359`; `client/builder.rs:172`; `client/hs.rs:1136` (resumes when the verifier and resolver `Arc` are unchanged, which they are after a rotation); `client/tls13.rs:573-581`.
- Flaminio: `flaminio-net/src/tls.rs:68-90` sets none of these; `session.rs:113`, `:197`.
- Go 1.27 for contrast: `crypto/tls/handshake_server_tls13.go:370`, `handshake_client.go:405`.
- Plan: spec S5, S6; intent :83; decisions :109-112; design Rust (:67-83), Tests (:164-170), Lab "Expiry" (:202-203).
Fix:
- In `Pair::server_config()` set `session_storage = Arc::new(NoServerSessionStorage {})` and `send_tls13_tickets = 0`. In `Pair::client_config()` set `resumption = Resumption::disabled()`. This covers replication and the metadata client.
- Add to S6: "no TLS session resumption in the Rust configurations".
- Tests in `rotation.rs`: (a) a peer that connected while valid reconnects after its certificate has expired, with the same configs, and is refused; (b) after a renewal, the reconnection is a full handshake (`handshake_kind() == Full`) showing the new serial.
- The metaclient test forces a reconnection (restart the loopback server) between its two calls.

### R1-S2 — High
**Renewed keys do not keep the mode and group their reader needs, and #16 ignores #12's `io-pki` and #74's `io.key` 0640 `flaminio-io`.**
`csr` always makes `NAME.key.new` under `umask 077` (root:root 0600, `pki.sh:16`), and `install` moves it into place with `mv -f`. Nothing in the plan sets a group or mode before the rename.
- **meta:** the design states "keys 0640" in `meta-pki` but not when or how that is applied. A key moved at 0600 root is unreadable to the `DynamicUser` service. Every renewal is then refused (case 2), the pair expires, and a restart fails (S7).
- **io:** #74 (approved) runs the I/O process as `flaminio-io`, with `io.key` 0640 group `flaminio-io`, and names this as a dependency on #16: "#16's renewal keeps `io.key` 0640" (74/design.md:206, :216, :369).
- **#12:** #12 moves `io` into `/etc/flaminio/io-pki` (12/design.md:131-150; decisions.md:196-207). It says "#16's 'Paths' contract then names `io-pki`, and its renewal keeps `io.key`'s mode and group".
- #16's design names neither plan. Its map keeps `io` in `/etc/flaminio/pki` (design.md:87-89, :142-143), and its Security line counts "three directories" (:240).
- If #16 lands after them, the first renewal makes `io.key` 0600 root. The unprivileged I/O process refuses every renewal and cannot restart, and the pod's `io-pki` copy is never renewed.
Fix:
- Give `pki.sh`'s per-name map each name's directory, directory owner and mode, and key group and mode.
- Apply the group and mode to `NAME.key.new` before the rename (`install -g G -m 0640` or `chgrp`/`chmod`). Use `flaminio-meta-pki` 0640 for `meta`/`meta-etcd`. Once #12 or #74 has landed, use `io` in `io-pki` with `flaminio-io` 0640.
- Add a "#12, #74" bullet stating the whichever-lands-second rule, and update Paths and Security to match.
- Script cases: after `csr` + `install`, the final key's mode and group; `file token.key` never lands in `io-pki`.

### R1-S3 — Low
**"Two renewals at once: `install` refuses the loser's" does not hold without a lock.**
`csr` always writes the same `NAME.key.new`, and `install` compares that file with the certificate and then renames it, in two steps. Interleaving: run A's `install` passes its comparison, then run B's `csr` overwrites `NAME.key.new`, then A renames B's key next to A's certificate. The pair at the final paths no longer matches, and the old key is already gone. etcd (which loads its pair at every handshake) then fails every handshake, and any restart fails (S7), until the next pass. Two runs at once is realistic: the `EVERY` loop plus a manual `make pki-issue`/`pki-renew`.
Evidence: design.md:90-93, :248.
Fix: take `flock` on a lock file in the PKI directory in both `csr` (while it writes `.new`) and `install` (around the comparison and the two renames). Add a script case for the interleaving.

### R1-S4 — Low
**The Secret update stores the controller's private key in a plain annotation.**
`kubectl create secret … --dry-run=client -o yaml | kubectl apply -f -` is a client-side apply. It writes the whole object, `data.tls.key` included, into `kubectl.kubernetes.io/last-applied-configuration`. `kubectl describe secret` and many tools print annotations in full while hiding `data`.
Evidence: design.md:103-104.
Fix: `kubectl apply --server-side --force-conflicts -f -` (managedFields keep field names, not values), or `kubectl replace -f -` once the Secret exists. Add a script case asserting the applied command.

### R1-S5 — Low
**#74's follow-up "metadata clients check the `meta` URI SAN" has no owner.**
#74 says "Not closed here: the key's usages are #16's" and records it as a follow-up. #16's intent puts SANs, usages and caller rules out of scope (intent.md:87-88) and does not mention the follow-up. Yet #16 rebuilds both metadata clients' TLS: Go `Pair.ClientConfig()` and the new tokio-rustls connector. Today they check only the CA and the address (`pki.go:300-309`; `metaclient/src/lib.rs:85-92`). So any `io` key can still answer as the metadata service at its node's IP.
Evidence: 74/decisions.md:196-199; 74/design.md:333-339; 16/intent.md:87-88; 16/design.md:77-83.
Fix: either take it here, as a `VerifyConnection` in Go and a check in the connector requiring the leaf's URI SAN to be a node's `meta`, with tests; or name it in #16's Out of scope as the open follow-up, so that neither plan claims the other owns it.

## Nits

- **N1:** design.md:3 says "Settled choices: decisions (none yet)", but decisions.md now records six settled answers.
- **N2:** design.md:92-93, "then removes the old layout's `NAME.*` in `/etc/flaminio/pki`":
  - A glob there expands as the VM user, who cannot list the 0700 directory, so nothing is removed and the old `meta.key` stays. The faked test root is listable, so the test passes anyway.
  - Read for every name, the same sentence would delete the `etcd`/`io` pairs it has just installed.
  - Fix: say "for `meta`, `meta-etcd` and `agent` only: `sudo rm -f` of the two named files" (as #12 R3-N1).
- **N3:** names crossing the VM boundary are not validated (AGENTS.md "Ids … validated and quoted").
  - `pki.sh` puts NAME into paths and into `sudo sh -c "… '$dir/$name.key'"`.
  - The workstation, which holds the CA keys, parses the VM's `NAME due|fresh|absent` lines.
  - Fix: `pki.sh` refuses a NAME that is not in its map. The renewal acts only on exact matches from `identities VM` and ignores other lines. Add a script case.

Reviewed commit: 89a6cc69ce67091be816a81a514fcc05a786086a
