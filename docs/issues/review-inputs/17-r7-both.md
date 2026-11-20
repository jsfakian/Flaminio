# Plan review #17 round 7 (incremental, design-code + design-security)
Diff ebd8e23..fc6cac8 (docs/issues/17/, docs/adr/, without report.md).

## Earlier findings
- R6-B1: fixed. Every maintenance detach, S12's included, is `DetachMaintenance{G}` (design.md:178-183, :281-285). Test at :595-597; its engine half is untested (R7-B1).
- R6-B2: fixed. Only the router removes an entry, on the engine's last `Detached{G}` (:192-195), and the report comes before it (:184-191). After a time-out no `Marked` is sent, so no late `Attached` comes (:196-201). Test at :587-589.
- R6-B3: fixed. Row `gone | MarkEpoch or time-out` (:225); test at :590.
- R6-B4: fixed. The reserve and `Input::Attach` share one hold (:171-177). The send cannot block: the input channel is an unbounded `mpsc::channel` (flaminio-io/src/node.rs:288), and the wake is an eventfd signal (node.rs:406-409). Test at :591-592; it has no hook (R7-B1).

Earlier rules still in place: the run mark before `Hello` (:39, :199-200, :572-574), the `StartFailed` back-off (:156-163), `EndMaintenance` at the epoch (:158-159, :271-273), and the forgotten-node refusals (:71, :92-102, not touched).

### R7-B1 — Medium
The table-driven test cannot test the engine's half of the lifecycle, and this round deleted the engine assertion that covered it.
- `maintenance_entry_lifecycle` runs "with the engine's outputs ... scripted" (design.md:575-576). Yet it checks "the reports to the `Maintainer`", one `EndMaintenance`, no "ended by root" line, "one back-off, no second epoch" and "the client's target keeps running" (:578-597). The `Maintainer`, its `EndMaintenance` and that log line are all inside the engine. With the engine scripted, these checks only repeat the script. Invariants 1 (one end, :233-235) and 4 (no client's target ended, :244-246) are engine properties, so the test cannot fail on them.
- The engine rules the table rests on have no other test:
  - `DetachMaintenance` acts only on the maintenance attach or target of that generation, and emits nothing otherwise (:178-183);
  - a refused maintenance attach gives one `Detached` and no `AttachFailed`;
  - `StartFailed` comes just before `Detached` exactly when `Attached` never came, else the S11 kind (:184-191);
  - a `Marked{G}` that arrives after G was dropped starts nothing.

  The round deleted the one assertion that covered the first rule: "an `Input::DetachMaintenance` of another generation, or for a client's target, changes nothing" (the old R5-2 test bullet). An engine whose `DetachMaintenance` calls `Engine::detach`, which acts by volume (engine.rs:1856-1861), passes every test listed.
- "One case per row" leaves half of three rows untested. Rows :222 and :225 each bundle `MarkEpoch` with the time-out, and :228 bundles a second `Detached` with an unknown G's output.
- The R6-B4 case ("root's end sent at once after the reserve") fails against a reserve and a send in two separate holds only if root runs between them. No hook for that is named.

Fix:
- Add `maintenance_generation_end` to `flaminio-node/tests/engine.rs`, with the real engine and a fake metadata client. Cover each way G ends: a refused attach; `DetachMaintenance` before the `GetVolume` answer, between `MarkEpoch` and `Marked`, and after `Attached`; no work; deposed; gone. For each, check one report, then one `Detached{G}` as G's last output, `StartFailed` exactly when `Attached` never came, and `EndMaintenance` as S11 says.
- In the same test, send `DetachMaintenance` of another generation, over a client's target, and after G ended. Each emits nothing and leaves the target running. A `Marked{G}` after a drop sends no `Hello`.
- Keep the node test to node-side checks (what is sent, the state, the place count, the waits), with one case per event rather than per row.
- For R6-B4, hook the registry lookup. When root's lookup sees the entry, assert the fake channel already holds `Attach{G}`.

### R7-B2 — Low
Root's `maintenance end` now waits for the entry to be gone, up to "#15's 30 s" (design.md:215, :482-483); this wait is new in this round. The socket line still lists only `ok | error: VOLUME: no maintenance target here` (:479). The design says neither what root is answered past 30 s nor what state the entry is left in. S12's client attach relies on the same bound, and its answer on expiry is unstated too (:240, :285).
Fix: past 30 s, answer `error: VOLUME: the maintenance target did not end within 30 s`, as #15's detach does. The entry stays `ending`, and the router still removes it on `Detached{G}`. A client attach gets the same answer and reserves nothing. Add the answer to the socket line, and test it with a scripted engine that withholds `Detached{G}`.

## Nits
- N1: design.md:241-242 and decisions.md:346-347 say "two client attaches at once both detach G". The table's `ending | a client attach` row (:224) sends nothing, so only the first attach sends. Write "the first detaches G, the second waits for gone".
- N2: Row :214 (`starting | Detached{G}`) says the attach was refused and the engine "reported `StartFailed`". The same row is also reached in two other cases:
  - when the engine drops G after `Marked`;
  - when a running target's `Detached` is routed ahead of its `Attached`. #15's router runs on whichever thread took the outputs, after it unlocks (#15 design.md:31, :39), and a queue thread's flush takes outputs too (ublk.rs:582-600). In that case the engine reported the S11 end.

  Separately, the `gone` row (:225) leaves out the main thread taking `Detached{G}` from its waiter.
- N3: decisions.md:343-345 says item 54 replaces "item 53's time-out rules and item 52's exception". It also reverses four items, which it should name:
  - item 39: the main thread as the one reporter, and a dropped start with no log and no `on_ended`;
  - item 40: the main thread's detach after a `Detached`;
  - item 45: the main thread ending the entry on an early `Detached`;
  - item 30: `MaintainFailed` for every failed start.

Reviewed commit: 30cf82d8791dd2833627c8308600ac562fbbed14
