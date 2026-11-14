# Issue #14: Tombstones on nodes outside Kubernetes — decisions

Settled in the plan reviews (seven rounds). Reviewers treat these as given.

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-13) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below. The two risks named in
  round 1 are accepted under the user's instruction of 2026-11-13 (continue unattended, choose
  the recommendations), as the review's coordinating session applied it: see Risks accepted
  after approval.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Work on this issue merged before this approval was recorded (pull request #31); the issue is
still open. Approved before its plan review cleared (status: pending).

## Settled at approval

None: `decisions.md`, `spec.md` and `design.md` have no Open questions section, no numbered
question and no choice labelled Proposed.

## Choices

- **Step 2 only.** Step 1 (the sweep in the node daemon, `flaminio-io node`, planned in issue
  #13) waits; #14 stays open.
- **The service's rules and the API stay.** Proto comments too: a change regenerates `api/gen`.
- **One service change:** each new confirmation logs one line, after a `TxnRev` write: audit.
- **Two commands,** `tombstones [node]` and `confirm`, in `flaminio-meta`: the issue's step 2.
- **`confirm` reads first** and calls only when pending: the empty response cannot say more.
- **Nothing to confirm exits 0:** a repeat, or a retry after a lost answer, is not an error.
- **An explicit `--replicas-deleted`,** parsed strictly: the operator asserts the delete.
- **`ContinueOnError`:** `-h` and a bad flag now exit 1, for testability (a change).
- **Only the node's own agent confirms;** administrators are refused: "not alive" is a 10 s
  lease, true of any stopped node that holds replicas. Since #30 confirming such a node harms
  less: a volume created again drops its leftovers at its first session there, and #30's
  residual cases remain (S8, S9). The choice stays, more conservative than #30 needs.
- **The CLI checks its identity against the node before any call,** not only the service.
- **The harness never confirms.** One confirmer per node: CSI sweep on fl-a/fl-b, an operator
  through `vol-confirm` on fl-c.
- **`vol-confirm` is the only supported way to confirm by hand.**
- **No witness step:** it cannot tell `vol-confirm` from a hand confirmation with the same
  certificate; a nonce would add a mechanism.
- **No Kubernetes API check, no `OUTSIDE_K8S`:** on-node checks only; fl-c is the data node
  that is not a k3s agent.
- **No lab lock** in this step: concurrency is as today; a follow-up.
- **One shared `delete_volume`** for wbench, the `$VOL` removal and `vol delete`.
- **Stopped nodes are started before cleaning,** the campaign's node with `FORMAT=1` (a raw run
  overwrote it, and the campaign formats it anyway): no unrecorded replicas are skipped.
- **Pending on fl-c stops at once,** printing the procedure: waiting cannot help.
- **The wait:** 120 s by default, 5 s polls, its own loop to name the pending nodes.
- **ssh's 255 propagates,** so that `RETRIES` resumes.
- **`node.sh` prints one status line** for `delete` and `detach`, with distinct exits.
- **`vol-confirm`'s order:** cluster refusal, listed, no sweep, detached, disk, delete,
  re-check (detached; data plane, disk and no sweep again; `deleted 0`), confirm; `{}` at the
  confirm fails.
- **`$VOL` at a campaign's start:** decided over every volume first; any other volume but
  wbench and `$VOL` on the campaign's node stops it, named, before anything is removed; then
  `$VOL` is removed, as today. Chosen over refusing `$VOL` too: keeps today's behaviour.
  `VOL`'s default stays.
- **That check runs at the campaign's start only,** not before each reset: cheaper, and a
  volume placed on the node during a campaign is lost at the next reset today too.
- **`write_volume` waits for the campaign's node to be alive,** and checks placement: placement
  drops a preferred node that is not alive.
- **The restore's drain loop goes:** `$VOL` is deleted at the start.
- **The one-time cleanup** uses `vol-confirm`, for every fl-c tombstone, `smoke-*` and
  `resync-*` included.
- **Same-name re-creation hazards** are issue #30's, not handled here. #30 is done: a volume
  created again drops an earlier one's leftovers at its first session on each node; its S8 and
  S9 name what is left. A delete of the name that runs after the id is freed still wipes the new
  volume's replicas: that is what the tombstone guards.

## Accepted risks

- **A second confirmer with fl-c's own agent identity** is excluded only by procedure.
- **A sweep started on fl-c,** or a create and delete of the id, between `vol-confirm`'s checks
  and its confirm (seconds).
- **Tombstones keyed by name:** a confirmation can land on a newer deletion; an incarnation
  field is a follow-up. With it, the deleters could delete only the chunks fenced below the
  tombstone's revision, which is above every epoch of the deleted volume and below any volume
  created again (`Store::delete_volume(uuid, Some(..))`, which #30 added): that would close the
  wipe by a late delete (the spec's invariant), outside #30's S8-S9 cases. A volume created by
  an old metadata service starts at epoch 0, and one on a store whose revision went back can
  start low: both can sit below the tombstone's revision, and a bounded delete still wipes them.
- **Two harness runs at once** can delete each other's replicas, as today.
- **fl-c holds the token key and an etcd client certificate:** agent scoping guards mistakes,
  not a compromised fl-c; a follow-up.
- **`smoke-*`, `resync-*` and CSI PVC deletions** keep leaving fl-c tombstones until step 1.
- **No data-plane fence on deletion:** a stale target can create replicas again; `vol-confirm`
  re-checks before confirming; issues #13 and #30.
- **A log line can be missing** after etcd retries a write whose answer was lost: the retry finds
  the node confirmed, or, after the last confirmation, no tombstone.
- **A campaign with replicas on fl-c** stops at each deletion until an operator runs
  `vol-confirm`, then the interrupted command again.
- **A volume placed on the campaign's node during the campaign** is lost at its next reset, as
  today.
- **The one-time cleanup starts fl-c with `make node-up VM=c`,** which takes the disk by serial
  rather than re-running the claim as `ensure_node_c` does; it is the same disk in the lab, and
  `vol-confirm`'s disk check refuses if it is not (final review, Low).

## Risks accepted after approval

Named in plan review round 1 (2026-11-13), after approval; they describe the shipped code.
Accepted in round 2 under the user's instruction of 2026-11-13 (continue unattended, choose the
recommendations), as the review's coordinating session applied it. Why: they concern the lab
harness only, their window is seconds, and #13's S13 removes the harness's per-node deletes that
open the first one.
- **A name created and attached while the harness deletes it** (R1-S2, R2-B1). `delete_volume`'s
  deletes on every data node ignore fences, and can run after the sweeps of fl-a and fl-b have
  freed the id (an RF 1 wbench on fl-b, within 5 s); its re-check sees only a finished creation
  (`get` answers `NotFound` for one in progress). A creation alone writes only metadata: a node
  gets chunks when a session opens there. So the name created and attached by another caller
  (`make vol-create` then `make vol-attach`, or a second campaign), whose first session reaches
  a replica node between the re-check and the delete there (seconds), loses its replicas on the
  nodes it is not attached on. The node it is attached on refuses the delete ("attached here"),
  and the deletion stops there, sparing the nodes after it. It widens "two harness runs at once"
  to any other lab command while a deletion runs. So the next step (design) makes `docs/lab.md`
  state "no other lab command while a deletion runs" (spec, S10), and follow-ups §14's lab-wide
  lock item widens to cover it. The alternative the review offered, deleting the replicas before
  the metadata delete so that the live header blocks re-creation, would change shipped code that
  #13's S13 removes; closing the risk that way would restate the spec's invariant.
- **A confirmation by another caller between `confirm`'s read and its call** (R1-C3) is reported
  as `vol-confirm`'s own: the service answers the same empty success to a new confirmation, a
  repeat and a missing tombstone. Telling them apart is the incarnation follow-up's.

## Plan review round 1 (2026-11-13): changes made after approval

Reviewed commit 24877eb. Each change describes the code as it is after step 2 (pull request #31)
and #30; none narrows the intent or a rule but where a finding shows the rule false as stated.
- **R1-C1, R1-S1 (the hazard model predates #30):** intent Why 5 and 6, the spec's invariant and
  S2's usage text now say what a wrong confirmation does at HEAD: a delete of the name after the
  id is freed wipes the new volume's replicas; leftovers otherwise hold space until its first
  session, old tokens still open sessions on them, an old metadata service's volume is deposed.
  S2 named the consequence as adoption or deposition, which #30 made false. The design's facts:
  the control socket's delete is `Store::delete_volume(uuid, None)`, not under `u64::MAX`; #30's
  first epoch and drop. "Only the node's own agent confirms" and "Same-name re-creation hazards"
  keep their choice, with #30's reason. The incarnation follow-up names the bounded delete. The
  shipped usage text and `TestUsage` are corrected in the design's next step.
- **R1-S2 (the harness's deletes):** the invariant is restated, as the finding shows it false:
  one deleter and confirmer per node, plus the harness while no other lab command runs. S5 and
  the design's `delete_volume` step 4 name the residual; a risk named above.
- **R1-C2 (#13):** the node daemon, `flaminio-io node`, not "the node agent" (intent, decisions);
  the design's step 3 no longer adds #13's sweep to the no-sweep check: `vol-confirm`,
  `confirm.sh` and `confirm` go with #13's sweep (#13, S13).
- **R1-C3 (S7.8):** "the read just before the call shows the volume still pending" (spec, design
  step 7); the read-to-call window is a risk named above. `docs/lab.md:243`: the next step.
- **R1-C4 (S6):** an RF 1 campaign on fl-b never stops for wbench, and stops once at its start
  when `$VOL` has a replica on fl-c; S6 said it never stops, which is false. `docs/lab.md:226-227`:
  the next step.
- **R1-C5, R1-S3 and the security nits (design facts):** `confirm.sh`'s fixed paths, overridden
  only under `FLAMINIO_SCRIPT_TEST=1` (and the control socket's); the no-sweep check through
  `systemctl show`, not found or inactive and disabled or masked; `confirm` re-runs the data plane
  and no-sweep checks; `vol-confirm` refuses a node not in `data_vms`; the wait retries only
  ssh's 255 and transient errors; `cmd_vol detach` checks its status line; `node.sh delete`
  prints `not running` since step 2. Tests listed to match.
- **R1-C6 and the security nit (intent references):** the log line is S4; the later steps point
  to decisions and follow-ups §14, not "spec S13".
- **Code review nit (S4):** S4 states the exception: no line when etcd lost the answer to a
  committed write, the last confirmation's included; the accepted risk says so too.

## Plan review round 2 (2026-11-14): changes made after approval

Reviewed commit de8d800 (the plan files as at fb13b26).
- **R2-B1 (the harness's residual, stated too wide):** it needs the name created and attached,
  with a first session reaching a replica node between the re-check and the delete there, and
  it loses replicas only where the volume is not attached; the example is `make vol-create` then
  `make vol-attach`, or a second campaign (above, spec S5, design step 4 and Risks). The
  invariant says a control-socket delete spares the node where the volume is attached.
- **R2-B2 (the risks' status):** both risks are accepted, as recorded above, and the spec (the
  invariant, S5, S7.8) and the design (step 4, step 7, Risks) say "accepted (decisions)". S10
  and the next step add "no other lab command while a deletion runs" to `docs/lab.md`, and the
  follow-ups' lab-wide lock item widens to cover it.
- **R2-C1 (the shipped follow-ups):** the next step corrects follow-ups §14 step 1 (the node
  daemon, `flaminio-io node`, #13; `vol-confirm`, `confirm.sh` and `flaminio-meta confirm` go
  with it) and row 26's "When" column (#13); these go with #13 if #13 lands first.
- **R2-N1 (line numbers):** the next step cites `docs/lab.md:246` for `vol-confirm`'s step 7 and
  248-252 for the wrong-confirmation paragraph.
- **R2-N2 (#30's own residual cases):** S2, the next step's usage text and follow-ups item, and
  the incarnation follow-up above say "outside #30's S8-S9 cases" (a store whose revision went
  back; a volume created by an old metadata service at epoch 0).
