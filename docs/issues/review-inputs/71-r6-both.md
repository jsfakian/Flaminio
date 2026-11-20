# Plan review: issue #71, round 6 (incremental), design-code and design-security

Scope: `git diff 58ad388..177339f -- docs/issues/71/ ':!docs/issues/71/report.md'`, checked in
`/tmp/flaminio-plans-2026-11-12/71` (HEAD 7a3fcb3, which only adds report.md). Cross-plan points
were checked against #74 at f9732b9 and #16 at 89a6cc6.

## Earlier findings
- R5-1 (Medium): fixed. The state is cut. A stale node whose checks are still listed is in `TestExitCodes`, and a `disk`/`targets` mismatch with `fresh` is in `TestObserverStringsChecked`.
- R5-2 (Medium): fixed in substance. The refusal belongs to the plan that lands second, and #74's S7 row agrees (74 design.md:262). See R6-C1 for the case where #71 lands second.
- R5-3 (Low): fixed. The exit 3 is stated as by design (contracts.md:241–244; spec S6).
- R5-4 (Low): fixed. The `lstat` walk is specified, and the passing cases walk the root-owned `/` (see Nit 2 on `/tmp`).
- R5-5 (Low): fixed. The TCP readiness wait is on the second port, and the trap kills both pids.
- R5-C nit: fixed. The lab runs `TELEMETRY=1 node.sh up`.
- R5-S1 (Medium): fixed. No observer mode is left to drift. `--no-telemetry`, the `telemetry` field, `unavailable`, `flaminio_observer_telemetry_available` and the `PRIVSEP` branch in `observer.sh` are gone from every plan file except the dated decisions history, and the bound is 73 (20+2+26+5+2+16+1+1).
- R5-S2 (Low): fixed. The two `up == 1 unless on(instance) <freshness>` terms are correct PromQL. `or` matches without `__name__`, so they produce no duplicate label sets, and the promtool cases must fire.
- R5-S3 (Low): fixed. A DNS SAN breaks no current verifier:
  - Go `PeerIdentity` counts only URI SANs (pkg/pki/pki.go:249).
  - Rust `from_cert` filters for URI SANs (flaminio-net/src/identity.rs:116–127).
  - `MetaClient` verifies the dialed IP (flaminio-metaclient/src/lib.rs:90–92).
  - `flaminio-ca sign --dns` exists (control/cmd/flaminio-ca/main.go:84).
  - `meta.key` stays root 0600 (scripts/guest/pki.sh:16).

  See R6-B1 for the missing lab test.
- R5-S4 (Low): fixed. `/proc/PID/status` covers the uids, gids, groups, `CapEff` and `CapPrm` of the running process, and `id -Gn` covers the account.
- R5-S5 (Low): fixed. `lstat` is used per component, a symlink is refused, `/var/run` is named, and a test case is added. The lab's `/run` is a root-owned 0755 directory.
- R5-S6 (Low): fixed. The field is gone, and the `fresh` consistency rule is added and tested.
- R5-S nits 1–3: fixed (design.md:265–269; decisions.md:203).

### R6-B1 — Low
**Nothing tests that the `meta` certificate the lab serves carries `meta.flaminio.invalid`. The
check's prerequisites also do not make the metadata service load it.**
- Lab step 2 reaches only the observer by name ("an observer reached as
  `observer.flaminio.invalid` with `--resolve`", design.md:207–208). curl reaches the metadata
  `/metrics` by IP, which the old certificate's IP SAN already satisfies.
- `TestPrometheusExample` checks only the YAML, not the issued certificate.
- The metadata service reads its certificate only when the unit starts
  (`LoadCredential=meta.crt`, scripts/guest/meta.sh:21–22).
- `make pki-issue` (scripts/lab.sh:572–595) and `make deploy` (lab.sh:220) restart nothing. The
  prerequisite "this branch deployed, `make pki-issue` run since" (design.md:191) therefore
  leaves the old certificate in service.
- So a missing `--dns` at lab.sh:583, or a metadata service that was not restarted, passes the
  check. The example's metadata job would still fail every target.

Fix: in step 2, curl the metadata `/metrics` as `meta.flaminio.invalid` with `--resolve`, as the
observer is reached. Make the prerequisite `make pki-issue meta-up`, or have step 1 restart the
metadata service.

### R6-C1 — Low
**If #71 lands after #74, #71's plan names no test for the `--telemetry`/`--privsep` refusal it
must then add.**
- contracts.md:41–45 makes the second plan add the parse refusal and the `node.sh` rule, "each
  with a test". It names only the `node-scripts-test.sh` case.
- The design's S3 row dropped `telemetry_refused_under_privsep` and names nothing in its place.
- #74 names the test (`privsep_refuses_helper_flags`, "`--telemetry` once #71 has landed", 74
  design.md:262), but only for the order where #74 lands second.

Fix: add to the S3 row "if #74 has landed: the `--telemetry` case of
`privsep_refuses_helper_flags` and `node-scripts-test.sh`'s `TELEMETRY=1` case". Add `node.rs`
and `node.sh` to Files touched under the same condition.

### R6-S1 — Low
**Step 1 now walks "PATH as given, from `/`", but PATH is never required to be absolute.**
- contracts.md:8 accepts any `--telemetry PATH`. For a relative PATH, "from `/`" and "as given"
  conflict.
- An implementation that walks only the given components skips every ancestor of the working
  directory. The guarantee that step 5's `chmod` rests on ("only root can change an entry on
  the path", contracts.md:28–30) then does not hold.
- `Path::parent` of a bare file name is `""`, so `--telemetry t.sock` also fails with a
  misleading REASON.

Fix: refuse a relative PATH at parse (exit 1, `--telemetry needs an absolute path`), and add that
case to `refuses_an_unsafe_directory`.

## Nits
1. contracts.md:150–153: "This holds while no metadata endpoint is configured by that name" now
   follows `meta.flaminio.invalid`. It reads as the wrong name; say `observer.flaminio.invalid`.
2. design.md S3 row, "every passing case walks the real `/` and the sticky `/tmp`":
   - The crate's tests take `std::env::temp_dir()` (flaminio-node/tests/engine.rs:315), which
     follows `TMPDIR`.
   - On macOS `/tmp` and `/var` are symlinks, so step 1 would refuse them if `telemetry.rs` were
     not Linux-only, and `make test` runs `cargo test` on the workstation (Makefile:63–64).
   - Say the tests create under `/tmp` explicitly, and that `telemetry.rs` is
     `#[cfg(target_os = "linux")]` like `control` (flaminio-io/src/main.rs:11–12).

Reviewed commit: 7a3fcb3b4f8925112bf7b616185791c9e54755dd
