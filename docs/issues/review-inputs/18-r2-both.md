# Issue #18 plan review: round 2 (incremental), design-code and design-security

Scope: `git diff 1287453..93220c3 -- docs/issues/18/ ':!docs/issues/18/report.md'`, checked against
the code in /home/jsfakian/Documents/src/flaminio-18 and the plans of #13, #17, #45, #72 and #78.
HEAD differs from 93220c3 only in report.md.

Checked and correct:
- No plan of the batch claims `Volume` 18 or 19; meta.proto:117-127 ends at 8.
- The label writers match auth.go:71-72 (`isNode`: the node's `agent` or `io`; any administrator).
  The CSI node plugin uses the node's `agent` certificate (lab.sh:1000).
- No other refusal text starts with `failure domain collision: `:
  - #72: `node <to> has no current capacity report` and `... no readable capacity ledger ...`
    (72/capacity-contracts.md:115-116); `RESOURCE_EXHAUSTED` texts start with `capacity: `
    (:109-112).
  - #78: `destination "b" is draining` (78/design.md:54-55).
- "At most 9 compares, 2 operations" is right:
  - #72 adds the ledger compare and write (72/capacity-contracts.md:33);
  - #78 adds the gate compare (78/design.md:55, :138);
  - #78's gate replaces #13's `forgotten` mark (13/design.md:193-196);
  - replicas are at most 5 (domain.go:186).
- The #13 order matches #13's drain (13/design.md:787-799) and its hand-off
  (13/decisions.md:235-236). The #17 seam matches 17/design.md:428-433, :469-473.

## Earlier findings
- R1-C1: fixed (but see R2-C4: the lab's step 4 with #13).
- R1-C2: fixed (follow-ups R2-C2 and R2-C3).
- R1-C3: fixed.
- R1-C4: fixed.
- R1-C5: fixed.
- R1-C6: fixed.
- R1-C7: fixed.
- R1-S1: fixed for moves and drains. Its reach is overstated and its blocking effect is not
  stated (R2-B1); the drain half has no test (R2-C1).
- R1-S2: addressed as a documented rule. It is not enforced, and decisions.md says it is (R2-S1).
- R1-S3: fixed.
- R1-S4: fixed.
- R1-S5: fixed.
- R1-S6: fixed.
- R1-S7: fixed.
- Nit (compare count): fixed.
- Nit (`%q`): fixed.

### R2-B1 — Medium
**Creation still gives an unlabelled node a `strict` replica, so the docs overstate the fix. The
fix also makes such a chunk's other replicas unmovable, which nothing states.**

Creation is unchanged (S15):
- `Place` counts unlabelled nodes as one domain, `"z:"` (domain.go:322-331, `resolveDomain`
  :343-351, `placeChunk` :405-417).
- A new node has load 0, so it is first in `placeChunk`'s order. It gets a replica of every new
  `zone`/`strict` chunk until its labels arrive.
- A Kubernetes node without a zone label stays unlabelled for good: `topologyFromKube` skips the
  missing label (control/csi/topology.go:21-27).

The plan claims otherwise:
- design.md Docs (:300-301), the text for `docs/lab.md`: "a new node takes no `strict` replica at
  `rack` or `zone` until it is labelled".
- decisions.md:91-93: "takes no `strict` replica until it is labelled".

Under the new S5, a chunk with a replica on an unknown node cannot have its other replicas moved
until that node is labelled. That chunk can come from creation, or from a node that clears its
own zone to `""`: its `agent` or `io` may, and labels merge (service.go:197-200).
- Every drain of the chunk's other nodes ends with `no destination (zone/strict)`, which does not
  name the unlabelled replica (`BlockedVolume` has only the level and policy, design.md:135-136).
- #45's rebuild uses #18's rule (45/spec.md:68-69). It cannot restore redundancy for that chunk,
  and its reason "under `strict` every one collides" (45/spec.md:86) is wrong.
- Risks (design.md:315-330) says a node can block moves "by claiming a staying replica's zone". It
  does not say that clearing its own label blocks every move and every rebuild beside its
  replicas.
- Moving the unknown replica first is allowed, since its staying replicas are labelled. No doc
  says so.

**Fix:**
- Reword design.md Docs, decisions.md and intent.md:50-51: no move or drain puts a `strict` replica
  on an unlabelled node; creation still may (S15), until its labels arrive.
- Add to Risks: the block, a node's ability to cause it by clearing its own label, and the remedy
  (label the node, or move its replica first).
- Give `BlockedVolume` a reason (e.g. `replica "x" has no zone label`) and print it in `String()`
  and `BlockedError`.
- Add a hand-off to #45's S15, due in whichever of the two lands second.
- Optionally, a follow-up issue for `Place` to fail closed the same way.

### R2-C1 — Medium
**The drain's new rule for an unknown staying replica has no test, and the invariant test cannot
catch its bug.**
- The rule: a replica's node that is not listed, or has no label, is unknown, and `Strict`
  refuses every destination beside it (design.md:92-94, :99-100).
- The drain tests cover only unlabelled destinations: "a new unlabelled node with load 0 not
  proposed … with only unlabelled ones left the chunk blocked" (design.md:237-239). Spec Tests say
  the same (spec.md S10–S14 line).
- The likely bug: a drain that uses `Collision` for the staying replicas. It proposes a node, the
  service refuses it with the collision text, and the chunk counts as Refused. `Remaining ==
  Blocked - Refused` then never holds, so `Run` steps until the time-out instead of returning
  `BlockedError` (S13).
- The invariant test cannot catch this: the service refuses the move, so nothing breaks.

**Fix:** add a drain test, a `zone`/`strict` chunk on a labelled node and an unlabelled one,
draining the labelled node. Expect no `MoveReplica` request (an interceptor counts them),
`Blocked` 1, `Refused` 0, and `Run` returning `BlockedError`. Run it again with the replica's node
not listed (record deleted from the store).

### R2-C2 — Low
**No test proves that the drain recognises the refusal text the service actually sends.**
- The service test pins the message (design.md S5–S7 tests).
- Every drain test of a collision fakes the text in an interceptor (design.md:248-251).
- So if the drain's literal and the service's message drift apart, every test still passes, and
  real refusals are handled by the other-cause rule: the destination is dropped for every chunk.

**Fix:**
- The drain matches `domain.ErrCollision.Error() + ": "` against
  `status.Convert(err).Message()`. `control/drain` may import `meta/domain` (design.md:23).
- Add one drain test where an interceptor relabels the destination into a staying replica's zone
  (`svc.RegisterNode`) just before the real `MoveReplica` handler. Expect `Refused` 1, and the
  destination still proposed for the other chunks.

### R2-C3 — Low
**A chunk blocked by the issue #72 rule is reported as a domain block, and the last refusal has
nowhere to go.**
- The plan sends #72's "last refusal" to the progress line instead of a failed step
  (design.md:109-116).
- `Progress` gains only `Blocked`, `Refused` and `BlockedVolumes` (design.md:135-142). `String()`
  has no place for the last refusal.
- So `BlockedError` reads `have no destination: v1 (node/strict, 1 chunks)` (design.md:150-151)
  when the cause was a missing capacity report or a draining destination.
- #72's S8 named the last refusal in the error (72/spec.md:123-125). This rule and its test are
  #18's now ("Neither plan's code is needed").

**Fix:**
- Add `Progress.LastRefusal string` (or a reason per `BlockedVolume`), printed by `String()` and by
  `BlockedError`.
- Assert it in the existing test "a `FailedPrecondition` without the fixed text … with none left
  blocked".

### R2-C4 — Low
**With #13, the lab's step 4 hand move gets `applied=false` and exit 0, not the refusal.**
- After step 3 moves `dz18s` back to fl-b, fl-c holds a release of every chunk until its sweep
  deletes and confirms.
- #13's `CheckMove` answers `Applied: false`, `node T releases chunk C`, before #18's check
  (13/design.md:94-98; #18 design.md:79-84).
- `flaminio-meta move` prints the answer and exits 0 (main.go:426-431).
- Step 4 runs `move dz18s 0 fl-b fl-c` at once (design.md:283-285). Its result depends on timing.

**Fix:** once #13 lands, before step 4, wait at most 60 s until `metactl releases fl-c` is empty,
as #13's `drain-test` does (13/decisions.md, R3-C1). If it is not, exit 1 with a named message.

### R2-S1 — Low
**Q5's no-rollback rule is only documented, but decisions.md says it prevents the drop. The
operator cannot easily check when it applies.**
- decisions.md:100-104: this option "keeps an N-1 instance from dropping them; the other only
  documents the loss".
- Nothing enforces it:
  - an N-1 instance still marshals the header from its own struct and drops both keys
    (service.go:723-725);
  - after the re-upgrade an absent key reads as `node`/`strict`, like a legacy volume, so the loss
    is silent and permanent.
- "Once a `rack` or `zone` volume exists" is hard to check. `ListVolumes` returns ids only
  (drain.go:51-56), so the operator must `get` every volume.
- ARCHITECTURE §19 (:940) still says every release is tested "N → N-1 rollback before finalize";
  this release has no finalize step.

**Fix:**
- In decisions.md, call it an operating rule that nothing enforces.
- In the ARCHITECTURE §18 and `docs/lab.md` text:
  - say how to check (`get` each volume, e.g. a `jq` loop);
  - say that a rollback anyway strips the level and policy of every volume attached meanwhile,
    silently and for good. Recreating the volume is the only remedy, since Q3(b) does not exist.
- Note at §19 that creating the first `rack`/`zone` volume acts as this release's finalize.

## Nits
- spec.md:29-31, the invariant: "nor, at `rack` or `zone`, one whose destination … is unknown"
  lacks "for a chunk with a staying replica". S5 (:55-60), `Strict` ("nil when `staying` is
  empty") and the test "one replica: any destination, unlabelled included" allow that move. An
  implementer who follows the invariant would refuse it.
- design.md:67-78: it says "each wraps `ErrCollision`", and the message is
  `failure domain collision: volume %q chunk %d: ` followed by "`Strict`'s reason".
  - If `Strict`'s error is `%w: …`, appending its `Error()` repeats the prefix.
  - If the service wraps with `…: %w`, the marker ends up last.
  - Say that `Strict` returns the bare reason, and only the service writes
    `fmt.Errorf("%w: volume %q chunk %d: %s", domain.ErrCollision, …)`.

Reviewed commit: 339601b1b3c701700e9d3c5bae928264b794b06c
