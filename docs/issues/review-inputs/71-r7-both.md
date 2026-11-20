# Plan review #71 round 7 (incremental), design-code and design-security
Diff 177339f..e94d7af.

## Round-6 findings
- R6-B1: fixed. meta.sh:21-22 are the meta.crt/meta.key LoadCredential lines, 52 is the restart in `up`; `make meta-up` runs `meta up` on the cluster; step 2 now curls every metadata instance as meta.flaminio.invalid with --resolve.
- R6-C1: fixed. S3 row and Files touched name the #74 case (`privsep_refuses_helper_flags` exists in #74 design.md:277/282) and the node-scripts case.
- R6-S1: fixed in contracts and spec, but see R7-1 (test placement).
- Nit 1: fixed. Nit 2: fixed; main.rs:11-12 is the cfg(linux) `mod control`.

## Findings
### R7-1 Low: the relative-PATH refusal is at parse, but its test sits in a Linux-only walker test
contracts.md:8-10 puts the refusal at parse (exit 1, `--telemetry needs an absolute path`). `node.rs::parse` (node.rs:120) and `mod node` are not cfg-gated (main.rs:13), while telemetry.rs is Linux-only. design.md S3 lists "a relative PATH" inside `refuses_an_unsafe_directory` in telemetry.rs, so the parse rule has no test where the parse lives, and on a non-Linux `make test` it is never run. Fix: name a `node.rs` parse test (e.g. `telemetry_needs_an_absolute_path`, exit-1 message checked) and keep the walker test for the ancestors. Also say the `--telemetry*` flag parse in node.rs stays ungated while `telemetry`/`Attachment` use is `cfg(linux)` (as node.rs:401 does for ublk), so the workstation build compiles.

No other defects in the diff.
