# Issue #44 — report

## Review rounds

### Plan review round 1 — 07339ef6cc0c9ea6303d22478c59e0ffaf8dc99b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b1 (design-code and design-security).

- **R1-B1 — High:** The check measured the bytes the member received, but the digests it was
  meant to cost are replies the member sends; the 46 MiB of follow-ups §3 was read as digests.
  Evidence: `Response::Digests` at `flaminio-repl/src/target.rs:948`, requests to source and
  member at `:1861-1880`, the source being the target's own node (`source`, `:1644-1653`);
  5 bytes per block (`flaminio-net/src/frame.rs:306-318`); Discards in `max_io` pieces
  (`target.rs:966`, `flaminio-io/src/node.rs:330`).
- **R1-C1 — Medium:** Nothing made the volume land on T and the member only, and a chunk on
  fl-c leaves a tombstone the check cannot clear. Evidence: `meta/domain/domain.go:354-358`,
  `meta/service/service.go:397-402`, `scripts/lab.sh:1782-1786`, `docs/lab.md:207-229`.
- **R1-C2 — Medium:** S4's engine test could not observe the line: the engine logs with
  `eprintln!` and the design named no seam. Evidence: `flaminio-node/src/engine.rs:1772`,
  `flaminio-node/tests/engine.rs`.
- **R1-C3 — Medium:** The time figure's resolution was unspecified and coarser than the
  signal (about 0.2 s of digest work against 5 s polls over ssh). Evidence:
  `scripts/checks/resync.sh:50`, `wait_isr_full` in `scripts/lab.sh`, intent Why 3.
- **R1-C4 — Medium:** S3's condition can become true without a join, and the design then never
  reported; totals of a member that holds nothing were never dropped. Evidence: `leaving`,
  `target.rs:2334-2341`; `adopt_replicas`, `:2609-2641`.
- **R1-C5 — Low:** The check's preconditions differed between spec and design, and two had no
  mechanism ("exclusive lab use", the sweeps running). Evidence: `docs/lab.md:138,210`,
  `scripts/lab.sh:1796`, `flaminio-io/src/control.rs:104`.
- **R1-C6 — Low:** The design added `valid`, a reason, u128 and an "invalid summary" failure
  that no rule has. Evidence: spec S4 and S11; `design.md:26,47,58-63,110` of the reviewed
  commit.
- **N1 — Nit:** `spec.md:5` said "Decisions and design: to come".
- **N2 — Nit:** `design.md:17` said `node.sh` validates `attach`; only `detach` and `delete`
  call `volume_id` (`scripts/guest/node.sh:76-91`).
- **N3 — Nit:** `design.md:19-21, 173-176` cited audit #81, audit D01, ADR 0007 and #83, which
  are not on this branch and which no rule depends on.
- **N4 — Nit:** `design.md:146`: the implementation checks omitted `make hygiene`.
- **N5 — Nit:** `design.md:144` did not name the file that computes the S14 figures.

Changes/resolution:
- **R1-B1:** confirmed against the code. S9, S14 and the design now read the member's wg0
  `tx_bytes` with `rx_bytes`, and take the byte difference of the two runs each way; the limit
  of S11 stays on what is received. The allowance is 32 MiB, derived from the frame encoding
  and the measured transport overhead on W, not from the 46 MiB. S7.3's reason, intent Why 3
  (1.25 MiB per GiB mapped per remote party; discards added), What we want 2 and open
  question 2's extrapolation are restated. The tx counter is in the script-test fixtures. The
  results note that the rx difference follows the holes, not the data held.
- **R1-C1:** S12 and the design refuse, before any create, unless T and the member are the
  only alive, non-draining, non-witness nodes (`metactl nodes`); the refusal has a script
  test. The placement is still inspected after the create; the remaining window is under Risks.
- **R1-C2:** two seams, both in `flaminio-node/src/engine.rs`: a pure `resync_summary_line`,
  unit-tested for its exact text, and `Engine::set_log`, a line sink the engine test sets
  (default `eprintln!`). The spec's S4 test names them, and its "file-backed store" is
  corrected to the in-memory stores the engine tests use.
- **R1-C3:** a watcher on fl-a polls every 0.5 s and gives the wall interval; S9 and S14 take
  the time figure and its difference from the summary's `elapsed_us`, the wall interval listed
  beside it and never subtracted.
- **R1-C4:** S3 and the design evaluate the summary after every applied view, for each member
  with counted rounds; a member that holds nothing gets no summary and its totals are dropped.
  One unit test for each case replaces "source removal is not a spurious summary".
- **R1-C5:** S12 lists the refusals and the design names the command for each: the two nodes,
  `systemctl is-active`, control `status` answering `ok none`, `pgrep -x flaminio-csi`,
  `metactl nodes`, and `status` on the other data nodes with `metactl get`. "Exclusive lab
  use" is no longer a check.
- **R1-C6:** `valid`, the reason, u128 and the "invalid summary" failure are dropped; `epoch`
  stays and is in S4.
- **N1:** the spec links the design and the new `decisions.md`.
- **N2:** the sentence says what `node.sh` validates, and that the check validates its own id.
- **N3:** cut to the one fact used (the check stays at 64 chunks); the ADR paragraph names
  only the accepted ADRs relied on.
- **N4:** `make hygiene` added to the implementation checks.
- **N5:** the figures are computed by `dirty-resync.sh` itself (`summarize`);
  `scripts/tools/summarize.py` is not used.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 0dc56f2

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b2 (incremental design-code and design-security).

Every round-1 finding was confirmed fixed. The findings of the revision:

- **L1 — Low:** `metactl nodes` omits false booleans, so the filter of the alive-node refusal
  must default them. Evidence: `EmitUnpopulated: false`, `meta/cmd/flaminio-meta/main.go:501`;
  the idiom at `scripts/lab.sh:614`.
- **L2 — Low:** the volume id read from a remote `status` answer reached `metactl get`
  unvalidated, and a malformed answer was not said to refuse. Evidence: `ok V /dev/ublkbN`,
  `flaminio-io/src/control.rs:104-107`; `valid_id`, `scripts/lab.sh:1633`.
- **L3 — Low:** the cleanup's trap was installed before `create`, so a `create` failing on an
  existing id would delete a volume the run did not create. Evidence: `design.md` "Trap
  EXIT/INT/TERM after capturing original state" of the reviewed commit.
- **L4 — Low:** the watcher and the journal cursor assumed T is fl-a, while `T` was a
  parameter. Evidence: the administrator's key is on fl-a (`scripts/lab.sh:529`); spec S12.
- **L5 — Low:** the refusals on alive nodes and on volumes attached elsewhere overlap, and
  spec S12 did not exclude witness-labelled nodes as the design and `Place` do.
- **N1 — Nit:** the `make lint` wiring (`Makefile:92-94`) and the Makefile, `lab.sh` and help
  edits were not in a file list.
- **N2 — Nit:** a `Discard` is 25 bytes with its length prefix, not 21
  (`flaminio-net/src/frame.rs:1`, `:180`).
- **N3 — Nit:** the digests cover the blocks of the plan the source maps (`target.rs:1458`),
  1 064 960 here, not the 1 048 576 the member holds.
- **N4 — Nit:** the results note should say the rx difference is small by construction, so it
  is not read as the digests' cost.
- **N5 — Nit:** the 32 MiB allowance leaves about 22 MiB for background traffic, while the
  1800 s timeout allowed far longer runs.

Changes/resolution:
- **L1:** the design gives the filter with `(.alive // false)`, `(.draining // false)` and
  `(.labels // {} | has("flaminio.io/witness"))`, and the script test's fixtures are in the
  client's protojson shape, false fields absent.
- **L2:** a value read from a remote reply is matched before it reaches a command or socket
  line (a volume id by `valid_id`; the token, the epoch and the device path by their shape).
  A `status` answer that is neither `ok none` nor `ok V /dev/ublkbN` refuses; one script-test
  case (no `metactl get` recorded).
- **L3:** the run refuses when `metactl get` of its id does not answer NotFound, and sets its
  trap only once `create` returned success; a failed or lost `create` stops with the id named
  and deletes nothing. Spec S12 says it deletes no volume it did not create. Script test:
  `create` fails with AlreadyExists, no delete and no detach recorded.
- **L4:** T is fl-a and the member fl-b, no longer parameters (spec S12, design, the lab
  command): the watcher, the journal cursor and the summary are all on T. The refusal on the
  two inputs and its test cases are gone.
- **L5:** spec S12 excludes witnesses and says what the second refusal covers (a node that is
  draining, or that runs but is no longer alive); the design says the same, checked on fl-c.
- **N1:** the design has a file list, with the Makefile's one line in `lint`; the check gets
  no Makefile target and no `lab.sh` command, as the other checks, so the help does not change.
- **N2:** 25 bytes with the length prefix, in the facts and in the allowance (0.3 MiB of
  frames, still under 2 MiB at 150 bytes each).
- **N3:** the facts say the digests cover what the source maps of the plan, 5 bytes per block
  (the store gives an unwritten block the zero block's CRC); the expected figure is about
  5.1 MiB for 1 064 960 blocks. Intent Why 3, spec S7.3 and the decisions follow.
- **N4:** the results note says the rx difference is small by construction and that the
  digests' cost is the tx difference.
- **N5:** one limit of 300 s replaces 1800 s; a longer run is invalid, and neither limit is
  tuned after a failure (design, and a failure added to spec S11).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 093f431

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b3 (incremental design-code and design-security).

Every round-2 finding was confirmed fixed, and every figure checked against the code. The
findings of the revision:

- **L1 — Low:** the check's helpers are not in `lib.sh`: `valid_id`, `data_vms` and `metactl`
  are `lab.sh` functions, which the other checks extract. Evidence: `scripts/checks/reconcile.sh:15`,
  `scripts/checks/resync.sh:12`; `scripts/lab.sh:529,1628,1633`.
- **L2 — Low:** refusal 3 did not say what a failing `metactl get V` does: it is neither a
  volume without a replica on T or MEMBER nor a malformed `status` answer.
- **N1 — Nit:** "Add lab procedure/help" and "command help matches the actual script" read as
  if `lab.sh` or `make help` changed, against the file list.
- **N2 — Nit:** a `create` whose answer is lost may leave the volume, so S12 is knowingly
  unmet; the message should print the cleanup command, as the fl-c case does.
- **N3 — Nit:** the design did not say where the 300 s start.

Changes/resolution:
- **L1:** the design says the script sources `scripts/lib.sh` and takes `metactl`, `data_vms`
  and `valid_id` out of `lab.sh` with `sed -n`, as the other checks do (`valid_id` is one line,
  so `/^valid_id()/p`), and that what they use comes with `lib.sh`. The script test runs that
  extraction against the repository's `lab.sh` and fails if one of the three is undefined.
- **L2:** a failing `metactl get V` refuses (design refusal 3, spec S12); one script-test case.
- **N1:** the design says the procedure goes to `docs/lab.md`, and that `docs/lab.md` matches
  the script; no help changes, as the file list says.
- **N2:** the message of a lost `create` prints `VOL=<id> make vol-delete` for an operator;
  the script test checks it.
- **N3:** the 300 s run from the watcher's first line, counted by the watcher on T's clock;
  the script test has the boundary (299 s passes, 301 s fails).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 65ba058

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b4 (incremental design-code and design-security).

Every round-3 finding was confirmed fixed. Nothing at Low or above. The findings of the
revision:

- **N1 — Nit:** the 300 s are counted by the watcher, but the design named nothing that bounds
  the watcher when one `metactl get` or the ssh hangs. Evidence: `design.md`, the paragraph
  on what invalidates a run, of the reviewed commit.
- **N2 — Nit:** refusal 3 has three outcomes for fl-c; the reviewer asks for no change beyond
  the test row's order matching.

Changes/resolution:
- **N1:** the design states two bounds, both already in the scripts. In the watcher, each poll
  is `timeout 10 flaminio-meta get` (as `scripts/checks/reconcile.sh:57`) and the loop reads
  T's clock after every poll, so it exits non-zero at most 310.5 s after its first line. On
  the host, the wait is the watcher's ssh, which a dropped connection ends with 255 after
  120 s without an answer (`scripts/lib.sh:28-29`); an ssh that ends without the second line is
  a run over the limit. Two script-test cases: a hung `get`, and an ssh ending with 255. No
  host-side timeout is added.
- **N2:** no change, as the reviewer says; the test row keeps the order of the refusal.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 6fec02a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b5 (incremental design-code and design-security).

R4-N1 was confirmed fixed in substance. The findings of the revision, all in the lines round 4
added (the reviewer gave no ids; numbered here in its order):

- **R5-L1 — Low:** the citation "as `reconcile.sh:57` bounds its reads" was wrong, and so was
  "both already in the scripts". Evidence: `scripts/checks/reconcile.sh:57` is
  `sudo timeout 300 dd …`; no `flaminio-meta` call under `scripts/` has a `timeout`
  (`reconcile.sh:46`, its `meta()`, has none).
- **R5-L2 — Low:** "at most 310.5 s" was not derived: the 0.5 s sleep was not stated, and
  process start-up was not counted.
- **R5-L3 — Low:** the keepalives fire only on a dead connection (`scripts/lib.sh:28-29`), so
  the host's wait is bounded only because the watcher limits itself; and a dropped ssh leaves
  the watcher running on T while the cleanup starts.
- **R5-L4 — Low:** "an ssh that ends without the second line is a run over the limit" was not
  in spec S11, and a dropped ssh is an invalid run, not an exceeded limit.
- **R5-N1 — Nit:** `decisions.md` repeated the "already in the scripts" claim.

Changes/resolution:
- **R5-L1:** the design says the per-poll `timeout 10` is new in this check, that no
  `flaminio-meta` call under `scripts/` has one, and cites `reconcile.sh:57` only for what it
  is: `timeout` on a `dd`, so the command is in the VMs. "Already in the scripts" is gone.
- **R5-L2:** the bound is given by its terms: 300 s, one 0.5 s sleep between polls and one
  poll of at most 10 s, so about 310 s, process start-up not counted. A poll that fails or is
  cut is polled again.
- **R5-L3:** the design says the host's wait is bounded by the watcher's own limit, that the
  keepalives only end a dead connection, that the watcher may then go on until its limit, and
  that it only reads, so the cleanup neither waits for it nor depends on it. The script-test
  case says the cleanup runs without waiting for the watcher.
- **R5-L4:** spec S11's last bullet names the case (the check loses its watch on the ISRs
  before it sees them full: an invalid run), and the design calls it invalid, like any command
  failure, not a run over the limit.
- **R5-N1:** the decisions entry is rewritten to match.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — f2655aad82dbb9673f95e23f9cc6e4a54b7cd2e2

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b6 (incremental design-code and design-security).

r44b6 confirmed R5-L1 to R5-L4 and R5-N1 fixed, each against the tree, and reported nothing of Low or worse.

- **R6-N1 — Nit (optional):** The S12/S13 test row could also name the case where the watcher exits non-zero at its own limit as S11's invalid run; "still exits at its limit" already covers it. Evidence: design.md, test table row S12, S13; spec.md S11, last bullet.

Changes/resolution: the row now reads "the watcher still exits non-zero at its limit and the run is invalid (S11)". One clause in design.md, made by the orchestrator; nothing else changed.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — aa0457b7601c7ce03f4e17d8ebcfa49dc22c6c6e

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r44b7 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
