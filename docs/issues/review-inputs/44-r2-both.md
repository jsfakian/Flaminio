# Plan review: issue #44, round 2 (incremental), design-code and design-security

Reviewed: `git diff 07339ef..0dc56f2 -- docs/issues/44/` in /home/jsfakian/Documents/src/flaminio-44.
Reviewed commit: 0dc56f2 (HEAD da5a08e only adds report.md).

## Round-1 findings

- R1-B1 (High): fixed. The target is on T beside the source replica; `on_mapped`
  (`target.rs:1445`) intersects the plan with the source's mapped extents and discards every
  other block of the plan unconditionally (:1470-1478), so the "12 224 Discards" and the
  digests-are-member-tx reading are right. tx and rx are both read and both differenced; the
  limit stays on rx. The 32 MiB derivation checks: 6.3% of 128 MiB = 8 MiB; 12 224 MiB / 1 MiB
  pieces = 12 224 Discards (0.3 MiB at 25 bytes, under 2 MiB at 150); tx about 5 MiB.
- R1-C1 (Medium): fixed (refusal 3), with a residual window the design states. See L1 on the JSON shape.
- R1-C2 (Medium): fixed. `Event` is matched at `engine.rs:1765-1773` (the `let ... = e` must
  become a `match`, the design says both variants); `set_log` beside `set_arrivals` (:906) fits;
  `start_node` exists at `tests/engine.rs:196`.
- R1-C3 (Medium): fixed. Time is the summary's `elapsed_us`; wall interval reported only.
- R1-C4 (Medium): fixed in the design (evaluate after every view; holds-nothing drops totals)
  and the test table has a row for each case.
- R1-C5 (Low): fixed. Each refusal has a command: `systemctl is-active flaminio-node`
  (`confirm.sh:73`), control `status` (`control.rs:104-107`), `pgrep -x flaminio-csi`
  (`confirm.sh:39`; the controller runs on the witness, `deploy/csi/driver.yaml:32`, so a match
  on fl-a/fl-b is the node plugin's sweep, `control/cmd/flaminio-csi/main.go:96`),
  `metactl nodes` (`main.go:365`).
- R1-C6 (Low): fixed. N1-N5: fixed, except that N5's `make lint` wiring is not in the file list (N1 below).

## Findings of the revision

### L1 - Low: `metactl nodes` omits false booleans; the refusal-3 filter must default them
`flaminio-meta` prints protojson with `EmitUnpopulated: false` (`meta/cmd/flaminio-meta/main.go:501`),
so a draining or dead node has no `draining` / `alive` key. A filter written `.draining == false`
treats a draining node as a candidate (refuses wrongly) or `select(.alive)` is fine but
`select(.draining|not)` on null needs care. The design's test ("a dead, draining or witness one
does not refuse") is right only if the fixture has the real shape. Fix: say the filter uses
`(.alive // false)`, `(.draining // false)`, `(.labels // {} | has("flaminio.io/witness"))` (as
`scripts/lab.sh:614`) and that the fixtures are protojson output with the false fields absent.

### L2 - Low: refusal 4 feeds a remote-supplied id to a shell
V comes from `status` output (`ok V /dev/ublkbN`) of a VM, then goes to `metactl get V`
(`metactl` quotes with `%q`, `scripts/lab.sh:533`, so no injection), but the design says
"validate IDs before command use" only for the inputs. A malformed reply should refuse (not be
skipped, which would let an unknown attached volume through). Fix: pass V through `valid_id`
(`lab.sh:1633`), refuse on a failure or any `status` answer other than `ok none` / `ok V ...`;
one script-test row.

### L3 - Low: cleanup may delete a volume the run did not create
"Always attempt ... delete metadata, delete only this volume's replicas" runs from a trap
installed after the original state is captured, which precedes `create`. If `create` fails on an
existing id (a leftover of an earlier failed run, or a collision), the trap deletes that volume.
The id is unique in practice, but this is the "refuse on anything it does not own" rule.
Fix: arm the metadata delete and the replica delete only after `create` returned success for
that id (and before that, refuse if `metactl get <id>` already answers); one script test: create
fails with AlreadyExists, no delete recorded.

### L4 - Low: the watcher and journal cursor assume T is fl-a
The watcher runs "on fl-a ... (and is T as documented)" and the summary is logged on T's
journal. `T` is a parameter (spec S12, `T=fl-a MEMBER=fl-b`); with T=fl-b the "is T" premise
is false (only the admin key's location holds, `lab.sh:529`) and the cursor must be taken on T,
not fl-a. Fix: say the watcher runs on the admin-key VM, that the journal cursor, summary and
epoch come from T, and add a fixture with T=fl-b (or fix T=fl-a and refuse otherwise).

### L5 - Low: refusal 3 and 4 overlap, and spec S12 is narrower than the design
Spec S12 says "only alive data nodes that are not draining"; the design also excludes witness-
labelled nodes (right, `Place` does). Refusal 4 is only reachable for a running node that is
not alive (a node whose heartbeat lapsed). Say so in both documents, or the spec and design
drift; add witness to the spec bullet.

## Nits
- N1: `make lint` wiring: add `scripts/tools/dirty-resync-test.sh` to the Makefile's list
  (`Makefile:92-94`) and name the Makefile/`lab.sh` dispatch/help edits in the file list (AGENTS:
  help aligned).
- N2: `design.md` "Discard (21 bytes, `frame.rs:279-284`)" is the body; the frame is 25 bytes with
  the length prefix (`frame.rs:1`, :180). Harmless to the bound.
- N3: "5 bytes x the 1 048 576 blocks MEMBER holds": digests cover the source-mapped blocks
  (plan intersect mapped, `target.rs:1458`), 1 064 960 here (4 160 MiB), about 5.1 MiB. State
  which, since the figure is "confirmed or not by the runs".
- N4: the byte cost named in intent is mostly tx (about 5 MiB) while the rx difference is about
  0.3 MiB of Discards plus requests; the results note should say the rx difference is small by
  construction so a reader does not take it as the digest cost.
- N5: the 32 MiB slack leaves about 22 MiB for background traffic; fine for runs of a minute,
  but the 1800 s timeout allows much longer: say a run over, e.g., 300 s is invalid rather than
  tuned, or the allowance failure on a slow run is a false alarm.

No new safety rule, wire, token or on-disk change; no injected bug owed.
