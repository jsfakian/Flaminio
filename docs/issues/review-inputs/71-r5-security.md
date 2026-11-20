# Issue #71 plan review: round 5 (incremental), design-security

Scope: `git diff b18e199..58ad388 -- docs/issues/71/ ':!docs/issues/71/report.md'`, checked
against the worktree `/tmp/flaminio-plans-2026-11-12/71` (HEAD 9dea4a9, which only adds
report.md). The cross-plan points are checked against #74's plan at 2040d58 (its round-3 fix,
committed during this review).

## Earlier findings
- R4-S1: mostly fixed. The parse refusal, its message and `telemetry_refused_under_privsep` are
  added, `node.sh` dies, and "unavailable" is a separate state. #74's 2040d58 now uses the same
  wording and carries the "whichever lands second" work (74 design.md:137–139, 178–181, 262,
  331–333; 74 decisions.md:149–152). Still open: the new state comes from the observer's own
  start flag, not from the daemon's mode (R5-S1).
- R4-S2: fixed. The reach, the free-port case and the alerts are stated, and the residual is
  marked open. Two wording defects remain (Nits 1–2).
- R4-S3: mostly fixed. Every ancestor is checked, the order after `control::spawn` is stated
  (`ublk.rs:771`; the directory is created at `control.rs:46–47`), and the owner cases are
  added. Ancestors reached through a symlink are not specified (R5-S5), and decisions keeps a
  wrong summary (Nit 3).
- R4-S4: fixed for the two vacuous assertions: the probe token (`node.sh:61–62` verified) and the
  `getent passwd` check. The third bullet is resolved by moving the user and group checks to
  the lab check. That check tests the account, not the process (R5-S4).
- R4-S5: fixed for `/etc/flaminio/pki` and for the metadata clients. #74 now records the `io`
  key with the follow-up "metadata clients check the `meta` URI SAN" (74 design.md:301–306,
  decisions.md:164–166). That follow-up does not cover the Prometheus metadata job, and #71's
  new Risks text says that job is unaffected (R5-S3).
- R4-S Nit 1: fixed (decisions.md:114–119 annotated).
- R4-1, R4-2, R4-3, R4-4 (code focus): fixed. `sigs.k8s.io/yaml` is at `go.mod:70`, indirect.
  `call_blocking` returns `Answer::Failed` on a failed connection (`flaminio-metaclient/src/lib.rs:205–216`).
- R4-5: fixed apart from the promtool release, which is a recorded open question.
- R4-C Nits 1–2: fixed.

### R5-S1 — Medium
**"Unavailable" is set by the observer's start flag, not by the daemon's mode, so the two drift
apart in ordinary lab use. Each drift either brings back R4-S1's permanent alert or silences
loss detection.**
- `observer.sh` passes `--no-telemetry` "when `PRIVSEP=1`" (contracts.md:43–44). Under #74,
  `node.sh` defaults `PRIVSEP` to 1, or to 0 with `ZERO_COPY=1` (74 design.md:178). With
  `PRIVSEP` unset, the default lab case, the observer therefore runs in telemetry mode while the
  daemon runs privsep. Its snapshot is never fresh, so `FlaminioTelemetryLost` fires on every
  node all the time. This is R4-S1's consequence, now reached by default. #74's plan does not
  mention `observer.sh`.
- The lab restarts nodes into the other mode as a matter of course. `bench_reads` restarts a
  node whose flags differ (`scripts/lab.sh:1495–1513`), and #74 adds `--privsep` to the compared
  flags (74 design.md:190–191). `observe.sh` restarts every daemon in direct mode, and its trap
  "restarts the daemons with their previous settings", but "the observers … stay"
  (design.md:190–191, 216–219). After the check, every privsep node keeps a telemetry-mode
  observer and alerts.
- The opposite drift hides data. A direct-mode daemon with `TELEMETRY=1` next to a
  `--no-telemetry` observer exports no disk or recovery series, and `FlaminioTelemetryLost`
  never fires. The node is silently unmonitored, which contradicts S8: "Missing or stale data
  raises lost telemetry".
- Nothing verifies `--no-telemetry` against the daemon, and no test covers either drift.

Fix:
- Take the observer's mode from the one value `node.sh` resolved. For example, `node.sh`
  (re)starts the observer whenever it starts the daemon, with `--no-telemetry` exactly when it
  passed `--privsep`.
- Have `observe.sh`'s trap restore each observer's previous mode.
- Add `node-scripts-test.sh` cases: unset `PRIVSEP`, `ZERO_COPY=1`, and `PRIVSEP=0 TELEMETRY=1`
  each give the matching observer flag.
- See also R5-S2.

### R5-S2 — Low
**`FlaminioTelemetryLost` is silenced by a missing `flaminio_observer_snapshot_fresh`, not by
the explicit `flaminio_observer_telemetry_available 0`.**
- The expression is unchanged and never reads the new gauge (contracts.md:291). Any observer
  target with `up` 1 and no `snapshot_fresh` series fires nothing, whether that comes from a
  bug, a version skew or a forger.
- The design's "(also by reporting `flaminio_observer_telemetry_available 0`)" (design.md:259–260)
  implies that the gauge is what hides the alert. It is not.
- The only new promtool case is available 0 with no snapshot (contracts.md:296–297). That case
  passes whether or not the rule reads the gauge.

Fix:
- Add the term `up{job="flaminio-observer"} == 1 unless on(instance)
  (flaminio_observer_snapshot_fresh{job="flaminio-observer"} or
  flaminio_observer_telemetry_available{job="flaminio-observer"} == 0)`.
- Add promtool cases that must fire: `up` 1 with available 1 and no `snapshot_fresh`, and `up`
  1 with no series at all.
- Correct the Risks parenthetical.

### R5-S3 — Low
**The new Risks sentence "The metadata job and the CLI are not affected" is wrong for the
metadata job, which accepts the co-located `io` key.**
- The lab issues each VM's `io` certificate as `--server --client --ip "$ip"`, with the same IP
  SAN as that VM's `meta` certificate (`scripts/lab.sh:583–584`).
- The metadata job checks only the CA and the IP SAN (contracts.md:201; design.md:260–262). On
  every metadata host, the `io` key therefore passes that job. It can hide or raise
  `FlaminioLostRedundancy` (critical) for every volume, through the unprivileged port 7402 while
  the listener is down or by redirection.
- Under #74's privsep, `io.key` is 0640 for the non-root, network-facing `flaminio-io` process
  (74 design.md:184).
- #74's follow-up, "metadata clients check the `meta` URI SAN" (74 design.md:301–306), cannot
  close this, because Prometheus does not check URI SANs.

Fix:
- State this residual in design "Risks" beside the observer residual: which key, which alert.
- A simple way to close it: give `meta` a DNS SAN such as `meta.flaminio.invalid` and set that
  `server_name` on the metadata job. `TestPrometheusExample` then checks it. Otherwise record it
  with #74's follow-up.

### R5-S4 — Low
**The lab check, which now carries the observer's user and group checks, tests the account and
not the running observer process.**
- The design says that "Startup checks the certificate; the lab check checks the rest"
  (design.md:91–93; contracts.md:151–152).
- Every assertion in step 1 runs "As `flaminio-observer`" or inspects a file or a group
  (design.md:191–197). An `observer.sh` that starts the observer as root, with capabilities, or
  with extra groups (`disk`, #74's `flaminio-io`) passes all of them.
- The check also never asserts the account's own group list. `getent group` and `getent passwd`
  show who else is in `flaminio-observer`, not which other groups `flaminio-observer` is in. So
  contracts' "the supplementary group `flaminio-telemetry`" (contracts.md:164–165) is not
  checked.

Fix: for each observer unit's MainPID, assert from `/proc/PID/status` that every `Uid:` and
`Gid:` field is `flaminio-observer`'s, that `Groups:` is exactly `flaminio-telemetry`'s GID
(plus the primary), and that `CapEff` and `CapPrm` are 0. Also assert that `id -Gn
flaminio-observer` lists exactly the two groups.

### R5-S5 — Low
**Step 1 does not say how an ancestor that is a symlink is checked.**
- Step 1 checks "every ancestor of D" (contracts.md:15–17) but names neither `stat` nor `lstat`,
  nor whether the path is first resolved.
- With `stat`, a symlinked ancestor's target is checked but the target's own ancestors are not.
  The claim "nobody else can rename or replace an entry on the path to D", and step 5's `chmod`
  argument that rests on it (contracts.md:27–29), then do not follow.
- With `lstat`, every symlink ancestor (mode 0777) is refused with the misleading `ancestor A is
  open to others`. A path through `/var/run` is one example.
- No test has a symlinked ancestor (design.md S3 row).

Fix:
- Say that step 1 walks the given path with `lstat` and refuses a symlink ancestor (REASON
  `ancestor A is a symlink`), or that it resolves the path first and walks the result.
- Add that case to `refuses_an_unsafe_directory`.

### R5-S6 — Low
**The CLI's validation of the new `telemetry` field is unspecified and untested.**
- The list of what the CLI treats as `invalid` (contracts.md:188–192) does not cover:
  - a `telemetry` value outside `available|unavailable`;
  - an `unavailable` reply with `fresh` true, or a non-null `observed_at`, `disk` or `targets`,
    although contracts requires `fresh` false and `observed_at` null (contracts.md:186–187).
- The precedence among `unavailable`, `stale` and fresh data is not stated.
- The S6 tests name no `unavailable` case (design.md S6 row): omitted with R `unavailable`,
  checks still listed, exit 3.

Fix: add these to the invalid list, and add them and the `unavailable` exit and output case to
`TestObserverStringsChecked` and `TestExitCodes`.

## Nits
1. design.md:258–260 says a forger can "hide" `FlaminioTelemetryLost`, while the ADR
   (adr-proposal.md:51–53) says "raise or hide". A forger can also raise it, with
   `snapshot_fresh 0`. Align the two texts.
2. The named way to close the Prometheus residual (design.md:262–263) needs a DNS-safe,
   case-preserving encoding of node ids. Ids allow upper case, `_` and 128 bytes, while TLS name
   matching folds case, so nodes `A` and `a` would share one name. Per-node jobs also need
   `job` relabelled to `flaminio-observer`, or every rule's selector misses them.
3. decisions.md:171–172 says "`chmod` is safe because only root can write the path". Step 1
   allows sticky, world-writable ancestors. Use contracts' "only root can change an entry on
   the path".

Reviewed commit: 9dea4a9c0fe425adee46b8d620e59c0936daf34a
