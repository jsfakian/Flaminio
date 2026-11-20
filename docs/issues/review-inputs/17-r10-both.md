# Plan review #17 round 10 (incremental, design-code + design-security)
Diff a28a267..7daf8ea (design.md, spec.md, decisions.md).

## Round-9 findings
- R9-B1 (hook deadlock): fixed. The hook is cut; `reserve_maintenance` queues the attach inside the caller's hold. The channel is an unbounded `mpsc::channel::<Input>()` (node.rs:288), so the send under the registry lock cannot block. Residual: R10-1.
- R9-B2 (narrowing wording): fixed in the live text (spec Tests :196, design :83, :626). Only history says otherwise (decisions.md :119, :332, design :766). Item 21 and the item-for-round-4 line stay as history.
- R9-B3 (plugin string): partly fixed. The `data plane: <volume>: ...` form is right (dataplane.go:83). The `DeadlineExceeded` half is open (R10-2).
- R9-B4 (late reporter): partly fixed. The accessor is right. The "recording log sink" does not exist (R10-3).
- R9-B5 (gone wins): fixed in intent, with wiring gaps (R10-4).

## Findings

### R10-1 - Medium: "only way a maintenance entry is inserted" is not enforced where the main thread lives
design.md:189-192, :638-643. Rust privacy is per module. The claim "code outside node.rs can reach the entries only through methods" holds only if `Registry` is defined in node.rs. #15 does not say where it is (#15 design.md:61; it names `Node` in `ublk.rs`, :13). The main thread is inside node.rs, so the compiler stops nobody there from writing `entries.insert(maintenance...)` and sending later; that is the R6-B4 bug. The fallback is "after every case, each maintenance entry's attach is first among the inputs naming its generation". A reserve in one hold and a send in a later hold also passes that check in a single-threaded test, since the channel order is the same. The `try_recv` test only covers the method, not the callers.
Fix: put `Registry` in its own small module (`registry.rs`, or `mod registry` in node.rs) with a private field and `pub(super)` methods, and say so in the files list. Name `reserve_maintenance` as the only constructor of a maintenance `Entry` (a private constructor). State that the check after each case is a backstop only.

### R10-2 - Medium: the `DeadlineExceeded` path rests on a string #15 does not map, and the table's wording is wrong for a wait cut mid-way
design.md:142-145, :247, :272, :567-569. #15 maps an attach "that times out" to `DeadlineExceeded` (#15 design.md:399) but names no string for it. The only timeout text it gives is `its caller's time ran out before it could begin` (#15 :92), for a request never begun. Because `within` is the remaining time minus `answerMargin` (dataplane.go:106-107), the data plane answers before the Go deadline. The plugin then gets an error text, not a context error, so `DeadlineExceeded` appears only if the plugin matches a string. Two defects:
- #17 says a client attach cut by its own limit during the wait answers "before it could begin". The attach has begun by then, so the wording is wrong. An implementer either reuses a misleading text or emits a different one.
- The plugin test ("an attach whose time limit is shorter than 30 s gets `DeadlineExceeded`") names no input string, so it cannot tell which text the fake socket must send.
Fix: give the mid-wait cut its own exact text (for example `error: VOLUME: its caller's time ran out while the maintenance target ended`), or say that #15's text is reused and that the plugin matches the `data plane: VOLUME: its caller's time ran out` prefix as #15 specifies. Name the matched string in the plugin bullet and the test, and have the table row use it.

### R10-3 - Low: the "recording log sink the test installs" does not exist
design.md:665-666. The engine logs with `eprintln!` (engine.rs:830, 860, 1343, 1425, 1440). It has no sink to install, and stderr cannot be counted in-process. The design adds no sink to its changes (design.md:499-504, the files list).
Fix: either add one (an `Engine` field holding a `Box<dyn Fn(&str)>` that defaults to `eprintln!`, or a `log` callback on `Maintainer`) and list it, or drop line counting and count `EndMaintenance` calls and `maintenance()` state only.

### R10-4 - Low: the "gone wins" test does not say how the waiter sees the clock or is woken
design.md:651-655. The waiters block on a queue or condvar with a real timeout. "Moves the fake clock past 30 s" does not reach a blocked thread. The test removes "as the router does", which may be inline code that does not wake the waiters. If it does not, the waiter ends on a real time-out instead and the case tests nothing. The design does not say that the waiter takes its deadline from the injected clock, nor that the test must be sure the waiters are blocked before taking the lock.
Fix: say that the waits compute their deadline from the injected clock (the same one the 30-s case at :646 uses); that the test calls the router's own removal function (which wakes), not a copy; and that it first waits on a barrier until both waiters block. The waiter must read the state before the clock.

### R10-5 - Low: the new test lines say "read with `try_recv`" on a channel the real node owns
design.md:638. The node test's "fake engine channel" is the `Sender<Input>` given to `reserve_maintenance`, so `try_recv` on the receiver is possible. But `reserve_maintenance` takes `&attach_tx`, while the plan elsewhere says the router and main thread share the sender (`node.rs:288`, whose `tx` is cloned). Say that the node under test is built with its own `mpsc::channel` and the test keeps the receiver.

Nits: none.

Reviewed commit: 7daf8ea
