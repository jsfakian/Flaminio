# Issue #18: Moves and drains ignore a volume's failure domain — design

How the [spec](spec.md)'s rules are built. Settled choices: decisions (after approval).
Approved 2026-11-13; changed after approval by plan review round 1 (decisions.md); not
implemented.

The facts it starts from:
- **The header:** `Header` has no level or policy (`meta/domain/domain.go:37`); `NewVolume` copies
  neither from the `Spec` (`:199`). The header is JSON at `volumes/<id>`, written whole from the
  struct by `createVolume` (claim and final write, `meta/service/service.go:414`, `:467`),
  `Attach` (`:724`) and `DeleteVolume`'s mark (`:541`). Every answer with a volume goes through
  `volumePB` (`:337`). `Volume`'s last proto field is 8 (`meta.proto:117`).
- **The move:** `MoveReplica` checks the destination's record once, before its loop (`:870`),
  then per attempt reads the header and the chunk, answers `applied=false` on `CheckMove`, and
  writes the chunk if chunk and header are as read (`:907`); 32 attempts, then `Aborted`. No
  path of it answers `FailedPrecondition` today (`errStatus`, `:143`).
- **Node records** (`nodes/<id>`, address and labels) are rewritten only when the address or a
  label changes (`:202`), and never deleted. Liveness and the drain mark are other keys.
  `Cmp{Rev: 0}` means absent (`meta/kv/kv.go:18`). With this change a move compares at most 7
  keys: the chunk, the header, the destination and at most 4 staying replicas (5 replicas, less
  the source; `CheckMove` refuses while a move is pending).
- **Domains:** `LoadFromLabels` and `domainKey` (`domain.go:311`, `:322`): unlabelled nodes share
  one domain per level. `control/csi` already imports `meta/domain`; `control/drain` may too.
- **The drain:** as the intent says (`control/drain/drain.go:74`, `:120`, `:150`, `:181`).
  `flaminio-meta drain` prints progress on stderr, `{}` on success; any error exits 1
  (`meta/cmd/flaminio-meta/main.go:76`, `:436`). `lab.sh` discards its stdout (`:1279`).
- **Rust:** `pb.rs`'s `Volume` names tags 1–8 (`flaminio-metaclient/src/pb.rs:56`); prost skips
  unknown fields; the client only decodes a `Volume` (`lib.rs:190`). The vectors come from the
  Rust model alone (`Makefile:105`); the Go replay calls `NewVolume(Spec{ID, MinISR}, …)`
  (`meta/domain/domain_test.go:60`). No Rust file changes: `make contracts` proves it (Tests).

## The record (S1–S4)

- **`Header`** gains `FailureDomain string` (`json:"failure_domain,omitempty"`) and
  `FailureDomainPolicy string` (`json:"failure_domain_policy,omitempty"`): the request's words.
- **`NewVolume`** sets them to `s.Domain.String()`, `s.Policy.String()`: never empty, defaults
  included, and in the claim's header already. `Attach` and the deletion mark keep them: they
  marshal the header they read.
- **`(h *Header) Placement() (FailureDomain, PlacementPolicy, error)`** parses them with
  `ParseFailureDomain`, `ParsePlacementPolicy`: absent reads as `node`, `strict` (S3). A header
  without the fields marshals without them, so no write adds the record.
- **`volumePB`** reports the stored words, `node` and `strict` for empty ones.
- **A stored word the service does not know** (a later version's, or damage): reported as it is;
  attach, ISR changes and deletion work; `MoveReplica` answers `Internal`
  (`volume "v": recorded failure domain "x", policy "y"`) and records nothing.
- **No version rule, no fuzz target:** this is the etcd record, not the on-disk format.

## Accepting a move (S5–S9)

`MoveReplica`, per attempt, after `CheckMove` passes (its `applied=false` answer comes first, as
today; the registered check before the loop stays):
1. `h.Placement()`. `bestEffort`, or level `node`: write as today, two compares. Nothing more is
   read (S6).
2. Otherwise read, in this order: the destination's record (`NotFound` as `:872` if gone), then
   each staying replica's (`c.Replicas` without `from`, in placement order). A missing or
   unparsable record gives a node without labels: unknown at `rack` and `zone` (S5, S8).
3. `domain.Strict(level, to, staying)`; if it gives a reason, the refusal below, nothing
   written.
4. One `Txn`: the chunk and the header at the revisions read, the destination's record at its
   revision, each staying replica's record at its revision (0 if it was missing); one op, the
   chunk. Not applied: the next attempt reads everything again.
- **A lost race** returns whatever the new read gives: applied, `applied=false`, `NotFound`, or
  the refusal. After 32 attempts: `Aborted`, "too much contention", as today.
- **`domain.Collision(level FailureDomain, to NodeLoad, staying []NodeLoad) (with, value string,
  ok bool)`:** the first of `staying` whose `domainKey(level)` is `to`'s, and the label's value
  (the id at `node`). Unlabelled nodes share one domain, as at creation: the drain's `bestEffort`
  choice uses it as it is.
- **`domain.Strict(level FailureDomain, to NodeLoad, staying []NodeLoad) string`** (S5): the
  bare reason, empty when the move is allowed; it wraps nothing and carries no prefix. Empty
  when `staying` is empty. At `rack` or `zone`, in this order: the first of `staying` whose label
  at the level is empty, `replica %q has no zone label` (it refuses every destination, so it is
  named first); `to`'s label empty, `destination %q has no zone label`; then `Collision`,
  `destination %q shares zone %q with replica %q`; `rack` for `zone` at `rack`. At `node` only
  `Collision`, which `CheckMove` has already made false.
- **The refusal:** `domain.ErrCollision = errors.New("failure domain collision")`, which
  `errStatus` maps to `FailedPrecondition`. Only the service builds the message, once, in this
  fixed order: `fmt.Errorf("%w: volume %q chunk %d: %s", domain.ErrCollision, id, chunk,
  reason)`, so the prefix comes once and first, and every id and label value is formatted with
  `%q` (a label is a free string a node sets on itself, `service.go:197-200`): `failure domain
  collision: volume "v" chunk 3: destination "c" shares zone "z1" with replica "a"`. The prefix,
  `domain.ErrCollision.Error() + ": "`, is the marker the drain tells this refusal by
  (Contracts).
- **With issues #13, #72 and #78** (whichever lands last states it): the transaction compares the
  chunk, the header, #78's gate (#13's `forgotten` mark until #78 lands), #72's `capacity/<to>`,
  the destination's record and at most 4 staying replicas' records: 9 compares at most, and 2
  operations (the chunk, #72's ledger), under `kv.MaxTxnOps` (120). #78's draining refusal
  comes before `CheckMove`, and #13's `applied=false` reasons from `CheckMove`: both before
  step 1.
- **The injected bugs:** unexported `skipCollisionCheck bool` on `Service`, set by the
  package's tests only, skips step 3; `skipStayingCheck bool` leaves the staying replicas
  without a label out of `Strict`'s input, a service that checks the destination but forgets an
  unknown staying replica.
- **`ChangeIsr`, `CheckMove`, `ApplyMove`, `validMove`:** untouched (S9).

## Creation (S5)

- **`leftOut(n NodeLoad, replicas int, level FailureDomain, policy PlacementPolicy) bool`**
  (unexported, the one predicate): true under `PolicyStrict` at `DomainRack` or `DomainZone`
  with `replicas >= 2`, for a node whose label at that level is empty (`LoadFromLabels` gives an
  empty one for a node without the label or with an empty value). `Place` and `LeftOut` both
  call it, so they cannot drift.
- **`Place`** (`domain.go:359`), after today's live-node check (on all nodes), leaves out the
  nodes `leftOut` names, then places as today. A preferred node left out is no longer one of the
  nodes, so it is ignored, as a preferred node that is not a candidate is today. Too few domains
  left: `ErrPlacement`, today's message. One replica, `bestEffort`, and level `node` are
  unchanged.
- **`domain.LeftOut(nodes []NodeLoad, replicas int, level FailureDomain, policy
  PlacementPolicy) string`:** `; %d nodes without a zone label left out` (`rack label` at
  `rack`), counting the nodes `leftOut` names, exactly when `Place`'s refusal is caused by too
  few labelled domains: some node left out, at least `replicas` nodes in all, and fewer than
  `replicas` distinct domains among the nodes kept. Otherwise empty: a refusal labels would not
  cure (more replicas than live nodes), `bestEffort`, level `node`, or nothing left out, gets no
  suffix.
- **`createVolume`** (`service.go:404`): when `errors.Is(err, domain.ErrPlacement)` and `LeftOut`
  is not empty, `err = fmt.Errorf("%w%s", err, suffix)`: re-wrapped with `%w`, so `errStatus`
  still maps it to `FailedPrecondition` (`service.go:143`), the suffix last. No other refusal is
  touched.
- **With issue #72** (whichever lands second): the filter runs before #72's room rule, so a node
  left out counts for no domain, and a full labelled one still counts as #72 says. The refusal
  for too few labelled domains stays `ErrPlacement` under #72 (`resolveDomain` fails before any
  node is skipped for room), so it carries #18's suffix, after #72's own left-out suffixes
  (`; no capacity report: <ids>`, `; no readable capacity ledger …`). #72's `ErrCapacity`
  (`RESOURCE_EXHAUSTED`) room refusal gets no suffix: its cause is room, not labels. #72's text
  needs the change (not made here).
- **Chunks placed before the rule:** none of a volume that records `rack` or `zone`: the record
  (S1) and this rule ship together, and a volume without the record reads as `node` (S3). A
  chunk gets a replica on an unknown node only when that node loses its label or record after
  placement (Risks): moving that replica is allowed; moving the others is not (S5), by hand, by a
  drain (blocked, with the reason `replica "x" has no zone label`) or by issue #45's rebuild.

## The drain (S10–S14)

`control/drain/drain.go`.
- **`Step`** keeps a `domain.NodeLoad` (from `LoadFromLabels`) for every listed node, not only
  the candidates: a staying replica may be dead or draining. A replica's node not listed has no
  labels: unknown at `rack` and `zone`.
- **`choose`** replaces the call of `leastLoaded` (`:120`). Level and policy:
  `ParseFailureDomain`, `ParsePlacementPolicy` of the volume's fields; a word they refuse makes
  each of the volume's chunks on the node without a pending move blocked, with the words as
  read, and the step goes on to the other volumes (S11). Eligible: the candidates without a
  replica of the chunk, less the nodes this step took out (below). `strict`: the eligible nodes
  for which `domain.Strict` gives no reason, so no node is proposed beside a staying replica
  unknown at `rack` or `zone`, and no unknown node beside any.
  `bestEffort`: those for which `domain.Collision` is false at the level, then one level down,
  to `node`, stopping at the first level with any. Then `leastLoaded`, unchanged.
- **Blocked:** no node chosen (no eligible node included): the chunk is counted, no request is
  made, the loop goes on. The error at `:122` goes.
- **A blocked chunk's reason** (S12), kept for the first blocked chunk of each volume, in chunk
  order. C is the candidates without a replica of the chunk, releasing ones included; Left is C
  less the nodes refused for this chunk in this step and those this step's refusals took out;
  the step keeps, for each node it took out, the message of the refusal that took it out.
  Exactly one applies, the first in this order:
  1. the chunk refused in this step (a collision, or #72's rule), and `Strict` refuses no node of
     Left (Left empty, or after a collision, which is not followed by a new choice, nodes that
     pass): the chunk's own last refusal's message;
  2. words the drain does not know: `failure domain or policy not known to the drain`;
  3. C empty: `no eligible node`;
  4. Left empty, the chunk not refused itself, so every node of C was taken out by earlier
     chunks' refusals: `every candidate refused earlier in this step: ` and, for each node of C
     in id order, `<node>: <message>`, joined by `; `. It names this chunk's own candidates and
     why each went, not another chunk's refusal;
  5. otherwise (under `strict` only: a `bestEffort` chunk with a node of Left is never blocked):
     `domain.Strict`'s reason for the least loaded node of Left that it refuses, lowest id among
     equals. An unknown staying replica is named first: `replica "x" has no zone label`. A
     refusal of this chunk does not hide it.
- **A refusal of the drain's proposal:**
  - **A collision:** `status.Code(err) == codes.FailedPrecondition` and
    `strings.HasPrefix(status.Convert(err).Message(), domain.ErrCollision.Error()+": ")`: the
    marker comes from the constant the service builds its message with, not from a literal in
    the drain. The chunk is counted blocked and refused for the step; the destination stays a
    candidate for the other chunks: the collision concerns that chunk only (S14).
  - **Any other `FailedPrecondition`, or `ResourceExhausted`** (issue #72's destination without
    a report, a ledger or room; issue #78's draining destination): issue #72's rule. The
    destination leaves the step's candidates, for the later chunks and volumes too, and `choose`
    runs again for the chunk under the same rules (allowed set, level, fall-back). None left:
    the chunk is blocked, not refused, and the step goes on (S11, S14); the last refusal's
    message is its reason, in the progress line and in `BlockedError`, not in a failed step (a
    change to #72's S8, whichever of #72 and #18 lands second). Neither plan's code is needed for
    this rule: today no other `FailedPrecondition` or `ResourceExhausted` comes.
  - **Other errors** as today.
- **With issue #13** (whichever of #13 and #18 lands second; #13's decisions, hand-offs to #45
  and #18), `choose` follows this order:
  1. A chunk at #13's bound (`MaxReleased` releases) waits before `choose`, as #13's drain says.
  2. Eligible also excludes the chunk's `GetReleased()`.
  3. The domain filter above runs on what is left.
  4. No node left: if a candidate in the chunk's `GetReleased()` would pass the same filter,
     the chunk waits, `volume V: chunk C: only nodes releasing it are left (N)` in
     `Progress.Held`, counted in `Remaining`, not in `Blocked`; otherwise it is blocked.

  A move #13 answers `applied=false` (a release committed since the step's read) is left to the
  next steps, as #13 says.
- **With issue #17** (whichever of #17 and #18 lands second): `choose` is #17's seam
  (`Drainer.Step` requests moves as the package `Step` does). A volume not held, with no move
  pending and a blocked chunk on the node, is #17's state `waiting`, its why `no destination
  (LEVEL/POLICY: REASON)` from `BlockedVolumes`, not `no move pending`: #13's reason is not
  given for it.
  `Run` holds #17's `Drainer` and checks `BlockedError` right after `Drainer.Step`, before #78's
  `CheckDrain`.
- **With issue #45** (whichever of #45 and #18 lands second): the rebuild takes #18's choice
  (#45's S11), so a chunk with a staying replica unknown at its level gets no destination. #45's
  S15 "no destination: no eligible active node, or under `strict` every one collides" becomes
  "... or under `strict` every one is refused by #18's S5", and gives `domain.Strict`'s reason
  (`replica "x" has no zone label`). #45's text needs the change (not made here).
- **`Progress`** gains `Blocked, Refused int` (`Refused` is part of `Blocked`) and
  `BlockedVolumes []BlockedVolume` (`ID, Level, Policy, Why string; Chunks int`, by listing
  order; `Why` the reason above). A volume deleted mid-step adds nothing, as today.
- **`Progress.String()`** appends, right after the counts (`N chunk replicas left, M moving`),
  when `Blocked > 0`: `; 6 blocked (no destination): v1 (zone/strict, 4 chunks: replica "c" has
  no zone label), v3 (node/strict, 2 chunks: no eligible node)`, and, when `Refused > 0`, `; 1
  refused by the service, tried again`. A word the drain does not know is shown with `%q`: `v9
  ("x"/"y", 2 chunks: failure domain or policy not known to the drain)`. Its place does not
  depend on `Unattached`, which #17 removes.
- **`Run`**, after `report(p)`: `Remaining > 0` and `Remaining == Blocked - Refused`: returns
  `*BlockedError{Node, Volumes}`. Else `Remaining == 0`: nil (once #78 lands, its `CheckDrain`
  instead, success only on `complete`). Else it waits and steps again. So pending moves (of a
  volume with a target or one waiting for one), requests not applied, refused proposals and
  #13's waiting chunks keep it going. A volume waiting for a target keeps it going only through
  a pending move: one whose chunks are all blocked ends it, as `TestDrain`'s `v2` does. The
  condition is on these counts alone, not on `Unattached`.
- **`BlockedError.Error()`:** `drain of b cannot finish: 6 chunk replicas have no destination:
  v1 (zone/strict, 4 chunks: replica "c" has no zone label), v3 (node/strict, 2 chunks: node d
  has no current capacity report); b stays draining (undrain clears it)`: each volume with its
  reason, as `String()` gives it. A refusal's message is used verbatim, in its own plan's form
  (#72's ids unquoted); #18's own reasons quote with `%q`.
- **`flaminio-meta drain`:** no code change: stderr `flaminio-meta: <that>`, nothing on stdout,
  exit 1. The usage comment says so.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): S2, S16.
- `meta/domain/domain.go` (with `Place`), `domain_test.go`: S1, S3, S4, S5.
- `meta/service/service.go`, `service_test.go`, new `meta/service/placement_test.go`: S2, S5–S9,
  the invariant.
- `control/drain/drain.go`, `drain_test.go`: S10–S14.
- `meta/cmd/flaminio-meta/main.go` (usage comment), `main_test.go`: S2, S13, S16.
- `docs/replication-protocol.md`, `docs/lab.md`, `docs/followups.md`, `docs/ARCHITECTURE.md`
  (§18, §19): S16.
- `scripts/lab.sh`, `Makefile`: the lab check (the later step).
- **Not touched:** `meta/service/auth.go` (no new method), `dataplane/`, `meta/testdata/`,
  `control/csi/`, the on-disk format.

## Contracts

- **Proto, `Volume`:** `string failure_domain = 18;` ("node", "rack" or "zone": the level its
  creation asked for, not one a bestEffort placement fell back to; "node" for a volume created
  before it was recorded) and `string failure_domain_policy = 19;` ("strict" or "bestEffort").
  Never empty from this service. No other message or RPC changes; comments change (below). 9
  and 10 are #79's, 11–14
  #70's, 15 #13's, 16–17 #17's; 18 and 19 no plan of the batch claims. `pb.rs` names neither
  (tags 1–8, below); if it ever does, tags 18 and 19.
- **Proto comments:** `MoveReplica`: refused with `FAILED_PRECONDITION`, its message starting
  `failure domain collision: `, when the volume is strict and the destination shares a domain at
  its level with a replica that stays, or, at rack or zone, the destination or a staying replica
  has no label there; `MoveReplicaResponse.applied` unchanged. `CreateVolumeRequest`:
  `failure_domain_policy`, "strict" (default) refuses if the topology is short and, at rack or
  zone with two replicas or more, places no replica on a node without that label;
  `prefer_node`, a preferred node without that label is then ignored.
- **Stored header:** the two JSON keys above, strings, omitted when empty; absent means `node`,
  `strict`.
- **`MoveReplica` answers:** `InvalidArgument`, `NotFound`, `applied=false`, `Aborted` as today;
  `Internal` for an unknown stored word. `FailedPrecondition` for a collision, its message
  starting with the fixed text `failure domain collision: ` (the refusal above).
  `FailedPrecondition` also carries other causes once other plans land: issue #72's destination
  without a current capacity report or a readable ledger (`node <to> has no …`), issue #78's
  draining destination (`destination "b" is draining`); `ResourceExhausted` issue #72's
  destination without room. The drain tells a collision by the code and the prefix
  `domain.ErrCollision.Error() + ": "`, and treats every other `FailedPrecondition` and
  `ResourceExhausted` by #72's rule (the drain).
- **`CreateVolume` answers:** as today, but under `strict` at `rack` or `zone` with two replicas
  or more no replica goes on a node without the label; too few domains left: `FailedPrecondition`
  (`ErrPlacement`), today's message and, last, `; N nodes without a zone label left out` (`rack
  label` at `rack`), on that refusal only; the CSI controller passes it on as
  `ResourceExhausted` (`control/csi/controller.go:248-249`).
- **Go:** `domain.Collision`, `domain.Strict` (a bare reason string), `domain.ErrCollision`,
  `domain.Place` (the filter), `domain.LeftOut` (with the unexported `leftOut`),
  `Header.Placement` as above; `drain.Progress`,
  `drain.BlockedVolume` (with `Why`), `drain.BlockedError` as above.
- **CLI:** `get`, `create`, `attach` print `failureDomain` and `failureDomainPolicy`; `move`
  refused: exit 1, `flaminio-meta: rpc error: code = FailedPrecondition desc = …`; `drain`: the
  progress line and the final error above, exit 1.
- **Control socket, wire frames, on-disk records:** none.

## Tests

- **S1, S3, S4** (`domain_test.go`, unit): `NewVolume` records each level and policy, defaults
  as `node`, `strict`; a header JSON without the keys parses, gives `node`, `strict`, and
  marshals without them; an unknown word fails `Placement`. `TestVectors` unchanged.
- **S5** (`domain_test.go`, `TestCollision`, `TestStrict`): per level; `Collision`: unlabelled
  nodes share one domain; `Strict` at `rack` and `zone`: a destination without the label or with
  an empty one refused, a staying replica without it refused, each with its exact bare reason
  (no `failure domain collision` in it), an unknown staying replica named before an unknown
  destination; at `node`, only a shared id; the first staying replica in order is named; no
  staying replica: empty.
- **S5, creation** (`domain_test.go`, `TestPlaceStrictLeavesOutUnlabelled`): under `strict` at
  `zone` and at `rack`, two replicas, an unlabelled node and one with an empty label, both with
  load 0, get no replica of any chunk; with only one labelled domain besides them,
  `ErrPlacement`, and `LeftOut` gives `; 2 nodes without a zone label left out` at `zone` and `;
  2 nodes without a rack label left out` at `rack`; `LeftOut` empty when none is left out, when
  the labelled domains suffice, when there are fewer live nodes than replicas, under
  `bestEffort` and at `node`; a preferred unlabelled node is ignored; one replica, and
  `bestEffort`, may use them. `TestLeftOutPredicate`, a table of node sets over level, policy,
  replicas (1, 2, 3) and each node's label (set, empty, absent), with three assertions per row:
  `leftOut` for each node matches the row's expected value; `LeftOut` is non-empty exactly when
  `Place` returns `ErrPlacement` and some node is left out, at least `replicas` nodes are in all,
  and fewer than `replicas` domains remain among the nodes kept; and, when non-empty, the count
  it gives equals the number of nodes for which `leftOut` is true, with the level's label named.
  `placement_test.go`: a node registered with an address only gets no replica of a new
  `zone`/`strict` volume of two replicas, and gets one once labelled; a creation refused for too
  few labelled domains: `FailedPrecondition`, its message ending with the suffix, at both
  levels; one refused for too few live nodes: no suffix.
- **S1–S3** (`placement_test.go`, `stores`): create, get and attach report what was asked,
  defaults included; a `zone`/`bestEffort` volume created with every node in one zone and two
  racks (placed at `rack`, as `TestPlaceBestEffortFallsBack`) reports `zone`, `bestEffort` after
  create, get and attach; the claim's header carries the words (`interleaveKV` on the volume
  key); a header rewritten without the keys reports `node`, `strict`, takes a move into a staying
  replica's zone, and is still without the keys after an attach.
- **S5–S7** (`placement_test.go`, `stores`, zones and racks as `TestFailureDomainPlacement`):
  per level, a colliding destination: `FailedPrecondition`, the exact message (its fixed
  start, ids and label values quoted; a label holding `"` and a newline comes out escaped), no
  `moving` recorded; the same under `bestEffort`: applied; the source's own domain: applied;
  under `strict` at `rack` and `zone`, each refused with its message: an unlabelled destination,
  one whose label is set to the empty value, a staying replica without the label, one without a
  node record (deleted from the store), one whose record does not parse. The volume is created on
  labelled nodes (creation leaves unlabelled ones out), then the staying replica's node is
  changed: an empty value through `RegisterNode`; an absent label, a deleted or an unparsable
  record by writing the store directly, as `RegisterNode` merges labels and cannot remove one
  (`service.go:197-200`). Under `bestEffort`, an
  unlabelled destination beside a labelled replica: applied; a chunk already colliding moves
  where it adds none; one replica: any destination, unlabelled included; an unknown stored word:
  `Internal`. `TestMoveAndDrain` unchanged.
- **S8** (`placement_test.go`, `interleaveStores`, the hook on a staying replica's node key):
  the hook gives that replica the destination's zone: refused, nothing recorded; the hook
  changes the destination's zone to that replica's: refused; the hook sets the destination's
  zone to the empty value: refused; the hook registers a missing record with a zone: read again;
  the hook changes an address only: applied.
- **S9** (`placement_test.go`): a recorded move completes through `ChangeIsr` after its
  destination is relabelled into a staying replica's zone.
- **The invariant** (`placement_test.go`, memory store, `math/rand/v2`, the fixed seeds 1–64,
  each a forced step and then 40 random steps): 4–8 nodes, built so that every seed admits a
  volume. The seed draws a level, `rack` or `zone`, for its first volume; the generator first
  gives three nodes distinct labelled zones and distinct labelled racks, so either level has at
  least replicas + 1 = 3 labelled domains for that volume (`strict`, 2 replicas), then draws the
  other 1–5 nodes in random zones and racks, some unlabelled. The first volume is always created;
  the test fails if it is not. More `strict` volumes, 2 or 3 replicas, at `rack` or `zone`, are
  created only where their level has at least replicas + 1 labelled domains. After creation: no
  chunk has a replica on a node unlabelled at its level.
  - **The forced step,** first on every seed, on the first volume's chunk 0: clear its first
    replica's node's label at the volume's level (zone for `zone`, rack for `rack`) to the empty
    value, and move its second replica by hand to a labelled node outside the chunk whose domain
    no staying replica's labelled domain shares; the chunk holds two of the first three nodes,
    so at least one of the three is outside it and is such a node (R6-B1). The test asserts on
    every seed that the step ran (the chunk existed and the move was requested) and that the
    service refused it with `FailedPrecondition`, its message starting with
    `domain.ErrCollision.Error() + ": "` and naming the cleared replica's node (R6-B2); with
    `skipCollisionCheck` or `skipStayingCheck`,
    that it was accepted and the "beside" clause below failed. So each injected bug is caught
    on every seed, not by chance.
  - **Random steps:** moves by hand and `drain.Step` through a localhost gRPC server, each
    accepted move completed as `completeMoves` does; and label changes: pick a volume and a node
    holding one of its replicas, and clear that node's label at the volume's level to the empty
    value (`RegisterNode`), or restore a cleared label to its former value. A label of another
    level would change nothing for that volume, so none is cleared.
  - **After each step:** the replicas of every chunk that are on labelled nodes are in distinct
    domains at its level, and no accepted move of a chunk with a staying replica went to, or
    beside, a node unlabelled at that level when it was accepted. Clearing and restoring never
    put two labelled replicas of a chunk in one domain, so only a move can: while a node is
    cleared, S5 refuses every move of the other replicas of its chunks and every move to it, so
    when its former label comes back, no replica of those chunks has entered that domain; only
    its own replica may have left. The argument rests on S5: with S5 weakened, it fails, and so
    does this check.
- **S10–S14** (`drain_test.go`, in-memory service): the least loaded allowed node, not the least
  loaded; under `strict` at `zone`, a new unlabelled node with load 0 not proposed, a labelled
  one chosen, and with only unlabelled ones left the chunk blocked; `bestEffort` zone to rack to
  node, two chunks of one volume at different levels; a `strict` volume blocked while another
  drains, `Progress` and its string; a volume whose stored words the drain does not know (a fake
  service answering `"x"`, `"y"`): its chunks blocked, shown with `%q`, the other volumes moved,
  `Run` ends with `BlockedError` naming it; `Run` returns `BlockedError` naming the volumes, each
  with its reason, the node still draining; pending moves keep `Run` going, an unattached volume
  whose chunks are all blocked does not; a collision refusal from an interceptor
  (`FailedPrecondition`, the marker): blocked and refused, the destination still proposed for the
  other chunks, which move, `Run` does not end on it; a `FailedPrecondition` without the marker,
  and a `ResourceExhausted`, from an interceptor: not counted refused, the destination taken out
  for the step's later chunks and volumes, the chunk proposed to the next allowed node, and with
  none left blocked, the step not failed, the volume's `Why` the last refusal's message in
  `String()` and in `BlockedError` (reason 1); a collision faked on a chunk's least loaded node
  while `Strict` refuses its other candidate: `Why` `Strict`'s reason for that candidate, not the
  refusal (reason 5); a second chunk, of the same volume and of a later volume, whose only
  usable node c the first chunk's #72-style refusal took out: blocked, its `Why` exactly `every
  candidate refused earlier in this step: c: node c has no current capacity report`, naming its
  own candidate, not the first chunk, and not `no eligible node` (reason 4); `TestDrain`'s
  "nowhere to go": no error, `Blocked == Remaining == 8`, `Why` `no eligible node` (reason 3).
- **An unknown staying replica** (`drain_test.go`, R2-C1): a `zone`/`strict` volume created on
  two labelled nodes, then one of them set to the empty zone (`RegisterNode`), the other
  drained: an interceptor counts no `MoveReplica` request, `Blocked` 1 per chunk, `Refused` 0,
  `Why` `replica "x" has no zone label` (reason 5), and `Run` returns `BlockedError` (it does
  not run to its time-out). Again with that node's record deleted from the store, so that
  `ListNodes` does not list it. A drain that checked staying replicas with `Collision` would
  propose a node, count the service's refusal as refused, and fail this test.
- **A real refusal** (`drain_test.go`, R2-C2): nodes a (z1), b (z2, drained), c (z3), d (z4); one
  volume on a and b, one on d and b (each created while only its two nodes are live; c added
  after). An interceptor, before the real `MoveReplica` handler of the first request, relabels c
  into z1 through the service's `RegisterNode`. The first volume's chunks: refused by the
  service, each counted refused, the volume's `Why` the service's message (reason 1: d is left
  and passes in the drain's listing); the other volume's
  chunks still go to c. This fails if the drain's marker and the service's message drift apart.
- **With issue #13** (`drain_test.go`, whichever lands second): a releasing node is the least
  loaded and allowed: the next allowed node is chosen; the only allowed node releases the chunk:
  no request, `only nodes releasing it are left` in `Progress.Held`, `Blocked` 0, `Run` goes on;
  a releasing node that the domain filter refuses as well: blocked.
- **With issue #17** (`drain_test.go`, whichever lands second): a volume whose chunks on the node
  are all blocked: `VolumeState` `waiting`, why `no destination (zone/strict: <reason>)`; a
  `Drainer`-based `Run` returns `BlockedError`.
- **S2, S13, S16** (`main_test.go`): `get` prints the two fields; `move` refused: exit 1 and
  the code; `drain` of a blocked node: the error on stderr, nothing on stdout; the usage text.
- **S15** (`auth_test.go`, existing): `MoveReplica` and `DrainNode` stay an administrator's.
- **Targets:** `make lint test race contracts`. `contracts` regenerates `api/gen` and the
  vectors with no diff in `meta/testdata`, and runs the Rust client against the Go service: its
  volume `v` now carries fields 18 and 19 set, which the unchanged client must skip. `sim`,
  `crash-test`, `miri`, `sanitize`: nothing they cover changes.

## Lab

The later step: `make domain-drain-test` (`lab.sh domain-drain-test`), after `make meta-up` and
`make data-node-up`. Volumes `dz18s`, `dz18b`: 1G, 2 replicas, 256M chunks, prefer fl-a.
1. **Start:** exit 1, naming them, if `metactl volumes` lists any volume (`make vol-delete`
   first): its replicas on fl-b would move too and, unattached, keep the drain waiting. Once
   issue #13 lands, it also dies at once unless `DELETE_RELEASED=1`, as #13's `drain-test`
   does, and restarts fl-b's and fl-c's nodes through `node_env` so that they delete with it:
   step 3's move back (`drain fl-c`) moves chunks fl-b released, whose only free node is fl-b,
   and they wait until fl-b deletes.
2. **Labels:** fl-a and fl-b as `meta-smoke` sets them; fl-c's zone set to `lab-c`, a zone of
   its own (an unlabelled fl-c joins no other replica of a `strict` chunk, S5).
3. **`zone`/`strict`:** fl-c marked draining (`drain fl-c`: it holds nothing), `dz18s` created,
   `undrain fl-c`; `get` shows `zone`, `strict`, every chunk on fl-a and fl-b. Attached on fl-a.
   `drain fl-b`: exit 0, every chunk on fl-a and fl-c. `undrain fl-b`, `drain fl-c` (back to
   fl-b), `undrain fl-c`.
4. **Blocked:** once issue #13 lands, first wait, 60 s at most, until `metactl releases fl-c`
   is empty, as #13's `drain-test` does; still not empty: exit 1, `fl-c still holds releases
   after 60 s`. Otherwise #13's `CheckMove` answers the hand move below `applied=false` (`node
   fl-c releases chunk 0`) and exit 0, whatever the timing. fl-c registered with zone `lab-a`.
   `drain fl-b`: exit 1, stderr names `dz18s (zone/strict, 4 chunks: destination "fl-c" shares
   zone "lab-a" with replica "fl-a")`; no chunk has a move pending. `move dz18s 0 fl-b fl-c`:
   exit 1, `FailedPrecondition`, `shares zone "lab-a" with replica "fl-a"`. `undrain fl-b`.
5. **`bestEffort`:** `dz18s` deleted; `dz18b` created the same way with `bestEffort`, attached;
   `drain fl-b`: exit 0, every chunk on fl-a and fl-c (racks differ). `undrain fl-b`.
- **Deletes:** its two volumes only, through `delete_volume`; replicas on fl-c need
  `lab.sh vol-confirm` (issue #14) until issue #13 lands; with #13, `vol-confirm` is gone and
  the nodes' sweeps delete and confirm (#13's lab harness). It formats nothing.
- **Restored after,** also when it fails (a trap): fl-b and fl-c undrained, fl-c's zone empty.
  Then by hand: `make vol-create`, `make vol-attach`, and `make drain-test` passes as before.

## Docs

- **`docs/replication-protocol.md` §7, moves:** the request's bullet gains the refusal, what it
  compares, and that `ChangeIsr` never looks at domains.
- **`docs/lab.md`, drains:** step 2 (how a destination is chosen, the fall-back), blocked
  chunks, the progress line, the exit, what to do (add or relabel a node, or `move` by hand
  under `bestEffort`; `undrain`); under `strict` at `rack` or `zone`, no move, drain or
  creation puts a replica of a chunk of two replicas or more on a node without that label, so
  label a new node before it is needed; a chunk with a replica on a node that lost its label
  cannot have its other replicas moved: label the node again, or move its replica first; every
  `flaminio-meta` updated (`make meta-up`) before a `rack` or `zone` volume is created or moved,
  and the rollback rule below (Q5); an upgrade note: where `rack` or `zone` `strict` classes are
  in use, label every storage node before upgrading, or creations that counted unlabelled nodes
  as a domain are refused after it; `make domain-drain-test` in the targets table.
- **`docs/ARCHITECTURE.md` §8.2, the `failureDomainPolicy` row:** `strict` refuses placement if
  the topology is short, and at `rack` or `zone`, for two replicas or more, counts a node without
  that label as no domain and places nothing on it.
- **`docs/ARCHITECTURE.md` §18, upgrades, and `docs/lab.md`:** beside #30's rule, once a `rack`
  or `zone` volume exists the metadata service is not rolled back below this version. It is an
  operating rule that nothing enforces: an older instance cannot know it. How to check, with no
  creation running (§18 already forbids creations during a downgrade): `for v in $(flaminio-meta
  volumes | jq -r '.ids[]?'); do flaminio-meta get "$v" | jq -r '.volume | select(.failureDomain
  == "rack" or .failureDomain == "zone") | .id'; done` (with its connection flags; `metactl` in
  the lab); any id printed means the rule applies. A rollback anyway strips the level and policy
  of every volume attached meanwhile (an agent's restart, a CSI publish), silently and for good:
  after the upgrade they read as `node`, `strict`, like a volume created before this change, and
  a later drain may put a chunk's replicas in one zone. Recreating the volume is the only remedy
  (Q3(b), a command to set them, does not exist).
- **`docs/ARCHITECTURE.md` §19, the upgrades row:** "N → N-1 rollback before finalize" holds for
  this release with creating the first `rack` or `zone` volume as its finalize step: rollback is
  tested before that, not after.
- **`docs/followups.md` §4:** the "Left" item done, with the lab check's date and result once it
  has run; the table's row; step 2 (placement) gains the same sentence as §8.2's row.
- **`meta.proto`:** the comments of Contracts (`MoveReplica`, `Volume`'s new fields,
  `CreateVolumeRequest.failure_domain_policy` and `prefer_node`).
- **`README.md`, `docs/performance-plan.md`, benchmarks:** nothing.

## Risks

- **Safety:** the rule is enforced only at `MoveReplica`; it holds because `ChangeIsr` carries
  out recorded moves only (`validMove`). No epoch, fence or durability rule changes.
- **Security:** who may move or drain is unchanged. Labels are trusted, as at creation, and
  now gate every `strict` move. Who may set any label of a node (`RegisterNode`,
  `auth.go:71-72`): every administrator identity, for any node, which includes the CSI
  controller's `admin/csi-controller` certificate (`scripts/lab.sh:1004`) as well as the
  operator's; the node's `agent`; and the node's `io`, for its own node, though the data plane
  never sends labels (`dataplane/crates/flaminio-node/src/engine.rs:585-593`). A node can so
  attract `strict` replicas by claiming a new zone (not an empty one: S5 refuses unknown
  domains), or repel them and block drains by claiming a staying replica's zone or by clearing
  its own label (below). Creation trusts the same writers. Refusing label changes from `io`
  would narrow this; it is left to its own issue, not filed by this plan.
- **Security, unlabelled nodes:** a new node registers its address before its labels arrive
  (`engine.rs:585-593`; the CSI node plugin's `SyncTopologyLabels`, `control/csi/topology.go`,
  which skips a label Kubernetes lacks, so such a node stays unlabelled), and would otherwise be
  a zone of its own with load 0, first in a drain's and a creation's choice. S5 refuses it under
  `strict` until it is labelled, at a move and at creation, and refuses a move beside a staying
  replica without a label or a readable record.
- **Blocked by a lost label:** a chunk with a replica on a node that has lost its label (its
  `agent` or `io` may set it to the empty value: labels merge, `service.go:197-200`) or its
  record cannot have its other replicas moved under `strict` at `rack` or `zone`: by hand, by a
  drain (blocked, reason `replica "x" has no zone label`) or by issue #45's rebuild, which so
  cannot restore that chunk's redundancy. A node can cause it on purpose. Remedy: label the node
  again, or move its replica first, which S5 allows. Fail-closed is chosen over a replica pair
  in one zone; the risk is named, not accepted (below).
- **Compatibility:** an older `flaminio-meta drain` against the new service takes a refusal for
  a failed step. A node's address change during a move costs one more attempt. A `rack` or
  `zone` `strict` class whose creations counted unlabelled nodes as one domain is refused after
  the upgrade (`FailedPrecondition`, `ResourceExhausted` through the CSI controller,
  `controller.go:248-249`), with no warning before: `docs/lab.md`'s upgrade note says to label
  every storage node first.
- **Finding, S13 against S14:** S13 ends the drain once every replica left is blocked; S14 says
  a refused proposal is no failure. Designed: refused chunks do not end the drain; the next
  step's listing sees the labels and blocks them itself. Labels flapping every step run it to
  its time-out.
- **Finding, the invariant's test:** "with the check removed" needs a switch in the service
  (`skipCollisionCheck`, unexported). Removing the drain's own filter is not caught by it: the
  service still refuses.
- **Finding, the lab check:** vol1's replicas on fl-b move with any drain of fl-b, so the check
  runs with no other volume (step 1), and its deletions need `vol-confirm` for fl-c until #13
  lands, `DELETE_RELEASED=1` after (step 1).
- **Open, Q1 (b):** with (a), `stepVolume` keeps its error and `Progress` is unchanged; with
  (c), `Run` never returns `BlockedError` and ends at the time-out.
  Settled at approval, 2026-11-13: (b) (see decisions.md, Settled at approval).
- **Open, Q2 (a):** with (b), `MoveReplica` lists nodes, liveness and drain marks, and compares
  them all: a far wider transaction.
  Settled at approval, 2026-11-13: (a) (see decisions.md, Settled at approval).
- **Open, Q3 (a):** with (b), a new administrator's RPC, its authorization, the Rust client's
  contract run; with (c), `Placement` infers from chunks and node records.
  Settled at approval, 2026-11-13: (a) now; (b) later (see decisions.md, Settled at approval).
- **Open, Q4 (Go only):** otherwise the Rust model gains labels, the vectors change, and
  `make meta-vectors` and `sim` join the targets.
  Settled at approval, 2026-11-13: Go only (see decisions.md, Settled at approval).
- **Open, Q5 (a):** with (b), headers are read and written as raw JSON maps.
  Settled at approval, 2026-11-13: (a) (see decisions.md, Settled at approval).
- **Accepted:** existing `rack` and `zone` volumes get no protection (Q3); an older instance's
  attach during an upgrade drops the record (Q5); a broken spread is not repaired (intent).
- **Named, not accepted:**
  - a rollback below this version once a `rack` or `zone` volume exists strips the level and
    policy of every volume attached meanwhile, silently and for good (Q5, changed after
    approval, R1-S2, R2-S1). The no-rollback rule is documented, not enforced;
  - a node that clears its own label, or loses its record, blocks every move, drain and #45
    rebuild of the other replicas of its chunks under `strict` at `rack` or `zone` (Blocked by a
    lost label, above; R2-B1, R3-S1).

## ADRs

- **0002 (etcd, the narrow `kv` interface):** relied on: revision compares, `Rev: 0` for an
  absent key, both stores. Consistent.
- **0001 (Go control plane, Rust data plane):** consistent; the rule is Go only (Q4).
- **No new ADR:** no dependency, protocol or format rule is added.

## Parallelizable?

There is no Rust unit. The Go units, yes, in this order: the proto fields with `make proto`,
and `meta/domain` (`Header`, `Placement`, `Collision`), first; then the service and the drain,
each against the contracts above. The drain's tests fake the refusal with an interceptor; only
the invariant's test needs both.
