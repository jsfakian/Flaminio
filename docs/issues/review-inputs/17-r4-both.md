# Plan review: issue #17, round 4 (incremental), design-code + design-security

Scope: `git diff a4c01a7..9e3a5e2 -- docs/issues/17/ docs/adr/ ':!docs/issues/17/report.md'` on
issue/17-drain-unattached-volumes. HEAD 958436b adds only report.md. Checked against
`control/csi/meta.go`, `control/csi/node.go`, `meta/service/{service,auth}.go`,
`control/drain/drain.go` and #15's plan (flaminio-15, e36882b).

## Earlier findings
- R3-B1: fixed.
  - The variant, the rollback order (Rollout, S19, ADR 0014, the `docs/lab.md` line) and the
    two-instance test are all in.
  - The variant skips no new instance wrongly and accepts no old instance's success. A new
    instance answers a publish held every time, and answers an unhold with anything but
    `Unimplemented`. An old instance that answers an unhold never runs `Authorize`
    (unregistered method), so it answers `Unimplemented`.
  - An old instance's publish over another node writes before the call moves on. That write is
    harmless: held by S6, and the next instance's `Publish` row then changes nothing.
  - A rollback leaves no record wrongly unheld: the node plugin's `only_if_target` attach
    (`control/csi/node.go:70`) rewrites the header through the old service.
  - The test's `preferred` assertion is a Nit (R4-N1).
- R3-C1: fixed. The main thread is the one ender and the one reporter. Removals check the
  generation, `on_ended` ignores repeats, and the engine runs no S11 end before `Attached`. No
  double free remains: both removals check the generation, under the registry's lock. Two new
  defects: who records "has had its `Attached`" is not specified (R4-C1), and a refusal before
  the entry sends a `Detach` with no generation (R4-C3).
- R3-C2: fixed. The main thread's `Input::Detach` follows its own `Input::Attach` on one FIFO.
  The client reserves only after the main thread has ended the entry, so no `Attaching` is left.
- R3-C3: fixed (no epochs counted). The resend of `EndMaintenance` stays conditional on node,
  hold and epoch, and is idempotent: it is sent only while the volume is still listed at that
  epoch, and epochs only rise. The new detection misses the common fast failure (R4-C2). The
  Stall rule now fires right after the drain's own re-ask (R4-C4).
- R3-C4: fixed in the design (:460-463), the spec (:176-178) and the ADR. Both injected bugs
  still trip their check.
- R3-N1, R3-N2: fixed.

The crash-loop residual is stated the same way in the design's Risk 7 (:601-603), decisions
item 41 and the report, and it agrees with S17's "alive and started is not asked again".

### R4-C1 — Low
**A maintenance entry whose `Detached` is routed after its `Attached`, but before the entry is
marked running, is ended by nobody.**
- **The rule.** The router ends only "a running entry, one that has had its `Attached`". "A
  starting entry is the main thread's to end" (design.md:189-194).
- **The gap.** Nothing says who records that the entry has had its `Attached`, or when.
  - Step 4 has the main thread take `Attached` off the waiter and "take the next attach"
    (design.md:164-165).
  - In #15, the thread that owns the entry turns it running (#15 design.md:163-166).
- **The race.** Suppose the main thread marks the entry running when it takes `Attached`.
  1. The router routes `Attached` to the waiter.
  2. A `Detached` of that generation follows: a client attach's or root's `Input::Detach`, a
     deposition, or no work. The router routes it before the main thread has taken `Attached`,
     sees a starting entry, and leaves it.
  3. The main thread then takes `Attached`, marks the entry running, and moves on.
- **Effect.** The entry, and its `--max-maintenance` place, are never freed. S12's client attach
  waits out its 30 s and fails until a restart.
- A client attach over a start in flight, a tested scenario (design.md:503-508), is exactly when
  a `Detached` follows `Attached` closely.

Fix:
- The router marks the entry running as it routes `Attached`, under the registry's lock.
- Then, if `Detached` is routed first, the waiter sees it first, and the main thread ends the
  entry (step 5). If `Attached` is routed first, the router ends the entry on `Detached`.
- Test: a `Detached` routed right after `Attached`, before the main thread takes `Attached`. The
  entry and its place are gone.

### R4-C2 — Low
**The "failed start, asked elsewhere at once" rule misses a start that fails within one drain
step, which is the usual case.**
- **What it needs.** The rule needs the previous read to have seen the node's maintenance record
  with a move pending (design.md:233-234, :262-267).
- **The timing.**
  - The drain reads the record (not held), then asks the node (design.md:243-249). Its next read
    comes 5 s later (`main.go:436`).
  - The node's scan, also every 5 s (design.md:139), takes `StartMaintenance` within that
    window.
  - The failures that step 5 names end the record within milliseconds: a refusal at the
    registry, a failed mark (design.md:166-181).
- **What follows.**
  - The next read finds the record not held, naming the node, with a move pending. The flag is
    false, so this is no failed start.
  - The drain asks the first candidate again, the same node, "it ended at no work"
    (design.md:248-249), and takes another epoch.
  - That node is in back-off, so the volume moves on only after `Stall`, about 5 minutes and two
    epochs later.
- **Spec.** S17 requires asking again "when it ended the record's maintenance while a move of
  the volume was pending" (spec.md:126-127).
- **Test.** The test (design.md:532-533) scripts the intermediate read, so it cannot see this.

Fix:
- Count the drain's own applied `AttachMaintenance` answer as a read: it returns that node's
  maintenance record, and the drain asked because a move was pending. Say so where the sighting
  is defined.
- Test: the node's start fails between the drain's ask and its next step, and the volume is
  asked elsewhere at the next step.

### R4-C3 — Low
**Step 5 sends `Input::Detach` "with the generation" on every failed start, including a refusal
before the entry, which has neither an entry nor a generation.**
- **The text.** design.md:166-171 lists "a refusal before the entry" among the failed starts,
  then sends `Input::Detach` "after its own `Input::Attach` if it sent one". The only failed
  start without an `Input::Attach` is that refusal.
- **Generations.** A generation is taken at the reservation (design.md:153-157).
- **Why it matters.** One cause of that refusal is that "the volume has any entry", for example
  a client's device entry.
  - Such a `Detach` is harmless only if #15's engine ignores a `Detach` whose generation is not
    its target's or attach's. #15 does not say so (#15 design.md:248-253).
  - Today's `Engine::detach` works by volume alone (`engine.rs:1856-1860`). It would end the
    client's target.

Fix: send `Input::Detach` only when the main thread reserved an entry. Say that the engine
ignores a `Detach` of another generation.

### R4-C4 — Low
**The Stall rule fires right after the drain's own re-ask of the same node, before that node has
had time to start.**
- **What changed.** The sighting's epoch is reset at the drain's own ask, but its clock is kept
  across that ask (design.md:229-233; spec.md:128-131).
- **The case.** The volume ended at no work while a chunk waited on #13's releases (R3-C3's case).
  - Both clocks are then usually past `Stall`.
  - At the drain's next step, if the node has not yet taken its start epoch, "the record's epoch
    is still the sighting's" (design.md:268-270) holds.
  - The drain asks another candidate. It is not the best-ranked one, and it may depose a target
    that started just after the drain's read: `AttachMaintenance` has no expected epoch.
- **Contradictions.**
  - The R3-C3 test, "a node re-asked after an end at no work ... not asked elsewhere"
    (design.md:534-535), depends on the node starting within one step.
  - The spec's text has the same flaw ("no epoch ... since the drain's own last ask of it").

Fix:
- Require `Stall` since the sighting's epoch was last set, too: restart the clock at the drain's
  own ask.
- With failed starts detected directly (R4-C2), R2-N4's reason for keeping the clock is gone.
  A node that never starts never ends, so it is never re-asked.

## Nits
- **R4-N1.** design.md:443-444 and ADR 0014's Confirmation test that "`preferred` is unchanged"
  after a success through the second instance.
  - Since `metaCall` stores the instance that answered (`meta.go:82`), the natural variant sets
    `preferred` to 1.
  - Because the old instance is index 0, the default, the assertion cannot catch an old instance
    stored as preferred.
  - Say "`preferred` is never set to the old instance". Test it with `preferred` starting at the
    old instance.
- **R4-N2.** design.md:500-501: "the router's removal of a later generation's entry leaves a
  client's entry alone" says the opposite of the rule. Say "the router's removal on the
  maintenance generation's `Detached` leaves a later client entry alone".

Reviewed commit: 958436b264e64db056fc36033881dba547e6e58f (the diff a4c01a7..9e3a5e2; 958436b adds only report.md)
