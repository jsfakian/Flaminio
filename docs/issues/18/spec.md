# Issue #18: Moves and drains ignore a volume's failure domain — specification

Both steps of the issue ([intent](intent.md)). Settled choices and accepted risks: decisions
(after approval). How: design (after approval). Rules marked (Q*n*) follow the recommendation of
an open question.

## Terms

- **Header:** the metadata service's record of a volume, in etcd. Not the on-disk format.
- **Level:** `node`, `rack` or `zone`, lowest first. **Policy:** `strict` or `bestEffort`. A
  volume's level and policy are those its creation asked for.
- **A node's domain at a level:** as at creation. Its id; its `flaminio.io/rack` label; its
  `topology.kubernetes.io/zone` label. Nodes without the label share one domain at that level.
- **Unknown at a level** (`rack` or `zone`): a node without the label at that level, or with an
  empty one; for a staying replica, also one whose node has no record or a record that does not
  parse. Under `strict`, an unknown node is no domain of its own (S5).
- **Move:** a `MoveReplica` request for one chunk, from a source to a destination. The chunk's
  **staying replicas** are its replicas other than the source.
- **Collides at a level:** the destination is in the domain of a staying replica at that level.
- **Eligible:** a node the drain may propose, as today: registered, live, not a witness, not
  draining, not the drained node, without a replica of the chunk; once issue #13 lands, also not
  a node that releases the chunk (#13's `released`).
- **Blocked chunk:** a replica on the drained node with no move pending and no eligible
  destination the rules allow. A chunk whose only nodes the rules allow release it (#13) waits
  instead, as a pending move does: it is not blocked.

## The invariant

Under `strict`, the metadata service never records a move whose destination collides at the
volume's level, nor, at `rack` or `zone`, a move of a chunk with a staying replica when the
destination or a staying replica is unknown at that level. A chunk whose replicas are in
distinct domains therefore stays so through every move, as long as its nodes' labels do not
change. The request is the only place a destination is accepted: `ChangeIsr` carries out
recorded moves only. The rule asks no more than that: a chunk
that already has two replicas in one domain may still move, where it adds no collision.

## Behaviour

### The record

**S1. Recorded at creation.** The header keeps the level and the policy the creation asked for,
defaults included; not the level a `bestEffort` placement fell back to. They are written with
the header's first write and never change.

**S2. Reported.** Every answer that carries a volume (create, get, attach) carries its level and
policy, in the request's words. `flaminio-meta get` shows them.

**S3. Volumes without a record (Q3).** A header written before this change reads as `node`,
`strict`: its moves are accepted and chosen as today. No header is rewritten to add the record.

**S4. The stored format only grows.** A header parses with or without the record. The chunk
records, the transition vectors (`meta/testdata`) and the data plane's client are unchanged.

### Accepting a move

**S5. `strict` refuses a collision, and an unknown domain.** A move of a `strict` volume is
refused, for every caller, when its destination collides at the volume's level, or, at `rack` or
`zone`, when the destination or a staying replica is unknown at that level: an unlabelled node
joins no staying replica until it is labelled, and an unlabelled replica lets none join it. The
source does not count: it leaves. A chunk with one replica has no staying replica, so any
destination passes.

Creation keeps the same rule: under `strict` at `rack` or `zone`, a volume of two replicas or
more gets no replica on a node unknown at that level; with too few labelled domains left, it is
refused as a short topology is today; when too few labelled domains cause that refusal, its
message counts the nodes left out by the label they lack, and no other refusal does. A
one-replica volume, and `bestEffort`, place as today. No chunk of a volume that records `rack`
or `zone` was placed before this rule: the record and the rule come in one change, and a volume
without the record reads as `node` (S3). A chunk has a replica on an unknown node only when
that node loses its label after placement (its `agent` or `io` may set it to the empty value) or
its record. Its other replicas then cannot move, by hand, by a drain or by issue #45's rebuild,
until the node is labelled again or its own replica is moved first, which this rule allows.

**S6. Nothing else is refused (Q2).** A `bestEffort` volume's move is accepted as today:
preferring the higher level is the proposer's work (S10). At level `node` the rule adds nothing:
a destination that holds a replica is refused already.

**S7. The refusal is an error, and says why.** It is of the class a creation gets when the
topology is short (a failed precondition), and its message starts with a fixed text that tells
it from the other failed preconditions of a move (issue #72's destination without a capacity
report, issue #78's draining destination). It names the volume, the chunk, the level, the
destination, and the staying replica and domain it collides with, or the node that is unknown;
ids and label values quoted. Nothing is written. Today's refusals (an unregistered destination;
a request that is not applied) keep their answers.

**S8. Decided on current labels, in one conditional write.** The check uses the labels of the
destination and of the staying replicas as the request reads them. The move is recorded only if
the chunk, the header and those nodes' records are still as read; otherwise the request reads
again. A label change that completed before the request began is always seen. A staying replica
whose node has no record, or a record that does not parse, is unknown (S5).

**S9. Carrying a move out is unchanged.** `ChangeIsr`, the target and the replication model do
not look at domains. A move recorded before its nodes' labels changed, or before this change,
completes as recorded.

### The drain

**S10. The choice.** For each replica on the drained node without a pending move, among the
eligible nodes:
- **`strict`:** only those S5 allows: no collision at the volume's level, and, at `rack` or
  `zone` with a staying replica, neither the node nor a staying replica unknown;
- **`bestEffort`:** those that do not collide at the highest level, from the volume's down to
  `node`, at which any such node exists. The fall-back is per chunk, and only as far as needed;
- **then** the least loaded, and the lowest id among equals, as today.

**S11. A blocked chunk does not stop the step (Q1).** The step still requests every move it
can, for that volume and the others. A chunk with no eligible node at all is blocked too: today
it fails the step. So are the chunks of a volume whose stored level or policy the drain does not
know. A blocked chunk is tried again at every step: nodes and labels may change.

**S12. The drain reports blocked chunks.** Each step's progress gives their number and, for each
volume that has some, its level and policy (as stored, when the drain does not know them) and
why its first blocked chunk has no destination, the first that applies of: its own refusal
(S14, the last one), unless S5 refuses a node still left to it; words it does not know; no
eligible node; its own candidates, each with the refusal that took it out earlier in the step;
the reason S5 gives (a staying replica unknown named first). The final error names the same. Once
issue #17 lands, a volume whose chunks left on the node have no move pending and some are
blocked is reported as waiting for that reason, not for #13's releases.

**S13. A drain with blocked chunks never succeeds (Q1).** It succeeds only when the node holds
no replica, as today (once issue #78 lands, only when #78's check says the drain is complete).
Once every replica left on the node is blocked, it ends with an error that names the volumes.
While moves are pending, it goes on, as today, whether their volume has a target or waits for
one; a volume waiting for a target whose chunks are all blocked keeps nothing going. The node
stays marked draining after a failure, as today; `undrain` clears it.

**S14. A proposal the service refuses (S5),** because labels changed since the drain listed
them, makes that chunk blocked for the step. It is not a failure of the drain, and the
destination stays a candidate for other chunks. A proposal refused for another cause (issue
#72's destination without room or a capacity report, issue #78's draining destination) takes
that destination out of the step's choice, for every chunk, and S10 chooses again; with none
left, the chunk is blocked, its reason the last refusal (S12).

### Unchanged, and docs

**S15. Unchanged.** Creation: `Place`, its refusals and its fall-back, but for S5's unknown
nodes. Who may call what:
`MoveReplica` and `DrainNode` stay an administrator's. This change adds no check of a
destination named by hand other than S5: a draining one is refused once issue #78 lands (its
S5), and a dead one, or one without a capacity report, once issue #72 lands; S5 composes with
both. The replication protocol's move steps, the simulator and the on-disk format.

**S16. Docs.** `docs/replication-protocol.md` §7 (moves): when a request is refused.
`meta.proto`: the comments of `MoveReplica`, of the volume's new fields, and of a creation's
policy and preferred node (S5's creation rule). `docs/lab.md` (drains): how a destination is
chosen, what a blocked chunk is, what to do; and an upgrade note: where `rack` or `zone`
`strict` classes are in use, label every storage node before upgrading.
`docs/ARCHITECTURE.md` §8.2 (the policy's row) and follow-ups §4 step 2: S5's creation rule.
Follow-ups §4: the "Left" item done, with the lab check's result once it has run.
`flaminio-meta`'s usage text, if the drain's output or exit changes. `docs/ARCHITECTURE.md`
§18 and `docs/lab.md`: the metadata service is not rolled back below this version once a
`rack` or `zone` volume exists (Q5), an operating rule that nothing enforces; how to check
whether one exists; and that a rollback anyway strips the level and policy of every volume
attached meanwhile, silently and for good. `docs/ARCHITECTURE.md` §19: for this release,
creating the first `rack` or `zone` volume acts as the finalize step.

## Tests

- **S1–S4 (unit, service):** a created volume reports what was asked, defaults included, after
  a get and an attach; a `zone`/`bestEffort` volume placed at `rack`, its nodes all in one zone,
  still reports `zone`; a header stored in the old form reads as `node`, `strict`, and survives
  an attach and a move. `make contracts`: the generated API code matches, the vectors do not
  change, the Rust client still talks to the Go service.
- **S5–S7 (unit, service, on both stores):** per level, a colliding destination refused under
  `strict` and accepted under `bestEffort`; a destination in the source's own domain accepted;
  under `strict` at `rack` and `zone`, a destination without the label or with an empty one,
  and a staying replica without it, without a record or with one that does not parse, refused;
  under `bestEffort`, unlabelled nodes share one domain; a chunk already colliding still moves
  where it adds none; the error's class, its fixed text and what it names; today's refusals
  unchanged. Creation: a `strict` volume of two replicas at `rack` and at `zone` gets no replica
  on a node without the label or with an empty one, though it is the least loaded, and is
  refused when too few labelled domains are left, that message alone naming the rack or zone
  label;
  a one-replica volume and a `bestEffort` one place there as today.
- **The invariant (unit, property):** random topologies, some nodes unlabelled, each built to
  admit at least one `strict` volume; `strict` volumes created where enough labelled domains
  exist (then no replica is on an unknown node),
  and steps: moves, by hand and by the drain, and labels at the volume's level cleared after
  creation and restored. After every step, each chunk's replicas on labelled nodes are in
  distinct domains, and no accepted move of a chunk with a staying replica went to, or beside,
  an unknown node. With the check of S5 removed, or only its check of the staying replicas, the
  test must fail on every one of its fixed seeds (a forced step reaches the case on each).
- **S8, S9 (service):** a label change between the request's read and its write makes it read
  again, and refuse; a recorded move completes through `ChangeIsr` after a label change makes
  its destination collide.
- **S10–S14 (drain, against an in-memory service):** the least loaded allowed node, not the
  least loaded; an unlabelled node, the least loaded, skipped under `strict`; the fall-back zone
  to rack to node, per chunk; a `strict` volume blocked while other volumes drain; a volume with
  a stored word the drain does not know blocked while the others move; the progress and the
  final error, each blocked volume with its reason; a staying replica whose node lost its label,
  or is not listed: no request made, blocked and not refused, the drain ends with the error; a
  refusal of the drain's own proposal, one faked and one real (the destination relabelled
  through the service just before the request); a refusal for another cause, the chunk moved to
  the next allowed node, and with none left blocked with the refusal as its reason; a later
  chunk whose only usable node an earlier chunk's refusal took out, blocked with its own
  candidate and that refusal as its reason; a refused chunk whose other candidate S5 refuses,
  with S5's reason; no eligible node at all (today's `TestDrain` case, with its new result).
  Once #13 lands: a releasing node, the least loaded, skipped for the next allowed one; a chunk
  whose only allowed node releases it waits, with #13's reason, and the drain goes on. Once #17
  lands: a volume whose chunks are all blocked is reported waiting for that reason.
- **The lab, a later step** (three data nodes; fl-c has no zone today): fl-c labelled into a
  zone of its own, a `zone`/`strict` volume on fl-a and fl-b moves to fl-c when fl-b is drained,
  its replicas in distinct zones. With fl-c labelled into fl-a's zone, the same drain ends with
  the volume blocked and still on fl-b, `flaminio-meta move` to fl-c by hand is refused, and a
  `zone`/`bestEffort` volume moves to fl-c. `make drain-test` (vol1, level `node`) passes as
  before.

## Open questions

- **Q1. What a `strict` drain does with a chunk it cannot move** (S11–S13). (a) Stop at the
  first one, as today's "no node to move to": one volume then keeps the rest of the node from
  draining. (b) Skip it, move the rest, report it, and fail once only blocked chunks are left.
  (c) As (b), but wait until the time-out for the topology to change. **Recommended: (b),**
  also for a chunk with no eligible node at all, which changes today's result for that case.
  Settled at approval, 2026-11-13: (b), also for a chunk with no eligible node at all (see decisions.md, Settled at approval).
- **Q2. A `bestEffort` move named by hand** (S6). (a) The service accepts it as today; only the
  drain prefers the higher level. (b) The service refuses it when another eligible node would
  keep a higher level: it must then judge liveness and draining for every move, and an
  administrator cannot override it. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), the service accepts it as today (see decisions.md, Settled at approval).
- **Q3. Volumes that exist already** (S3). (a) They read as `node`, `strict`, which is how
  their moves behave today; one created with `rack` or `zone` keeps no protection. (b) Add an
  administrator's command to set a volume's level and policy. (c) Infer the level from the
  current placement. **Recommended: (a) now;** (b) later, if such volumes exist outside the lab.
  Settled at approval, 2026-11-13: (a) now; (b) later, if such volumes exist outside the lab (see decisions.md, Settled at approval).
- **Q4. Whether the rule also goes into the Rust model, its vectors and the simulator.** The
  model has no node labels, and placement (`Place`) is Go only: adding topology changes the
  vectors and the simulator for a rule about placement, not replication. **Recommended: Go
  only,** with the property test above as the injected bug's check.
  Settled at approval, 2026-11-13: Go only, with the property test as the injected bug's check (see decisions.md, Settled at approval).
- **Q5. Mixed versions of the metadata service.** An older instance that attaches a volume
  rewrites the header from the fields it knows, and drops the record: the volume then reads as
  `node`. (a) Accept it: every instance runs the new version before a `rack` or `zone` volume
  is created or moved (`docs/lab.md`). (b) Make headers keep fields an instance does not know:
  a wider change, beyond this issue. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), accepted: every instance runs the new version first (see decisions.md, Settled at approval).
  Changed after approval (plan review round 1, R1-S2): a rollback is a mixed version too, so
  the metadata service is not rolled back below this version once a `rack` or `zone` volume
  exists (S16; decisions.md). Round 2 (R2-S1): an operating rule, documented, that nothing
  enforces: an older instance cannot know it. A rollback anyway is a risk named, not accepted.
