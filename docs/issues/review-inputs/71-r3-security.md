# Issue #71 plan review — round 3 (incremental), design-security

Scope: `git diff 6a9d1a3..215748e -- docs/issues/71/ ':!docs/issues/71/report.md'` (round-2 fix
e6fc50a, approval 215748e), checked against the worktree `/tmp/flaminio-plans-2026-11-12/71` and
#74's draft (`/tmp/flaminio-plans-2026-11-12/74`, 96d792d).

## Earlier findings
- R2-S1: fixed as asked (server-only issuance with the IP SAN, `/etc/flaminio/observer`, the
  scrape TLS rules, `TestStartupRefusesIdentity`, lab step 1). The recorded residual understates
  what that key can do: R3-S1. The key's group is not stated: R3-S4.
- R2-S2: fixed for the telemetry directory itself. Its parent, the umask and the path comparison
  are not covered: R3-S3.
- R2-S3: fixed (job selectors, `honor_labels: false`, the promtool case). The config that the
  guarantee rests on is untested: R3-S5.
- R2-S4: fixed (evidence and interface grammars, `TestObserverStringsChecked`). Two small gaps
  remain: Nits 1 and 2.
- R2-S5: fixed (one pending `Observe`, `one_observe_pending_at_most`).
- R2-S nit: fixed (`volumes_recovering` counts only the target's observer).

### R3-S1 — High
**The observer's key passes every metadata client's server check, not only Prometheus's. The
plan gives the consequence as a hidden telemetry alert.**
- The revision issues `observer` with `--server --ip <node IP>` and copies its key to
  `/etc/flaminio/observer`, readable by the non-root, network-facing observer (contracts
  "Observer files"). Until now, every server key for a node's IP was root-only in
  `/etc/flaminio/pki` (`scripts/guest/pki.sh:10`, `scripts/lab.sh:579–584`).
- Metadata clients check only the CA and the dialled host:
  - Go: `pki.Files.ClientConfig` sets only `RootCAs` (`pkg/pki/pki.go:300–311`), with no URI SAN
    check anywhere (no `VerifyConnection` or `VerifyPeerCertificate` in the tree). CSI uses it
    (`control/csi/meta.go:33`), and so does metactl (`meta/cmd/flaminio-meta/main.go:333`).
  - Rust: the data plane's `MetaClient` checks CA and `domain_name(host)` only
    (`flaminio-metaclient/src/lib.rs:85–92`).
  - Only the replica path checks the role (`flaminio-net/src/session.rs:198`).
- Metadata listens on that same node IP, on port 7400, which needs no privilege
  (`scripts/lab.conf:99`, `scripts/lab.sh:518`). Every daemon's `--meta` lists every instance
  (`lab.sh:671–675`).
- So, whenever the metadata instance on its node is down, a compromised observer can bind
  `IP:7400` and act as the metadata service. It can answer a target's `ChangeIsr` with
  `applied: true`, which the engine takes as committed (`flaminio-node/src/engine.rs:1536`). The
  target then acknowledges writes without a replica that the real metadata still counts in-sync.
  That breaks the rule that shrinks count only once committed (`docs/replication-protocol.md:104`),
  and with it the acknowledgement rule.
- Even within Prometheus, the stated consequence is incomplete. A forged metadata job can also
  raise a false critical `FlaminioLostRedundancy` for any volume label: the rule is a `max`.
- The plan says otherwise in three places: "Observation grants nothing" and "The observer cannot
  call the metadata service" (design "Risks"), and "so it is no client of..." (adr-proposal.md:24).
  The residual names only the metadata metrics job (design.md:231–235; decisions.md:104–107).

Fix:
- Issue the observer certificate without an IP SAN, with a role DNS SAN that no metadata client
  dials (for example `observer.flaminio.invalid`). Have the CLI set `ServerName` to it, keeping
  the URI SAN check, and set the observer job's `server_name` to it. Then state that Prometheus
  binds a series to its node by scrape address only.
- Alternatively, make #71 depend on metadata clients checking the server's `meta` URI SAN, in Go
  and Rust, before the observer key leaves `/etc/flaminio/pki`.
- Either way, rewrite the residual in design "Risks", the ADR proposal and decisions.
- Add a test that a `pki.Files.ClientConfig` gRPC dial and a `MetaClient` both refuse a server
  presenting the observer certificate.

### R3-S2 — Medium
**The `--privsep` dependency is deferred in both directions, so no plan specifies it.**
- #71 says #74's root helper "does all this and passes the listening socket to the I/O process"
  (contracts.md:21–23; design.md:243–244; intent.md:48–50).
- #74 says the opposite: "this step adds nothing for it" (74 design.md:228–230), and "#71's
  telemetry socket is #71's dependency on this step … no mechanism added here" (74
  decisions.md:68–69). Its helper takes no telemetry flag (74 design.md:134) and has no request
  for that socket (protocol table, 74 design.md:141–146).
- #74's `--privsep` parse refuses `--control` but not `--telemetry` (74 design.md:75). The I/O
  process would then run #71's setup as a non-root uid. It cannot `mkdir` in `/run`, and the
  owner check refuses a root-owned D.
- #12's listed helper capabilities hold no `CAP_CHOWN` for the `lchown` to GID (74
  design.md:224–225).

Fix:
- State in #71 that `--privsep --telemetry` exits 1 (`telemetry needs direct mode`) until the
  helper supports it, and test that.
- Record the helper's part as a named follow-up with one owner, named the same way in both
  plans: the flags, a `telemetry` request, the directory rules run by the helper, and how the
  group is set without `CAP_CHOWN` (for example `setegid` before `mkdir` and `bind`).

### R3-S3 — Low
**The directory rules protect the socket inside D, but not D itself.**
- Nothing checks D's parent. If the parent is writable by a non-root user (a misconfigured PATH
  or hostPath, without the sticky bit), D can be swapped for a symlink in three windows: after
  `mkdir`, after the `lstat`, or before `bind`. Root's path-following group set on D, its `bind`,
  then the `chmod` 0660 and `lchown` on the socket all land in the attacker's chosen directory.
  "Only D's owner can write D" (contracts.md:21) does not cover this.
- `mkdir(D, 0750)` is masked by the umask. Under umask 077, D is 0700, the group cannot connect,
  and the next start refuses the daemon's own directory.
- "D is the control socket's directory" is unspecified as a comparison. `--control` is
  configurable (`flaminio-io/src/node.rs:82`, `177`), and a string compare misses `.`, `..` or a
  symlinked spelling.
- `refuses_an_unsafe_directory` lacks these cases: a wrong group, a regular file at D, a
  writable parent, and creation under umask 077.

Fix:
- Require D's parent to be root-owned and not group- or world-writable (or sticky).
- After `mkdir`, open D with `O_DIRECTORY|O_NOFOLLOW`, then `fchmod` 0750, `fchown`, and
  `fstat` again.
- Compare D with the control socket's directory by `(st_dev, st_ino)`.
- Add the four test cases.

### R3-S4 — Low
**Who can read the observer's key is not fully stated.**
- Contracts gives `/etc/flaminio/observer` as root:flaminio-observer 0750 and the key as 0440,
  with no owner or group.
- The design runs the observer "as user `flaminio-observer` in group `flaminio-telemetry`"
  (design.md:89–90), with no membership in a `flaminio-observer` group.
- Read literally, the observer cannot open its key. The obvious repair, using
  `flaminio-telemetry`, would give every telemetry-group member the key.
- Lab step 1 checks only `/etc/flaminio/pki` and `node.sock`. It does not check that another
  telemetry-group member cannot read the key, or that the observer cannot write D or read the
  tokens in `/run/flaminio`, as "Who must not" claims (design.md:226–228).

Fix:
- State the key as root:flaminio-observer 0440. Make the observer's primary group
  `flaminio-observer`, with it as the only member, and `flaminio-telemetry` supplementary.
- Add these lab assertions: a second telemetry-group user cannot read the key, and the observer
  cannot create a file in D or read a `/run/flaminio/*.token`.

### R3-S5 — Low
**The scrape example bounds only the observer job, and nothing tests its security keys.**
- The plan allows a forged `flaminio-meta` target (R3-S1, design "Risks"). That job has no
  `sample_limit`. Neither job sets `label_value_length_limit`, `label_limit` or
  `body_size_limit`, so one target can grow Prometheus's memory without bound (contracts
  "Scraping").
- The promise "a series of one job never feeds another job's rule" rests on `honor_labels:
  false`. TLS verification rests on having no `insecure_skip_verify` or `server_name`.
- The promtool case feeds series that are already labelled, so it cannot catch an edit to
  either. No test reads `prometheus.example.yaml`.

Fix:
- Give `flaminio-meta` a `sample_limit` from #69's bound (4V + 2N + 2), and give both jobs label
  and body limits (ids are at most 128 bytes).
- Add a script test, run by `make lint`, that asserts these keys and `honor_labels: false`, and
  that no `insecure_skip_verify` appears.

## Nits
1. The kernel evidence grammar has no value for an `osrelease` outside `[0-9A-Za-z._+-]{1,64}`
   (contracts.md:163). The rule that made it `unknown` was removed, so an honest observer either
   lies (`release unreadable`) or sends text that makes the CLI mark the whole node `invalid`.
   Add `release unparsable`.
2. The CLI prints the observer's `checked_at` (contracts.md:205). The output rule
   (contracts.md:210–211) lists neither grammar-checked evidence nor timestamps, and
   `TestObserverStringsChecked` covers only `evidence` and `interface`. Say that T is parsed and
   formatted again, and add a T with control bytes to the test.
3. adr-proposal.md:3 now says "Accepted with the plan". Its title still says "Proposed:", its
   section is "Decisions proposed", and contracts.md:3–4 calls every format "a [Proposed]
   choice". Make the status one thing.

Reviewed commit: 215748eaab772b53f1b3fe3f539b772ca4b59494
