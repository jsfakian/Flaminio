# Issue #18: Moves and drains ignore a volume's failure domain — intent

[Issue #18](https://github.com/jsfakian/Flaminio/issues/18), [follow-ups](../../followups.md) §4
("Left: a volume does not record its failure domain, so moves and drains ignore it"). This
change is both steps of the issue: it closes it.

## Why

A volume is created with a failure-domain level (`node` | `rack` | `zone`) and a policy
(`strict` | `bestEffort`); `Place` then puts no two replicas of a chunk in one domain
(ARCHITECTURE §8.2, §9). Nothing after the creation knows either of them.

1. **The header does not keep them.** `createVolume` gives `spec.Domain` and `spec.Policy` to
   `Place` (`meta/service/service.go:404`) and then builds the volume with `NewVolume`, which
   copies neither (`meta/domain/domain.go:199`). `Header` has no field for them
   (`domain.go:37`), and the API's `Volume` reports none (`meta.proto:117`).
2. **The drain chooses by load alone.** Its candidates are the live nodes that are not
   witnesses, not draining and not the drained node (`control/drain/drain.go:74`). For each
   chunk it takes the least loaded one without a replica of that chunk (`leastLoaded`,
   `drain.go:120`, `:150`). It never reads a node's labels.
3. **The service accepts any registered destination.** `MoveReplica` checks that the
   destination is registered (`service.go:870`) and `CheckMove`: the source holds a replica,
   the destination does not, no move is pending (`domain.go:287`). `flaminio-meta move` passes
   the names through (`meta/cmd/flaminio-meta/main.go:426`). Only an administrator may call it
   (`meta/service/auth.go:64`).
4. **The request is the one place a destination is accepted.** The target carries a move out
   with `ChangeIsr`, which accepts a change of replicas only when it is the recorded move's add
   or drop (`validMove`, `domain.go:259`; the Rust model, `flaminio-repl/src/meta.rs:232`). A
   rule on the request therefore holds for every caller.
5. **What goes wrong.** A `zone`/`strict` volume with two replicas, on a (zone z1) and b (z2),
   and a spare node c in z1. Draining b moves its replicas to c, the only candidate: both
   copies of every chunk are then in z1, and losing z1 loses the volume. `flaminio-meta move`
   to c by hand does the same. No lab run shows it: the 2026-10-28 check covered creation only
   (follow-ups §4). In the lab fl-a is in zone lab-a, fl-b in lab-b, and fl-c has no zone
   (`scripts/lab.sh:609`).
6. **A chunk without a destination stops the whole step today.** `stepVolume` returns an error
   at the first chunk with no candidate (`drain.go:122`); the volumes after it get no moves,
   and `Run` ends. `TestDrain` relies on it. More chunks will have no destination once the
   domain counts, so what the drain then does has to be chosen.
7. **The Rust side has no topology.** The reference model and the vectors
   (`meta/testdata/isr-vectors.jsonl`) cover `CheckMove`, not placement: `Place` is Go only,
   with Go tests. The data plane's client (`flaminio-metaclient/src/pb.rs`) is written by hand
   and ignores fields it does not name.

## What we want

1. A volume records its failure-domain level and policy when it is created, and reports them.
2. Under `strict`, the metadata service refuses a move whose destination shares a domain with a
   replica that stays, whoever asks: the drain, or an administrator naming a node by hand. A
   node without the label of a volume's `rack` or `zone` level is no domain of its own there: a
   new node starts unlabelled, and is labelled later. It joins no other replica of a `strict`
   chunk until then, by a move, a drain or a creation.
3. The drain chooses destinations by that rule. Under `bestEffort` it falls back a level, for
   that chunk, when no destination keeps the volume's level.
4. A drain that cannot move a chunk says which volume and why, moves everything else, and does
   not report success.
5. Volumes created before this change move as they do today.
6. The protocol, the API comments, the lab guide and follow-ups §4 say so.

## Out of scope

- **Repairing a spread that is already broken:** a node whose labels change after placement,
  or a `bestEffort` volume placed a level down. Nothing moves replicas to restore it; a later
  move only never adds a collision under `strict`. A rebalancer is the placement controller's
  work (`docs/phase1-plan.md`).
- **Draining unattached volumes:** issue #17. It uses this change's destination choice.
- **Replicas a completed move leaves on the source:** issue #13. Once it lands, the drain
  proposes no node that releases the chunk, and a chunk only such nodes could take waits.
- **The destination's other properties, at the service.** Today `MoveReplica` accepts a
  witness, a dead or a draining node named by hand; only the drain filters them. Issue #78
  refuses a draining one, and issue #72 a dead one or one without a capacity report; this change
  adds neither check, and its rule composes with theirs.
- **Changing a volume's level or policy after creation,** and setting them on volumes that
  exist already.
- **Creation:** `Place` and its rules stay as they are, but for leaving out unlabelled nodes
  under `strict` (What we want 2), as do the other constraints of ARCHITECTURE §9 (device
  class, watermarks, locality).
- **The CSI controller's check of a repeated `CreateVolume`** stays on size, chunk size,
  replicas and minimum ISR.
- **Real independence in the lab:** one switch, and fl-w shares a server with fl-a
  (follow-ups §4).
