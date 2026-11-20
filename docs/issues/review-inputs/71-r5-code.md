# Plan review: issue #71, round 5 (incremental), design-code
Diff b18e199..58ad388.

## Round-4 findings
- R4-1 (example script): fixed in substance. `sigs.k8s.io/yaml v1.6.0` is at go.mod:70 as `// indirect`; `UnmarshalStrict` exists in it. design.md "Files touched" lists it as newly direct. Five mutated fixtures are in contracts; the stale script and Makefile lint line are gone (no leftover references). `make test`/`race` run it.
- R4-2 (dir steps): fixed. Ancestors checked; steps run after `control::spawn` (ublk.rs:771); `--control` parse is node.rs:177; missing control dir is a REASON; step-4 case is `T/x/../ctl`; tests inject U and use the process gid. See R5-4 for one small gap.
- R4-3 (decisions.md stale line): fixed (the "Changed in rounds 3 and 4" note).
- R4-4 (Rust interop): fixed. Second memory server, env var `FLAMINIO_META_INTEROP_OBSERVER`, `Answer::Failed` (exists, lib.rs:53). See R5-5.
- R4-5 (promtool pin, ci.md): partly. ci.md table is now listed; the rationale reworded; the release is still "chosen when rules-test is written", a deliberate open choice, not a finding.
- R4-S1 (privsep): fixed on paper; see R5-1, R5-2.

## Findings

### R5-1 Medium. New "unavailable" state has rules without tests
The diff adds `telemetry: "available"|"unavailable"` to `/v1/node`, the `unavailable` omission reason, the observer `--no-telemetry` mode, "doctor still reports an unavailable node's checks", and the lab scripts' `PRIVSEP` handling. Design's test table names only: the series bound (`--no-telemetry`: 1), the promtool case, `TestStartupRefusesIdentity`. Nothing tests: the Go decoder accepting/refusing the new field (golden line and `testdata/invalid` do not mention it), the observer's `/v1/node` under `--no-telemetry` (`fresh` false, `observed_at` null), the CLI listing the node as `unavailable` with exit 3 and still running its doctor checks (S6 row `TestExitCodes` is unchanged), and `fresh`/`observed_at` consistency (an `unavailable` reply with `fresh` true must be `invalid`). The lab check runs direct mode only, so `node.sh` dying with `TELEMETRY=1 needs PRIVSEP=0` and `observer.sh --no-telemetry` never run. Fix: add `TestDecode` cases (unavailable valid; unavailable+fresh invalid), a CLI golden/exit-code case for an unavailable node, a `/v1/node` case, and a script test or lab step for the two `PRIVSEP` branches.

### R5-2 Medium. The privsep refusal is untestable if #71 lands first
`--privsep` does not exist in `flaminio-io` today (no match under dataplane/crates/flaminio-io/src), and neither `node.sh` has `PRIVSEP` (lines 50-66 show none). The design makes `telemetry_refused_under_privsep` and the refusal "added by whichever of #71 and #74 lands second". If #71 lands first, the spec's S3 test line ("`--telemetry` with `--privsep` is refused") and the unit test cannot exist in #71, yet #71's done-when lists it; if #74 lands first, #71 edits #74's flag parser. Also `docs/issues/74/` is absent from this checkout, so the dependency cannot be verified. Fix: state which order the plan assumes (it needs #74's `--privsep` merged first, as it already says for `PRIVSEP=0`), or move the check and its test wholly to #74 and keep #71's spec to "unavailable when privsep is on".

### R5-3 Low. A privsep cluster can never exit 0
Spec S6 lists "telemetry unavailable" as omitted, and exit 3 is "incomplete". With `--privsep` nodes every node-observing command exits 3 permanently, indistinguishable by exit code from a lost observer, and `nodes_observed` is 0. If intended (a conservative choice), say so and say what a script should do; otherwise exclude `unavailable` from the omission count.

### R5-4 Low. Step-1 test cannot reach the "owned by uid 0" arm
Step 1 accepts an ancestor owned by 0 or U, but the unprivileged tests inject U and cannot create a root-owned, group-writable ancestor, so a bug that drops the "0 or" (breaking `/run` in production) or that accepts any owner is not caught for the 0 arm. Also a symlinked ancestor (`/var/run` style) is not discussed: `lstat` per component versus a resolved path. Fix: say which (resolve D's parent with `canonicalize` then check each component), and test with the real `/` and `/tmp`-like sticky ancestor.

### R5-5 Low. meta-interop.sh second server's cleanup
The script's trap kills only `$pid` (meta-interop.sh:7). Add that the second server's pid is killed in the trap and that the readiness wait uses the second port (it uses `admin`, bound to the first). Minor.

## Nits
- design.md:190 "`PRIVSEP=0` once #74's variable exists" and the check's step 1 refer to a variable that is not yet in `scripts/guest/node.sh`; fine, but the lab command it runs today is unspecified.
