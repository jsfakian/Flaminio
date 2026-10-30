# Issue #14: Tombstones on nodes outside Kubernetes — decisions

Settled in the plan reviews (seven rounds). Reviewers treat these as given.

## Choices

- **Step 2 only.** Step 1 (the sweep in the node agent, planned in issue #13) waits; #14 stays open.
- **The service's rules and the API stay.** Proto comments too: a change regenerates `api/gen`.
- **One service change:** each new confirmation logs one line, after a `TxnRev` write: audit.
- **Two commands,** `tombstones [node]` and `confirm`, in `flaminio-meta`: the issue's step 2.
- **`confirm` reads first** and calls only when pending: the empty response cannot say more.
- **Nothing to confirm exits 0:** a repeat, or a retry after a lost answer, is not an error.
- **An explicit `--replicas-deleted`,** parsed strictly: the operator asserts the delete.
- **`ContinueOnError`:** `-h` and a bad flag now exit 1, for testability (a change).
- **Only the node's own agent confirms;** administrators are refused: "not alive" is a 10 s
  lease, true of any stopped node that holds replicas.
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
  re-check (detached, `deleted 0`), confirm; `{}` at the confirm fails.
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
- **Same-name re-creation hazards** are issue #30's, not handled here.

## Accepted risks

- **A second confirmer with fl-c's own agent identity** is excluded only by procedure.
- **A sweep started on fl-c,** or a create and delete of the id, between `vol-confirm`'s checks
  and its confirm (seconds).
- **Tombstones keyed by name:** a confirmation can land on a newer deletion; an incarnation
  field is a follow-up.
- **Two harness runs at once** can delete each other's replicas, as today.
- **fl-c holds the token key and an etcd client certificate:** agent scoping guards mistakes,
  not a compromised fl-c; a follow-up.
- **`smoke-*`, `resync-*` and CSI PVC deletions** keep leaving fl-c tombstones until step 1.
- **No data-plane fence on deletion:** a stale target can create replicas again; `vol-confirm`
  re-checks before confirming; issues #13 and #30.
- **A log line can be missing** after etcd retries a write whose answer was lost.
- **A campaign with replicas on fl-c** stops at each deletion until an operator runs
  `vol-confirm`, then the interrupted command again.
- **A volume placed on the campaign's node during the campaign** is lost at its next reset, as
  today.
- **The one-time cleanup starts fl-c with `make node-up VM=c`,** which takes the disk by serial
  rather than re-running the claim as `ensure_node_c` does; it is the same disk in the lab, and
  `vol-confirm`'s disk check refuses if it is not (final review, Low).
