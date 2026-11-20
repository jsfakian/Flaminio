# Issue #16 plan review, round 3 (incremental), design-code and design-security

Diff: 46cb777..34b4ad5 in /home/jsfakian/Documents/src/flaminio-16.

## Round-2 findings

- R2-B1 (Medium): fixed. A directory keeps its mode and group, and the script case covers both landing orders. The spec (S9) does not carry the directory rule (R3-4).
- R2-C1 (Medium): fixed. One lock at `/etc/flaminio/.pki.lock`, outside every mounted directory. "io-pki holds exactly four files after every completed run" is too strong for `csr` alone (R3-5).
- R2-C2 (Low): not fixed. The new replication observation is wrong against the code (R3-1). The metadata observation holds.
- R2-C3 (Low): fixed. The Go and Rust tests each pair one side with a peer that resumes. See R3-6 for the Go client half.
- R2-C4 (Low): fixed in intent. `newFixture` cannot give the server another identity as written (R3-3).
- R2-C5 (Low): partly fixed (R3-2).
- R2-C6 (Low): fixed. #43's step 2 is the CA, and issuing on CSR stays #16's step 1, in spec S14, Open questions 1, design Docs, decisions and intent fact 8. One stale clause in intent Out of scope (Nit 1).
- R2-S1 (Low): fixed for `install`. `file` has no directory sync (R3-7).
- R2-N1 to R2-N4: fixed.

## New findings

### R3-1 — Medium
**The Expiry check's replication observation does not exist in the code.**
- `session to fl-c failed: <reason>` (engine.rs:1770-1772) prints a `&'static str` from `Event::SessionFailed` (target.rs:134). Its values are `"a request timed out"` (target.rs:825), `"a reply did not answer its request"` (:997) and `"a digest reply did not answer its request"` (:1503). The event is never built from a TLS error, so "with the TLS error" is false.
- A refused handshake is logged by `NetEvent::Lost` at engine.rs:828-831 as `flaminio-node: <vol>: session to fl-c lost: <why>`. `<why>` is the `SessionError` from `outgoing_session` (net.rs:294). On the accepting side it is `flaminio-net: session from <peer>: <e>` (net.rs:196).
- As written, the check would match the timeout line, which the partition itself produces, and would pass without proving the expiry.
- Fix: read `session to fl-c lost:` (engine.rs:830) and require the rustls expiry error in `<why>`, or `flaminio-net: session from <peer>` on fl-c's side. Cite engine.rs:828-831, and drop "`failed` at :1772". Confirm the exact rustls text (`invalid peer certificate: Expired`) in the flaminio-net session tests.

### R3-2 — Low
**The `pki.sh` stub list does not cover what the assertions need.**
- The lock case asserts that "the key check, `chown`, `chmod`, `sync`, the renames, the removals" run under the lock. The stubs that record the marker are `flock`, `chown`, `chgrp`, `groupadd`, `sync`, `chmod`, `mv` and `openssl`. `rm`, `find` and `stat` are not stubbed, so the removals and the key check cannot be seen under the lock.
- As a non-root user, the real `install -d -o/-g` (and today's `install -d -m` on `/etc/flaminio`, pki.sh:9-10) either fails or applies the owner unrecorded. The design does not say that directories are made with `mkdir`, then the stubbed `chown` and `chmod`. The case "no `chown`, `chgrp` or `chmod` recorded for an existing directory" holds only if the script uses those commands.
- The design does not say how the script finds a temporary root (`/etc/flaminio` and `.pki.lock` must move under it).
- Fix: add `rm`, `find` and `stat` to the recording stubs, or probe the lock with a real `flock -n` in a wrapper. State that directories are created with `mkdir` plus `chown` and `chmod`. Name the root override.

### R3-3 — Low
**`TestClientChecksMeta`: `newFixture(t)` hard-codes the server identity** (`pki.NodeID(td, "fl-w", "meta")`, main_test.go:74), and the other tests call it with only `t`. The design does not say how the server gets an `io` certificate.
- Fix: say `newFixtureAs(t, serverID)`, with `newFixture` as a wrapper, or a field set before `Serve`. The server certificate must be valid for 127.0.0.1, as the fixture's is. Name the case's assertion text, for example the identity error.

### R3-4 — Low
**Spec S9 lacks the directory rule.** The spec's bullet (spec.md:82) covers only the key ("its directory's default for a first one"). The new rule, that an existing directory keeps its mode and group (the fix for a Medium finding), is in design and decisions only. Neither spec Tests nor the S9 text names it. Add it to S9 and to the spec's test list.

### R3-5 — Low
**"io-pki holds exactly #12's four files after every completed run"** (design.md:139) is false after `csr io`, which leaves `io.key.RUN.new` there until `install` (or step 6, an hour later). The Risks line says so for a dead run. A pod that mounts `io-pki` also sees the stale key (root 0600, unreadable under #74's group).
- Fix: word it as "after every `install` and `file`", as the test does. Optionally make `csr` write to the lock's directory or a root-only staging directory outside `io-pki` and move it in `install`, but then `rename` crosses directories, so the current approach is acceptable if the wording is exact.

### R3-6 — Low
**`TestNoResumption`'s Go-client half cannot fail when its side's setting "goes".** A Go client with no `ClientSessionCache` cannot resume whatever `(*Pair).ClientConfig()` does. The half only fails if someone adds a cache, so "each half fails if its side's setting goes" is untrue for it. Say the half guards against a cache being added, or drop the claim.

### R3-7 — Low
**`file` syncs the temporary file but not the directory.** A first `ca.crt` or `token.pub` in a new directory (`io-pki`, `meta-pki`) replaces nothing, so it is the same window as R2-S1 and can come back empty or missing after a power cut. Add `sync -f DIR` after the `mv`, or state it in Risks.

### Checked, no defect
- `stat -f '%u %g %Lp'` is correct BSD syntax for numeric owner, group and permission bits (`%Lp` omits the file-type bits). `stat -c` first is the right order, because GNU `stat -f` succeeds with another meaning.
- Order is `sync` of the files, the renames, the removals, then `sync -f` of the name's directory and of `pki`. The renames are journalled before the removals, so a crash between steps cannot lose both layouts.
- The `metactl AS=io` observation is sound: Go's server sends `alertCertificateExpired` (handshake_server.go:987 in the local Go), and TLS 1.3 surfaces it on the first read, as `remote error: tls: expired certificate`. The client does not check its own certificate's expiry at load (S4/S7 start with an expired pair). #12's design (:144) keeps `metactl` with `AS=io` working after `io` moves.

## Nits
1. intent.md Out of scope (line 85) still says the open questions record "an issue for the operator". #43 exists and the spec says no new issue is filed.
