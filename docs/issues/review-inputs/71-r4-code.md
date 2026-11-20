# Plan review: issue #71, round 4 (incremental), design-code

Diff 215748e..b18e199, checked against /tmp/flaminio-plans-2026-11-12/71.

## Round-3 findings
- R3-1: fixed (S3 and Cost state the bound; campaign limit 10% on median p99).
- R3-2: fixed (table and S4 follow #69's whole-view failure; `TestViewOutcomes` row matches #69 design.md:41, 51-52). decisions.md:123-125 keeps the old text with a "Changed" note: acceptable.
- R3-3: mostly fixed (chmod, dev/ino, control.rs unchanged); see R4-2 for gaps in the new list.
- R3-4: fixed (cites main.go:7, the argument order there is right).
- R3-5: partly fixed. Own target and job, but the pin is still open (R4-5).
- R3-S1 (the observer cert): fixed in substance. Verified: Go `ClientConfig` (pkg/pki/pki.go:300-311) sets no ServerName, so gRPC verifies against the dial host (IP -> IP SAN only); Rust `MetaClient` (flaminio-metaclient/src/lib.rs:85-92) uses `domain_name(host)` (IP string -> IP SAN); replicas use `ServerName::IpAddress` (flaminio-net/src/session.rs:195). A DNS-SAN-only cert fails all three. `flaminio-ca sign --dns` exists (control/cmd/flaminio-ca/main.go:117) and `pki.issue` allows a cert with no IP.

## Findings

### R4-1 Medium. A rule's test cannot fail: `prometheus-example-test.sh`
Contracts "Scraping" and S5 say the script "checks every key above and that `insecure_skip_verify` appears nowhere", but the design gives no method (no yq or python is named in scripts/tools, which are plain bash) and no negative fixture. A grep over the very file it guards passes if a key is misspelled elsewhere or nested under the wrong job. Also the Makefile `lint` recipe (Makefile:92-94 lists the script tests) must gain a line; design "Files touched" names only `rules-test` and `PROMTOOL` for the Makefile. Existing scripts/tools tests carry a bash-version guard and a header (node-scripts-test.sh:1-9).
Fix: say how it parses (a bash/awk per-job block, or promtool-independent Go test in `deploy/observe`), give it mutated copies (missing `sample_limit` on one job, `insecure_skip_verify: true`, wrong `server_name`) that must fail, and list the Makefile `lint` line.

### R4-2 Low. Directory steps: order and test gaps
(a) Step 4 compares D with the control socket's directory by `(st_dev, st_ino)`, but that directory is created by `control::spawn` (control.rs:41-43, `create_dir_all`), and the contract does not say telemetry starts after it, or what happens when `--control`'s parent does not exist (an lstat error is not a REASON in the list). State the order or "absent: no conflict". The path is `--control` (node.rs:177), not a fixed `/run/flaminio`.
(b) `refuses_an_unsafe_directory` lists "the control socket's directory reached through a symlink"; a symlink D fails step 3 (lstat, not a directory), so step 4 is never reached by that case. Step 4 is reachable only through the same inode (a `..` path or bind mount). Use a `/x/../control-dir` form and a bind or hard-linked-equivalent case, or say the symlink case tests step 3.
(c) Step 1 requires the parent owned by the daemon's euid; `mode_and_group` and the group cases need a group the test user belongs to when not root. Say the unit tests use the process's own gid.

### R4-3 Low. Stale: decisions.md:114-116
"Prometheus checks only the CA and the IP SAN" now contradicts contracts (observer job checks the DNS name `observer.flaminio.invalid`, the meta job the IP SAN) and design "Risks". The R3 rewrite of the residual missed this line. Fix: "checks the CA and the scrape address's job name only".

### R4-4 Low. The Rust test's setup is unspecified
design.md:150 says meta-interop.sh gets "a server presenting an observer certificate", but not how: a second `flaminio-meta serve` on another port with `--tls-cert` the observer certificate, and a new env var for `interop.rs` (it reads only `FLAMINIO_META_INTEROP` and `_DIR`, meta-interop.sh:44-46). Also `meta-interop` is not in `make test`, only in `contracts` and `check`; S5's row says so only by naming the target. Fix: name the second server, the env var, and the expected `ClientError`.

### R4-5 Low. "make lint stays offline" is not a reason, and the pin is open
Lint already runs `$(BUF)`, a `go run` of a pinned module (Makefile:13), so lint is not offline; the own-job split is still fine for size. design.md:256 and decisions leave promtool's release "chosen when rules-test is written", which was R3-5's request to settle. Fix: record the version and that it builds with the repo's Go (or install a release binary in the job), and reword the rationale. Also add the `rules` job to docs/ci.md's table (design names ci.md for `rules-test`; the table at ci.md:13 lists jobs).

## Nits
- adr-proposal.md keeps its "proposal" file name after acceptance; fine until moved, but say so in the Status line.
- contracts "Observer certificate" refuses clients dialing `observer.flaminio.invalid` only by convention (`.invalid`); a metadata endpoint configured by that host name would accept it. Say metadata endpoints are IPs in the lab and unchecked otherwise.
