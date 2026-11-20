# Issue #71 plan review: round 4 (incremental), design-security

Scope: `git diff 215748e..b18e199 -- docs/issues/71/ ':!docs/issues/71/report.md'`, checked
against the worktree `/tmp/flaminio-plans-2026-11-12/71`, #74's plan (`/tmp/flaminio-plans-2026-11-12/74`,
6a9d064) and #69's plan (`flaminio-69`, 11d86de).

How each TLS configuration in the tree checks its peer (this is what decides where the observer
certificate is accepted):
- Go `pki.Files.ServerConfig` (`pkg/pki/pki.go:285–296`): checks the CA and Go's default client
  EKU. The metadata service and both new HTTPS servers use it. A server-only certificate is
  refused as a client.
- Go `pki.Files.ClientConfig` (`pki.go:300–311`): checks the CA and the host gRPC dials. CSI
  (`control/csi/meta.go:33`) and metactl (`meta/cmd/flaminio-meta/main.go:333`) use it, and so
  does the metadata service's etcd client, through the etcd CA (`lab.sh:562`).
- Rust `MetaClient` (`flaminio-metaclient/src/lib.rs:85–92`): checks the CA and `domain_name(host)`.
- Replica client (`flaminio-net/src/session.rs:195–203`): checks `ServerName::IpAddress` and the
  `io` URI SAN.
- Replica server (`flaminio-net/src/tls.rs:71`): `WebPkiClientVerifier`, which checks the client
  EKU.

No configuration skips host verification, and none uses a DNS name that matches
`observer.flaminio.invalid`. So the observer certificate is accepted only by the CLI, which
also checks the URI SAN, and by the Prometheus observer job. The plan's claim holds.

## Earlier findings
- R3-S1: fixed. The certificate has no IP SAN and its startup check refuses one. Go and Rust
  client tests are added, and the residual is rewritten. The new residual understates its
  consequence (R4-S2), and decisions.md keeps a stale line (Nit 1).
- R3-S2: partly fixed. #71 now says telemetry needs direct mode. The refusal is incidental and
  untested, and the helper's part still has no owner (R4-S1).
- R3-S3: mostly fixed. The parent is checked, `chmod` follows `mkdir`, and the control directory
  is compared by device and inode. P's ancestors, the order of step 4 and the owner test cases
  remain open (R4-S3).
- R3-S4: fixed on paper (key root:flaminio-observer 0440, primary and supplementary groups). Two
  of the new lab assertions can pass vacuously (R4-S4).
- R3-S5: fixed. Both jobs have limits: 4·4096 + 2·4096 + 2 = 24578 ≤ 25000, and with `__name__`,
  `job`, `instance` and `node`, at most 7 labels ≤ 10. `prometheus-example-test.sh` is added.
- R3-S Nits 1–3: fixed (`release unparsable`, T parsed and re-formatted with a test case, ADR
  and contracts headers).
- R3-1, R3-2, R3-3, R3-4, R3-5 (code focus): fixed as recorded in the report. R3-1 now has the
  campaign's p99 pass limit. R3-5's promtool release stays a recorded open question.

### R4-S1 — Medium
**`--privsep --telemetry` fails only by accident, nothing tests it, and the helper's part still
has no owner.**
- Contracts says: "Given both flags, the non-root I/O process fails step 1 and exits 1"
  (contracts.md:32–35). That holds only because the lab's P (`/run`) is root-owned. Suppose P is
  owned by #74's `flaminio-io` user (for example its home), and either there is no
  `--telemetry-group` or the user is in GID. Then steps 1–5 pass, and the I/O process serves
  telemetry. This contradicts spec S3, "under #74's `--privsep` it is unavailable".
- When the refusal does happen, the message is `parent /run must be owned by uid <flaminio-io>
  and closed`. That tells the operator to fix the wrong thing.
- No test covers either flag combination (design "Tests", S3 row).
- Neither plan owns the helper's part:
  - #71 says the telemetry socket is unavailable "until #74's helper provides the socket"
    (contracts.md:33–34; intent.md; design "Compatibility").
  - #74 still says "#71's plan owns that" and "no mechanism added here" (74 design.md:287–289;
    74 decisions.md:90–91, 129–130). It also still describes #71's superseded draft.
- #74 makes `PRIVSEP=1` the lab default (74 design.md:153–154). Every privsep node's observer
  would then never be fresh, so `FlaminioTelemetryLost` (contracts.md:266) fires on every such
  node all the time and hides a real telemetry loss. The plan does not say whether `observer.sh`
  starts an observer, or Prometheus scrapes one, on a privsep node.
- `node.sh` "passes `--telemetry` only in direct mode", so it silently drops a `TELEMETRY=1` that
  the operator asked for.

Fix:
- Refuse the pair explicitly at parse: `--telemetry needs direct mode in this step`. Put this in
  `node.rs` beside #74's `privsep_refuses_helper_flags`, whichever issue lands second. Add a
  unit test.
- Have `node.sh` die with `TELEMETRY=1 needs PRIVSEP=0`, as #74 does for `ZERO_COPY`.
- Name the helper's follow-up once, with one owner, in `docs/followups.md` or as an issue, and
  word it the same in both plans. Update #74's two cross-plan notes.
- State that a privsep node runs no observer and is not scraped, or that its
  `FlaminioTelemetryLost` is expected.

### R4-S2 — Low
**The Prometheus residual states the mechanism but not its reach or its consequence.**
- Design "Risks" (design.md:242–245) and contracts.md:184–185 say that any observer certificate
  passes for any observer target, "which takes network redirection to exploit".
- Before round 3, the IP SAN tied each observer key to its own node's address. Now every observer
  shares the one DNS SAN, so one compromised observer's key, readable by a non-root,
  network-facing process, can answer for every node's observer target.
- No redirection is needed when that node's observer is not listening. Port 7412 needs no
  privilege, so any local account on that node can bind it. This includes privsep nodes that run
  no observer (R4-S1).
- The plan does not state the consequence. For any node, the forger can hide or raise
  `FlaminioCapacityPressure` (including the critical level), `FlaminioRecoveryStalled` and that
  node's `FlaminioTelemetryLost`. The CLI is not affected, because it checks the URI SAN.

Fix: state the reach, the free-port case and the alerts affected in design "Risks", the ADR's
"Options" and decisions. If this is to be closed rather than stated, use one DNS SAN per node,
`N.observer.flaminio.invalid`, and one scrape job per node, each with its own `server_name`.
`prometheus-example-test.sh` would then check them.

### R4-S3 — Low
**The directory steps still trust P's ancestors, leave step 4's order open, and omit the owner
cases from the tests.**
- Step 1 checks P alone, with `stat` (contracts.md:14–15). The claim "Only P's owner can replace
  D … so nothing changes in between" (contracts.md:24–25) also needs every ancestor of P to be
  unwritable by anyone else. Otherwise P is renamed and replaced between steps 1 and 2. Steps 2
  and 5 use path-based `chmod`, which follows symlinks, so after `bind`, root's `chmod 0660` can
  land on a file the attacker chooses. This is the R3-S3 race, one level up.
- Step 4 compares D with "the control socket's directory". The design spawns telemetry "beside
  `control::spawn` (`ublk.rs:771`)" without an order. `control::spawn` is what creates that
  directory (`control.rs:46–48`). `/run` is a tmpfs, so if telemetry comes first, the `stat`
  fails at every boot, and the plan does not say what follows.
- Spec S3's tests list a "wrongly owned" directory (spec.md, S3 tests). The design's
  `refuses_an_unsafe_directory` has no wrong-owner D and no wrong-owner P (design.md:168).

Fix:
- Require every ancestor from `/` to P to be root-owned and not group- or world-writable, unless
  it is sticky. Alternatively, work relative to a descriptor: `open(P, O_DIRECTORY)`, `mkdirat`,
  `openat(O_DIRECTORY|O_NOFOLLOW)`, `fchmod`/`fchown`/`fstat`.
- Run the steps after `control::spawn`. A failed `stat` of the control directory exits 1.
- Add the two owner cases, with the expected uid injected so that the tests run unprivileged.

### R4-S4 — Low
**The checks of the observer's user, groups and files can pass vacuously, or are not defined.**
- Lab step 1 proves "only member" with "`getent group flaminio-observer` lists no other member"
  (design.md:184–188). `getent group` lists only supplementary members, so another user whose
  primary GID is `flaminio-observer` can read the key and still passes.
- "Reading any `/run/flaminio/*.token` fail[s]" runs on nodes where the check's precondition is
  that nothing is attached. Token files exist only after a `node.sh` `ATTACH`
  (`scripts/guest/node.sh:61–63`), so there may be nothing to read.
- The design says the observer's "user, groups, files and certificate … are checked at startup"
  (design.md:89–91). Contracts defines only the certificate checks (contracts.md:133–135,
  140–141), and `TestStartupRefusesIdentity` tests only those.

Fix:
- Also assert that no other `getent passwd` entry has that GID.
- Plant a probe token as `node.sh` writes them (`tee`, mode 0644), assert that the observer
  cannot read it, then remove it.
- Either define the startup checks and test them (refuse uid 0, refuse a key readable by others
  or writable by its group), or limit the sentence to the certificate.

### R4-S5 — Low
**"`/etc/flaminio/pki` stays root-only 0700" contradicts #74.**
- #71 says so in contracts.md:147 and decisions.md:115.
- #74 makes that directory 0750 and `io.key` 0640, both group `flaminio-io` (74 design.md:157–158).
  An `observer.sh` that enforces 0700 would break privsep.
- The `io` certificate is `--server --client --ip` (`lab.sh:584`). Under privsep, a non-root,
  network-facing process therefore holds a key that passes the metadata job's IP-SAN check and
  every metadata client (`pki.go:300–311`, `lib.rs:85–92`). This is R3-S1's attack, carried by
  #74's key.

Fix:
- Say that `/etc/flaminio/pki` is "not readable by `flaminio-observer`", which lab step 1 already
  checks, rather than giving a mode.
- Raise the `io`-key path with #74, or as the follow-up "metadata clients check the `meta` URI
  SAN".

## Nits
1. decisions.md:113–116 is marked "changed in round 3: no IP SAN" but still ends "Prometheus checks
   only the CA and the IP SAN". The observer job now checks the DNS SAN. Annotate or correct it.

Reviewed commit: 51cdf542d3f364de75f23e7b06dc580424b157e2
