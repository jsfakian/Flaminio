# Issue #17: Draining volumes that are not attached — design

How the [spec](spec.md)'s rules are built; [decisions](decisions.md).
Approved 2026-11-13; not implemented.
Proposed with it: [ADR 0014](../../adr/0014-maintenance-attach-only-while-not-held.md).
Settled at approval, 2026-11-13: the spec's open questions 1 to 10, each with its recommendation; ADR 0014 (numbered 0007 then) stays Proposed in docs/adr/ until accepted in its own step (see decisions.md, Settled at approval).
Changed after approval, in rounds 1 to 18 of the plan review (2026-11-14; decisions.md, with
each finding's id). #17's release is now an unhold (`UnholdVolume`, `flaminio-meta unhold`, the
record's `unheld`), since #13 keeps "release" for copies to delete; the approval lines keep the
old word.

The facts it starts from:
- **One attach, one writer.** `Header.Attach` alone sets `Target` and bumps `Epoch`
  (`meta/domain/domain.go:217`); the header has no mark of a client (`:37-54`). `Service.Attach`
  refuses `only_if_target` for another node (`meta/service/service.go:714`), answers
  `unless_target` to the target with no write and no token (`:716`), else writes the header
  conditional on its revision (`:725`) and signs a token (`:743`). A lost compare reads again,
  32 times, then `Aborted`.
- **Callers.** `Attach`: an administrator, or the node's `agent` with `only_if_target`
  (`meta/service/auth.go:66-70`). `ChangeIsr`: `io`, and the method checks the caller is the
  target (`service.go:802`). `ControllerUnpublishVolume` changes nothing
  (`control/csi/controller.go:294`); `lab.sh vol detach` only stops the node's device
  (`scripts/lab.sh:720`).
- **The model.** `flaminio-repl/src/meta.rs` has `Attach`, `ChangeIsr`, `MoveReplica`, `Get`
  (`:55-74`). `examples/meta_vectors.rs` writes `meta/testdata/isr-vectors.jsonl`, which
  `meta/domain/domain_test.go:43` replays. The simulator's controller attaches, then starts
  the target (`sim/mod.rs:1319`); its mutation hints are seeds of `Config::random`
  (`sim/tests.rs:487`).
- **The target** polls its record every 2 s (`types.rs:63`), and deposes itself on a newer
  epoch or on `NotFound` (`target.rs:1100-1111`). The engine never hands it `NotFound`: a
  failed `GetVolume` or `ChangeIsr` answer is dropped (`flaminio-node/src/engine.rs:1550`,
  `:1518`). An attach for a volume that has a target fails, `already attached` (`:641`).
- **The node, as #15 leaves it** (#15 design, "The shape", S1, S3, S7, S8, S11): an engine
  thread that runs with no device; a registry of entries by volume, locked before the engine
  (no thread takes it while it holds the engine); attaches taken one at a time on the main
  thread; the socket checks a token's signature, volume, node and epoch against the volume's
  run mark before it reserves an entry. The run mark (`DIR/epochs/<volume>`) is written by the
  main thread on the engine's `Output::MarkEpoch`, made durable, and only then does
  `Input::Marked` let the target start and send its first `Hello`. `MarkEpoch`, `Attached`,
  `AttachFailed` and `Detached` go by `(volume, generation)` to the entry's waiter; an unknown
  generation is dropped. The engine verifies the token again and refuses an epoch at or below
  the local fence; device generations from 1; `status`, `list`, `detach`, `delete` on the
  socket. The data plane's client has no attach call (`flaminio-metaclient/src/lib.rs:36-42`).
- **The drain.** `Step` keeps nothing between calls; it lists as unattached only a volume
  with no target (`control/drain/drain.go:113`), and chooses each destination at `:120`. `Run`
  steps every 5 s (`meta/cmd/flaminio-meta/main.go:436`). #78 (approved) makes `Run` call
  `Step`, then `CheckDrain`, and adds `Progress.PendingCreations` and `Incoming`. #13 (approved)
  adds `Progress.Held` (why chunks wait on releases) and `Released`, and forgotten nodes.

## The mark and the transitions (S1–S6, S8, S11)

- **`Header.Hold`** (`json:"hold,omitempty"`): `""`, `client`, `unheld` or `maintenance`.
  A bool cannot tell an unheld record from one written before this change (S6).
  - `Held()`: a target, and `Hold` neither `unheld` nor `maintenance`: `""` with a target,
    `client`, and a value this version does not know (R1-S7), which fails closed.
  - `Maintenance()`: `Hold == "maintenance"`.
- **The table,** the same in `meta/domain` and in `flaminio-repl/src/meta.rs`. Each row is one
  write of the header, conditional on the revision it was read at; nothing else is written.

| Transition | Applies when | Effect |
|---|---|---|
| `Attach` | always | `epoch+1`, target := node, hold := client |
| `Publish` (`unless_target`) | always | nothing if `Held()` and target = node; else as `Attach` |
| `Restart` (`only_if_target`) | target = node | `epoch+1`, hold := client |
| `Unhold` | target = node, `Held()` | hold := unheld |
| `AttachMaintenance` | not `Held()` | `epoch+1`, target := node, hold := maintenance |
| `StartMaintenance` | target = node, hold = maintenance | `epoch+1` |
| `EndMaintenance` | target = node, hold = maintenance, epoch = the caller's | hold := unheld |

- **With #13** (whichever of #13 and #17 lands second): `AttachMaintenance` and
  `StartMaintenance` also refuse a node in #13's forgotten mark, and compare the mark beside the
  header (R1-C6, R1-S10).
- **A lost race** is decided on a new read, after the failed compare.
  - `AttachMaintenance` and a publish: the publish first, and the maintenance attach reads a
    held record: `applied=false`. The maintenance attach first, and the publish reads again
    and applies over it (S5). Either way the client holds the volume.
  - `Unhold` and an attach elsewhere: `applied=false`.
  - `StartMaintenance` and a client attach: `FailedPrecondition`, no token.
  - `EndMaintenance` and anything that moved the record on: `applied=false`.
- **`Restart` on a maintenance record naming its node** is a client attach (S4, S5): a new
  epoch, hold := client. The agent's token is a client epoch's, and its device's attach ends
  the maintenance target on that node (S12). So no agent gets a token of a maintenance epoch,
  and the metadata service refuses none for maintenance (R1-C8, R1-S5; it was refused at
  approval, finding 2); on the node its device waits for the maintenance end, at most 30 s
  (S12, R9-B2).
  `only_if_target` for another node stays refused, as today.
- **`ChangeIsr`, `MoveReplica`, `DeleteVolume`:** untouched (S8). A header being created or
  deleted is `NotFound` for the new calls, as for every call (`readHeader`, `service.go:287`).

## The service's calls and callers (S2, S3, S4, S7, S11)

- **`UnholdVolume`:** the row `Unhold`; answers `applied` and the volume.
- **`AttachMaintenance`:** the node must be registered, as a move's destination
  (`service.go:870`), else `NotFound`; with #13, a forgotten node: `FailedPrecondition`.
  `applied=false`, with the volume and its `held`, is its one refusal that is not an error. No
  token.
- **`StartMaintenance`:** answers the volume and its token, signed as `Attach`'s
  (`service.go:743`): no new claim (question 7). With #13, a forgotten node:
  `FailedPrecondition`, no token.
- **`EndMaintenance(volume, node, epoch)`:** the row `EndMaintenance`; answers `applied`
  (R1-C9, R1-S4).
- **`ListMaintenance(node)`:** one range read of the headers: those with target = node and
  hold = maintenance, not being created or deleted; at most 1024 entries, the first by volume
  name, as #13 caps `ListReleases` (R1-S9). With #13, the node's own `io` when forgotten:
  `FailedPrecondition`.
- **`Authorize`** (`auth.go`):
  - `UnholdVolume`, `AttachMaintenance`: an administrator;
  - `StartMaintenance`, `EndMaintenance`: `id.Component == "io" && id.Node == node` only. Not
    an administrator, not the agent;
  - `ListMaintenance`: an administrator, or `io` of that node;
  - `Attach`: unchanged.
- **The log** (S7): one `slog.Info` after the commit, with `logConfirmation`'s fields
  (`service.go:678`): `"volume unheld"`, `"maintenance attach"`, `"maintenance ended"`, each
  with caller, peer, serial, volume, node, epoch, revision. A refused maintenance attach:
  `"maintenance attach refused"`, the same fields. An unhold or an end that changes nothing: no
  line.
- **`flaminio-meta`:** `unhold <volume> <node>`, `maintain <volume> <node>`.
- **`ControllerPublishVolume`** (R1-S5): an answer whose `Volume.held` is false is an older
  instance's: an older instance's publish of an unheld record naming the node writes nothing
  and never sets `held` (finding 3); a new instance's answer to a publish is always held.
- **`ControllerUnpublishVolume`:** `UnholdVolume(volume, node)`; with an empty node id, the
  record's target. `NotFound`: success. `Unimplemented` is an older instance's, never a
  success, which would leave the volume held for good (R2-C3).
- **An older instance's answer moves on within the call** (R3-B1). `control/csi/meta.go`'s
  `metaCall` (`:63-84`) moves to the next instance only on `Unavailable`, `DeadlineExceeded` or
  `Canceled`, and keeps any other answer's instance as `preferred` (`:82`), so a retry would
  start at the same old instance. The unhold and the publish's attach go through a variant
  given what counts as old (`Unimplemented`; an answer without `Volume.held`): such an answer
  moves on to the next instance in the same call, and never stores that old instance as
  `preferred` (the instance that answers as new is stored, as `metaCall` does: R4-N1). Both are
  safe to repeat on the next instance (a publish to its target changes nothing; an unhold
  applies once). Only when every instance answers as old, or none answers, is the call
  `Unavailable` (`the metadata service is older than this driver`), and Kubernetes retries.
- **`lab.sh vol detach`:** after the node's detach, `metactl unhold $VOL <node>`; a failed
  unhold fails the detach.
- **The CSI node plugin** (`control/csi/node.go`, `device`; S12, R8-L2, R9-B3): the socket
  answers `error: <requested volume>: the maintenance target did not end within 30 s`, which
  `ControlSocket.command` (`dataplane.go:83`) hands the plugin as `data plane: <requested
  volume>: the maintenance target did not end within 30 s`. That exact prefix, matched as #15
  matches `epoch: ` and `limit: `, maps to `Unavailable`, not `Internal`, as #15's map does for
  an attach still in progress, so kubelet retries the stage. The wait is cut by the attach's
  own time limit (`within`, `dataplane.go:107`, the caller's time less `answerMargin`, so the
  data plane answers before the plugin's own deadline): a caller with less than 30 s left is
  answered `error: <requested volume>: the attach's time ran out waiting for the maintenance
  target to end` (R10-2), which the plugin sees as `data plane: <requested volume>: the
  attach's time ran out waiting for the maintenance target to end` and maps, by that exact
  prefix, to `DeadlineExceeded`. Nothing is reserved either way.

## The maintenance node (S9–S14)

Built on #15's engine thread. The decisions are a sans-I/O state machine, `Maintainer`
(`flaminio-repl/src/maintenance.rs`, new), which the engine and the simulator both drive.
- **`--max-maintenance N`** (`node.rs`, `NodeConfig`), default 2; 0 starts none.
- **The scan,** from the engine's turn, every 5 s once the node is registered:
  1. `ListMaintenance(self)`. An entry whose volume fails `ValidID` (#15's socket check) is
     dropped and logged: listed names reach log lines, the socket and the run mark's path
     (R1-S9).
  2. Entries in name order, while starts and targets of kind maintenance are below the limit.
     Passed over: a volume the engine has a target or an attach for, or in #15's
     `pending_delete`.
  3. `GetVolume`: it names this node and is a maintenance record at the listed epoch, else
     nothing. Without work (no chunk with a move): `EndMaintenance` at that epoch, so the
     volume is listed no more, and no epoch is taken (S11; R1-C9, R1-S4).
  4. `StartMaintenance`. The engine hands its answer to the main thread as
     `Output::Maintain { volume, token }` (new), routed after the engine's lock is let go
     (#15's lock order). The main thread takes it in turn with the socket's attaches, on #15's
     attach path without a device (R1-C1, R1-S2). From there the maintenance entry's
     lifecycle (below) decides every step.

  A start that took its epoch and failed is reported once, as `End::StartFailed`: the
  `Maintainer` frees the start's place under the limit, backs the volume off for 5 minutes from
  `now` (the drain's `Stall`) before its next `StartMaintenance`, and sends `EndMaintenance` at
  that epoch, again at each scan that still lists the volume at it, so the record names the
  node no longer and the drain asks again (S17; R2-B1). A node that cannot start a volume (a
  stopped device entry of it, a state directory it cannot write, a malformed mark: #15) so
  takes one epoch per back-off, not one per scan. Any other failure (a metadata call's) is
  logged, and the next scan decides again.
- **The maintenance entry's lifecycle** (S9, S11, S12, S14). Rounds 4 to 6 of the plan review
  each found a new ordering between the main thread, the router, the engine, root's
  `maintenance end` and a client attach; this state machine replaces the prose they patched.
  A maintenance start has one registry entry, of kind maintenance, with a generation G from
  `Node`'s counters (no device id, no record), in one of three states: `starting`, `running`,
  `ending`; `gone` once removed. Each change of state is made under the registry's lock. The
  rules it rests on:
  - **One method makes a maintenance entry, and queues its attach** (R3-C2, R6-B4, R9-B1).
    The main thread checks the token as the socket does (signature, volume, node, an epoch
    above the volume's run mark), then, holding #15's `Mutex<Registry>`, calls
    `Registry::reserve_maintenance(volume, token, &attach_tx) -> Result<Generation, Refusal>`
    (new). It refuses if the volume has any entry, or at `--max-maintenance` (`--max-volumes`
    counts devices only); else it inserts the entry and sends `Input::Attach { volume, token,
    generation: G, kind: Maintenance }` on `attach_tx` before it returns, within the caller's
    one hold. Sending on the engine's channel, an unbounded `mpsc` channel (`node.rs:288`),
    takes no engine lock and cannot block (#15's lock order). It is the only way a maintenance
    entry is inserted, by the compiler (R10-1): `Registry` lives in a module of its own, `mod
    registry` in `flaminio-io/src/ublk.rs` beside #15's `Node`, which holds the registry (#15
    design, "After", and :61). Its entries are a private field, the maintenance `Entry` has a
    private constructor that only `reserve_maintenance` calls, and its methods are
    `pub(super)`: #15's `reserve(volume)`, which makes device entries only,
    `reserve_maintenance`, the lookups, the state's reads and marks, and the removal by
    generation. `pub(super)` reaches `ublk` only (R11-B1): `cli`, `control`, `node`, `proto`,
    `remote`, `replica`, `ublk` and `volume` are sibling modules under the crate root
    (`main.rs:10-21`). `Node`, the main thread and the router are in `ublk.rs` and call those
    methods; `control.rs` (the socket's threads: root's `maintenance end`, a client attach's
    wait) reaches the registry only through `Node`'s own `pub(crate)` methods, and the only
    maintenance insert among them is `Node::reserve_maintenance`, which calls
    `Registry::reserve_maintenance`. The registry's methods are not made `pub(crate)`: any
    module could then insert. How `control.rs` gets `Node` (R12-B2): `control::spawn` takes an
    `Arc<Node>` in place of today's `attached: Attached` (`control.rs:41-45`, an
    `Arc<Mutex<..>>` that #15's registry replaces), beside the `ctl` and `engine` senders it
    keeps. The socket's threads already touch shared state directly today, and #15's socket
    reserves entries itself, so this is the least change. Sending each wait to the main
    thread as a `Control` message instead would block the main thread, which takes attaches
    one at a time, for up to 30 s. `node.rs` only parses `--max-maintenance` into
    `NodeConfig`, from which `Node` is built. `Node` is built first in `serve_node`, where
    `attached` is made today (`ublk.rs:722`), and is handed to `control::spawn` and to the
    startup attach's retry thread (`:749`) (R13-L3). Today's users of `Attached` are #15's to
    replace:
    - the type and its making (`ublk.rs:682`, `:722`): by the registry `Node` holds (#15
      design, "After", :14-18, and :61);
    - the attach and device paths (`:838`, `:939`): by #15's main thread and device thread
      (#15 :79-89, :226-233);
    - `control.rs`'s `serve` and `detach` (`:72`, `:192`): by #15's `detach` over entries
      (#15 :365-374);
    - the startup retry's check (`:752`, a volume attached): by #15's "its retry stops once
      that volume has an entry" (#15 :71, :726). #17 counts a device entry only there, in any
      state (starting, running or stopping), as #15's rule and test say: a maintenance entry
      of the volume does not stop the retry (R13-L3; round 16's "running only" is reverted,
      R17-L1).

    The startup `--attach` is a client attach, and S12 holds for it too (R14-L2). Its attempts
    run on the startup thread (`ublk.rs:749`), not the main thread, and #15 gives it the same
    check and reservation as the socket's (#15 :71), made before the request is queued to the
    main thread. So the startup thread runs S12's step through the same `Node` methods the
    socket's thread uses, in a named function (R15-C1):
    - `startup_attempt(&Node, volume, token, deadline) -> Result<Receiver<String>, String>`
      checks the token as the socket does. If the volume has a maintenance entry, it sends
      `DetachMaintenance{G}` and waits for the entry to be gone (30 s, or its deadline).
      After the wait it re-checks `done()` (past `STARTUP_ATTACH_LIMIT`, or a device entry of
      the volume in any state, read from the registry, as #15's rule: R17-L1) and, if done,
      stops without reserving. Else it reserves and queues its attach as #15 does, and
      returns the reply's receiver. A `cfg(test)` gate sits between that re-check and the
      reserve (R17-L3).
    - Every failed attempt returns `Err(answer)`, one that never reached the main thread
      included (a refused check, a wait that ran out, a refused reserve).
    - The loop at `ublk.rs:745-767` takes `Ok(rx)` to `rx.recv()` as today, and an
      `Err(answer)` as a failed reply: logged like an answer, then the same `done()`, sleep and
      `done()` path.
    - The interval between attempts is a parameter: 5 s in production, milliseconds in the
      tests.

    The first attempt runs in `serve_node` before `control::spawn`, as today's first request
    is sent before the socket opens; it meets an empty registry, so it never waits. The retry
    thread's loop starts from that first attempt's `Result`, not from a receiver (R16-C1):
    `Ok(rx)` goes to `rx.recv()`, and an `Err(answer)` is logged and goes down the same
    `done()`, sleep and `done()` path, so a failed first attempt is retried like any other.

    Whoever sees the entry, root or a client, so sees it with its `Input::Attach` already
    queued ahead of whatever they send. (Round 8's lock-guard hook for the test is cut: as
    designed it would deadlock or recurse, and this holds by construction instead.)
  - **Maintenance events detach by generation** (R5-2, R6-B1). The main thread, root's
    `maintenance end` and S12's client attach end G only by `Input::DetachMaintenance {
    volume, generation: G, why }` (new), which the engine applies only to a
    `Kind::Maintenance` attach or target of that volume and generation; anything else, a
    client's target included, is left alone. `Engine::detach` acts by volume alone
    (`engine.rs:1856-1860`).
  - **The engine's last word on G** (R3-C1). For each G the engine emits `Detached{G}` exactly
    once, as its last output for G: after an attach it refuses (for kind maintenance it gives
    `Detached`, not #15's `AttachFailed`; the engine checks the token, the epoch, the local
    fence, and that the record at that epoch is still a maintenance record naming this node),
    after a start it drops, and after a target's end. In the same step that emits
    `Detached{G}`, and not after it, it reports G's end to the `Maintainer`, once:
    `End::StartFailed` if G never reached `Attached`, else the S11 kind (Ending, below). So the
    `Maintainer`'s state, a failed start's back-off included, already holds when `Detached{G}` is
    taken, before the router's removal (R6-B2, R8-L1). It knows G's epoch from the
    `StartMaintenance` answer it handed over.
  - **Only the router removes** (R2-C2, R5-3). On `Detached{G}` the router, after the engine's
    lock is let go, calls `Node::remove_generation(G)`: under `Node`'s `Mutex<Registry>` it
    removes the entry of generation G, freeing its `--max-maintenance` place, then calls
    `notify_all` on `Node`'s condvar; a second removal of G finds nothing. It marks the entry
    `running` on `Attached{G}`, only from `starting` (R4-C1).
  - **The clock** (R12-B1). `Node` holds an `Arc<dyn Clock>`, a small trait in `ublk.rs` with
    one method, `now() -> Instant`; in production it is the system clock (`Instant::now`).
    #15 adds no clock: this is #17's, and only the 30-s wait below reads it. The main thread's
    wait for a maintenance start stays on real time (`ublk.rs:858`; R13-L2), its limit
    `Node`'s `start_limit` field: `METADATA_LIMIT` in production, set by a `cfg(test)`
    constructor argument (R14-L1). `Node`'s condvar is an `Arc<Condvar>`. The tests' fake clock
    implements `Clock` and holds a slot, a `OnceLock<Arc<Condvar>>`, that the test fills after
    building `Node`, from `Node`'s `cfg(test)` accessor; its `advance` calls `notify_all` on
    the condvar in that slot (R13-L1).
  - **The 30-s wait** (root's `maintenance end`, a client attach; R10-4, R11-B2) is a `Node`
    method, since its condvar pairs with `Node`'s `Mutex<Registry>` (R12-B2). It loops under
    that lock: read the entry's state through the registry's methods (gone: done, `ok` or the
    reserve); then read `Node`'s clock (past the deadline, 30 s or the attach's own limit if
    sooner: the error); else `wait_timeout` on `Node`'s condvar for a bounded real slice, at
    most 100 ms or the time left on the clock, and loop. Every removal and every fake clock's
    `advance` call `notify_all`; the slice bounds a wake that never comes.
  - **The main thread's part ends at `Input::Marked`.** It waits on the entry's waiter, an
    ordered queue (R5-3), only for `MarkEpoch{G}`, `Detached{G}` or its time-out
    (`Node`'s `start_limit`, `METADATA_LIMIT` as #15's), and on each reads the entry's state
    under the registry's lock, acting only on `starting`. The target starts, and `Attached{G}`
    comes, only after
    `Input::Marked`; so no time-out or failed mark of the main thread can race `Attached`, and
    no `Attached` comes after one.
  - **`MaintainFailed` only where no entry was made** (R4-C3). A token refused at the main
    thread's check, or a reserve refused, leaves no entry and no G: the main thread sends
    `Input::MaintainFailed { volume, epoch }` (new), the start's one report, and no detach.

  | State | Event | Next | Who acts, under the registry's lock | Sends | Place |
  |---|---|---|---|---|---|
  | none | main thread: a `Maintain` passes both checks | starting | main thread: `reserve_maintenance` inserts G and queues its attach, in one hold | `Input::Attach{G}` | taken |
  | none | main thread: a `Maintain` refused | none | main thread | `MaintainFailed` | none taken |
  | starting | `MarkEpoch{G}`, the mark written | starting | main thread (the mark's file steps outside the lock); its part ends | `Input::Marked{G}` | kept |
  | starting | `MarkEpoch{G}`, the mark failed | ending | main thread; its part ends | `DetachMaintenance{G}` | kept |
  | starting | the main thread's time-out | ending | main thread; its part ends | `DetachMaintenance{G}` | kept |
  | starting | `Attached{G}` | running | router | nothing | kept |
  | starting | `Detached{G}`: the engine refused the attach, or dropped G after `Input::Marked`, or ended a running target whose `Detached` was routed ahead of its `Attached` (#15 routes them after the engine's lock is let go, on whichever thread took them) | gone | router removes G; the engine reported `StartFailed` if G never reached `Attached`, else the S11 end | nothing | freed |
  | starting | root's `maintenance end` | ending | root's socket thread; answers `ok` once gone (#15's 30 s) | `DetachMaintenance{G}` | kept |
  | starting | a client attach (S12) | ending | client's socket thread; reserves its own entry once gone | `DetachMaintenance{G}` | kept |
  | running | `Detached{G}` (no work, deposed, gone) | gone | router removes G; the engine reported the S11 end | nothing | freed |
  | running | root's `maintenance end` | ending | root's socket thread, as above | `DetachMaintenance{G}` | kept |
  | running | a client attach (S12) | ending | client's socket thread, as above | `DetachMaintenance{G}` | kept |
  | ending | `Attached{G}` (a detach queued after `Input::Marked`) | ending | router: no mark | nothing | kept |
  | ending | `Detached{G}` | gone | router removes G; the engine reported `StartFailed` if G never reached `Attached`, else the S11 end | nothing | freed |
  | ending | `MarkEpoch{G}` | ending | main thread: writes no mark; its part ends | nothing | kept |
  | ending | the main thread's time-out | ending | main thread: its part ends | nothing | kept |
  | ending | root's `maintenance end` | ending | root's socket thread: waits for gone, answers `ok` | nothing | kept |
  | ending | a client attach (S12) | ending | client's socket thread: waits for gone, then reserves | nothing | kept |
  | ending | root's wait passes 30 s (R7-B2) | ending | root's socket thread, reading the state under the lock: still `ending`; the entry stays, and the router removes it on `Detached{G}` | `error: VOLUME: the maintenance target did not end within 30 s` | kept |
  | ending | a client attach's wait passes 30 s, or its own time limit if that comes first (R7-B2, R9-B3) | ending | client's socket thread, reading the state under the lock: still `ending`; reserves nothing | `error: VOLUME: the maintenance target did not end within 30 s`; past its own limit, `error: VOLUME: the attach's time ran out waiting for the maintenance target to end` (R10-2) | kept |
  | gone | `MarkEpoch{G}`, behind the removal | gone | main thread: writes no mark; its part ends; the engine has reported | nothing | none |
  | gone | `Detached{G}`, taken from the main thread's waiter | gone | main thread: nothing; its part ends; the engine has reported | nothing | none |
  | gone | the main thread's time-out, behind the removal | gone | main thread: nothing; its part ends; the engine has reported (R6-B3) | nothing | none |
  | gone | root's `maintenance end` | gone | root's socket thread | `error: VOLUME: no maintenance target here` | none |
  | gone | a client attach | (#15) | #15's attach: no maintenance entry is in the way | nothing | none |
  | gone | `Attached{G}` or `MarkEpoch{G}` routed after the removal | gone | router: dropped, as #15 drops an unknown generation | nothing | none |
  | gone | a second `Detached{G}` | gone | router: finds no entry of G, removes nothing | nothing | none |

  No other cell can occur: `running` has no event of the main thread, whose part ended at
  `Input::Marked`, and the engine gives `Attached{G}` and `Detached{G}` once each. A 30-s wait
  that expires as the entry goes `gone` reads the state under the registry's lock: gone wins,
  so root is answered `ok` and the client reserves (R8-N1). Every cell keeps the four
  invariants:
  - **One end.** G's one report is the engine's, at its one `Detached{G}`; `MaintainFailed` is
    sent only where no G exists. No other actor reports, and `on_ended` also ignores a `(K, epoch)`
    it no longer holds.
  - **One place freed.** Only the router's removal on `Detached{G}` frees the place, once per
    G; a refused start takes none.
  - **A client attach waits for the maintenance end, at most 30 s** (S5, S12, as amended in
    round 8). In `starting`, `running` or `ending` it detaches G or waits, then reserves once G
    is gone, when the engine holds nothing of G: no "already attached" or "attaching" answer
    (R6-B2). The bound is #15's 30 s for a detach: an engine that has not dropped G by then is
    answered as #15 answers a detach that does not finish, `error: VOLUME: the maintenance
    target did not end within 30 s`, with nothing reserved; the plugin maps it to `Unavailable`,
    and kubelet retries the stage (R7-B2, R8-L2). The attach's own time limit, if shorter, cuts
    the wait with `error: VOLUME: the attach's time ran out waiting for the maintenance target
    to end`, `DeadlineExceeded` in the plugin (R9-B3, R10-2). Of two client attaches at
    once, the first detaches G and the second waits for gone (the `ending` row), which ends
    nothing of either client's; the second then meets the first's entry, a conflict between
    clients that #15 answers as today (R6-B1, R7-N1).
  - **No client's target ended by a maintenance event.** Maintenance events send only
    `DetachMaintenance{G}`, which touches only G's maintenance attach or target, and the router
    removes only G's entry.

  Against the findings: S12's detach is `DetachMaintenance` with the entry's generation
  (R6-B1). The main thread removes nothing, so the entry stands until the engine has dropped G;
  after its time-out or failed mark no `Input::Marked` was sent, so no late `Attached` comes,
  and the engine's `StartFailed` holds the back-off; an `Attached` routed while `ending` (root's
  or a client's detach queued after `Input::Marked`) leaves the entry `ending`, and its end is
  the S11 end with root's or the client's reason, rightly, for that start had run (R6-B2). An
  entry gone at the time-out means the engine has already reported: the main thread sends and
  reports nothing (R6-B3). Root's end cannot reach the engine before the start's own
  `Input::Attach`, which is queued in the hold that made the entry visible; so it is applied,
  as root asked, with nothing to remember, rather than refused, which would make root retry a
  race it cannot see (R6-B4).
- **Kinds.** `TargetRt` and `Attaching` carry `Kind::{Client, Maintenance}`.
  - A device's attach whose record is a maintenance record at the token's epoch:
    `error: VOLUME: epoch: a maintenance epoch starts no device` (#15's `epoch:` refusal).
  - A maintenance entry takes no device id, record or slot, and lives as the lifecycle above
    says. A device entry is ended by its device thread; a maintenance entry has none. It has no
    stop channel, and gets no `Deposed` (the engine ends it, below).
  - #15's `status`, `list` and `detach` pass over a maintenance entry, as if the volume had
    none; `delete` is refused while it has one, as for any entry.
- **No client I/O** (S10): `submit` and the fast-read path refuse kind maintenance
  (`NotAttached`).
- **Ending** (S11), of a target that reached `Attached`: as `Engine::detach`
  (`engine.rs:1856`), with `Output::Detached` to its entry. Logged: `VOLUME: maintenance target
  ended at epoch E: <why>`. Then, unless it was `deposed` or `the volume is gone`, the engine
  sends `EndMaintenance` at the target's epoch, which applies only while the record is still
  that maintenance record (R1-C9, R1-S4). A start dropped before `Attached` is no S11 end: the
  engine logs `VOLUME: maintenance start failed at epoch E: <why>` and reports `StartFailed`
  (the lifecycle above).
  - `no work left`: a `GetVolume` answer at the target's epoch, and the target's own view
    (`engine.rs:1558`), both without a move.
  - `deposed`: `drain_one`'s deposed branch (`engine.rs:1841`).
  - `the volume is gone`: a `NotFound` answer to a target's `GetVolume` or `ChangeIsr` now
    reaches it as `MetaReply::NotFound`, whatever its kind (finding 1).
  - `a client attach` (S12): the socket's `attach` of a volume with a maintenance entry, or
    an attempt of the startup `--attach` on the startup thread (R14-L2), verifies the token,
    and requires its epoch above the entry's, else `error: VOLUME: epoch: at or below the
    maintenance target's`, and the target keeps running (R1-S8). Then the lifecycle's
    client-attach row, through the same `Node` methods: `DetachMaintenance` with the entry's
    generation, a wait for the entry to be gone (#15's 30 s), and its own reserve, within
    #15's limits.
  - `ended by root`: `maintenance end VOLUME`, the lifecycle's root row.
- **Restart** (S13): everything above goes with the process; the run mark stays, so an old
  maintenance token is refused (#15, S11). `StartMaintenance` always takes a new epoch.
- **`delete VOLUME`** stays refused while the engine has a target (#15 design, S7): a volume
  deleted under maintenance is cleaned after its target ends, within a poll.

## The drain (S15–S18)

`control/drain/drain.go`. A `Drainer` holds the client, the node, `Stall` (5 min), `Now`, and
two records per volume:
- **its sighting:** that node, an epoch and a time, set together at this run's first sight of a
  maintenance record naming a node, and set again together by the drain's own applied ask of
  that node: the epoch its answer returned, the time of the ask (R3-C3, R4-C4). The node's own
  new epoch resets neither; the sighting is dropped when the drain asks another node or the
  volume is held. It also keeps whether, at this run's last read, the record was that node's
  maintenance record with a move pending. The drain's own applied `AttachMaintenance` counts
  as such a read: its answer is that node's maintenance record, and the drain asked because a
  move was pending (R4-C2);
- **its progress:** the volume's replicas on the node, and when they last went down.

`Run` keeps its signature, holds one `Drainer` and calls its `Step`. The package `Step` stays,
with its signature, as one step of a new `Drainer` (no clocks, so it asks again only for a
node that is dead or not eligible); #78's `Run` calls it, and whichever of #17 and #78 lands
second makes `Run` hold a `Drainer` and call `Drainer.Step` before #78's `CheckDrain` (R1-C5).
- **Per volume with a replica on the node:** its moves requested as today (`leastLoaded`,
  `:120`: #18's seam; no source is deleted, and a chunk waiting on releases waits: #13's).
  Then, on the volume read again:
  - **held:** nothing. State `in use`, with its target.
  - **not held, no move pending** (each of its chunks on the node waits on #13's releases):
    nothing asked. State `waiting`, `no move pending`; #13's `Held` says why (R1-C6).
  - **not held, a move pending, no maintenance attach standing:** `AttachMaintenance` to the
    first candidate (but see the failed start below); if that is the sighting's node (it ended
    at no work, then a move came), the sighting goes on, set again by this ask.
    `applied=false`: held now, not an error. `NotFound`: gone, or the node unregistered.
    `FailedPrecondition`: the node forgotten (#13), the next candidate at the next step.
    `Unimplemented`: `waiting`, `the metadata service is older`.
  - **A maintenance attach stands** while the record is a maintenance record naming a node
    eligible below, whoever asked for it: this run, another drain, `flaminio-meta maintain`,
    #45's rebuild (R1-C3, R1-S3). State `maintenance on NODE`.
- **The node** (S16): alive, not draining, not a witness, and in some chunk's `replicas` or
  `moving_to`. Ranked: most chunks of the volume, then name. None: `waiting`, `no eligible
  node`. A forgotten node is draining, by #13's mark.
- **Asking again** (S17). The drain counts no epochs: it cannot tell a node's starts from its
  own asks, or from a start retried after a lost answer (R3-C3). It asks another candidate:
  - when the node is no longer alive or eligible: at once, if there is another;
  - **after a failed start** (R2-B1, R3-C3): at the last read (the drain's own ask's answer
    included, so a start that fails within one step is seen at the next: R4-C2) the record was
    the sighting's node's maintenance record with a move pending, and now it is no longer a
    maintenance record: its maintenance ended (unheld, still naming that node), and the move is
    still pending (R5-1). A record still that node's maintenance record, at the same epoch or a
    newer one, is no failed start: it stands (the node has not started yet, or has). That
    node's end with work left does this (a failed start, or root's end), or an end at
    no work whose read came before the move was requested; in neither case does a target run,
    so asking another node restarts no copy. At once, if there is another; with none, that
    node again, whose back-off then takes no epoch;
  - when `Stall` has passed since both the sighting's time (its first sight, or the drain's own
    last ask of that node) and the replicas last went down, and the record's epoch is still the
    sighting's (the node never started: a full node, a node in back-off, an older data plane).
    With none, the sighting starts again.
  - Otherwise a node that started is not asked again while it stays alive and eligible,
    however long a chunk's copy takes: a new attach would depose its target, and the
    destination's resync would start over (protocol §7; R1-C7).
- **The node undrained:** no maintenance attach is asked while the step's `ListNodes` shows the
  node not draining, and `Run` ends with `node N is no longer draining` (R1-S3); once #78
  lands, its stale drain generation ends `Run` first.
- **`Progress`:** `Remaining`, `Moving`; #78's `PendingCreations` and `Incoming`; #13's `Held`
  and `Released`; and `Volumes []VolumeState{ID, State, Node, Why, Since}`, `Since` being when
  its replicas on the node last went down, for every volume left, in use or not (R1-C10).
  `Unattached` goes. Completion: #78's `CheckDrain` once #78 lands; until then `Remaining ==
  0`, as today (R1-C5).

## Rollout (S19)

1. Every metadata instance. 2. The callers of `UnholdVolume` (the CSI controller, `lab.sh`)
and the data planes. 3. `flaminio-meta drain`. Each step waits for all of the one before
(R2-C3; round 1 had let step 2 start early).
- **An older instance** answers `Unimplemented` to the new calls; the CSI controller's unhold
  moves on to the next instance, and answers `Unavailable` only when every instance is old, so
  a misordered rollout delays unpublishes and loses no unhold.
  Its `Attach` rewrites the header without `hold`: held, by S6. Its publish of an unheld record
  naming the node writes nothing and answers without `held`: the CSI controller answers
  `Unavailable`, and Kubernetes retries until a new instance answers (finding 3, enforced in
  code: R1-S5). Within one call, an old instance's answer moves on to the next instance
  (R3-B1), so a mixed window costs nothing while one new instance answers.
- **Rolling back** (R3-B1): the CSI controller and `lab.sh` before the metadata service, the
  reverse of steps 1 and 2. A metadata service older than the CSI controller fails every
  publish and every unpublish (`Unavailable`, retried by the external-attacher without end),
  so a VolumeAttachment keeps its finalizer and a ReadWriteOnce volume cannot attach elsewhere.
- **An older data plane** never lists; it takes no epoch, so the drain asks elsewhere after
  `Stall`.
- **A newer data plane on an older service:** `Unimplemented`, logged once, scanned again.

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): S1–S7, S11.
- `meta/domain/domain.go`: `Hold`, `Held`, `Maintenance`, the table's methods (S1–S6).
- `meta/service/service.go`: `Attach` on the table, the five calls, the log, `volumePB`
  (S1–S8, S11). `meta/service/auth.go` (S7).
- `meta/cmd/flaminio-meta/main.go`: `unhold`, `maintain`, the drain's line (S2, S3, S18).
- `control/csi/controller.go`, `control/csi/meta.go`: the unpublish's unhold (S2); the
  publish's check of `held` (S19); the call variant that moves past an older instance (R3-B1).
  `control/csi/node.go` (`device`): `data plane: <volume>: the maintenance target did not end
  within 30 s` to `Unavailable`, and `data plane: <volume>: the attach's time ran out waiting
  for the maintenance target to end` to `DeadlineExceeded`, each by exact prefix (S12, R8-L2,
  R10-2, R11-B3).
- `control/drain/drain.go` (S15–S18).
- `dataplane/crates/flaminio-repl/src/meta.rs`, `src/maintenance.rs` (new), `src/lib.rs`,
  `examples/meta_vectors.rs`, `src/sim/mod.rs`, `src/sim/tests.rs`: the model and its checks.
- `meta/testdata/isr-vectors.jsonl` (`make meta-vectors`).
- `dataplane/crates/flaminio-metaclient/src/pb.rs`, `src/lib.rs`, `tests/interop.rs` (S4, S9).
- `dataplane/crates/flaminio-node/src/engine.rs`, `tests/engine.rs` (S9–S14).
- `dataplane/crates/flaminio-io/src/ublk.rs` (`mod registry` beside #15's `Node`: the
  registry's kind, `reserve_maintenance`, `remove_generation`, all `pub(super)`; `Node`'s
  condvar beside its `Mutex<Registry>`, its `Arc<dyn Clock>` and the `Clock` trait with the
  system clock (R12-B1); `Node`'s `pub(crate)` wrappers for `control.rs`, the 30-s wait
  among them, with `Node::reserve_maintenance` the only maintenance insert (R11-B1, R12-B2);
  the main thread's maintenance start; the router's removal), `src/node.rs`
  (`--max-maintenance` into `NodeConfig`), `src/control.rs` (`spawn` takes an `Arc<Node>` in
  place of `Attached`; S9, S12, S14).
- `scripts/lab.sh` (`vol detach`, `drain-unattached`), `scripts/checks/drain-unattached.sh`
  (new), `Makefile`.
- Docs: see below. `docs/adr/README.md`: 0014's row, Proposed (added with the plan, after
  approval).

## Contracts

**`meta.proto`.** `Volume` gains `bool held = 16;` and `bool maintenance = 17;`: 9 and 10 are
#79's, 11–14 #70's, 15 #13's (R1-C4, R1-S6). `AttachRequest` and `AttachResponse` are
unchanged. New, in `MetadataService`:

```proto
// Records that a client's attach on node has ended. Applied only if node is the target and
// the volume is held. Changes no epoch. Caller: an administrator.
rpc UnholdVolume(UnholdVolumeRequest) returns (UnholdVolumeResponse);
// Makes node the target of a volume that no client holds, under a new epoch, for
// maintenance. Issues no token. Caller: an administrator.
rpc AttachMaintenance(AttachMaintenanceRequest) returns (AttachMaintenanceResponse);
// Takes a new epoch and its token, only while node is the maintenance target. Caller: that
// node's data plane.
rpc StartMaintenance(StartMaintenanceRequest) returns (StartMaintenanceResponse);
// Ends node's maintenance of the volume: applied only while the record is a maintenance record
// naming node at epoch. Changes no epoch. Caller: that node's data plane.
rpc EndMaintenance(EndMaintenanceRequest) returns (EndMaintenanceResponse);
// Volumes that name node as their maintenance target: at most 1024, the first by name. Caller:
// that node's data plane, or an administrator.
rpc ListMaintenance(ListMaintenanceRequest) returns (ListMaintenanceResponse);

message UnholdVolumeRequest { string volume = 1; string node = 2; }
message UnholdVolumeResponse { bool applied = 1; Volume volume = 2; }
message AttachMaintenanceRequest { string volume = 1; string node = 2; }
message AttachMaintenanceResponse { bool applied = 1; Volume volume = 2; }
message StartMaintenanceRequest { string volume = 1; string node = 2; }
message StartMaintenanceResponse { Volume volume = 1; bytes token = 2; }
message EndMaintenanceRequest { string volume = 1; string node = 2; uint64 epoch = 3; }
message EndMaintenanceResponse { bool applied = 1; }
message ListMaintenanceRequest { string node = 1; }
message MaintenanceEntry { string volume = 1; uint64 epoch = 2; }
message ListMaintenanceResponse { repeated MaintenanceEntry entries = 1; }
```

**Codes.** `InvalidArgument`: an id. `NotFound`: no such volume, or one being created or
deleted; `AttachMaintenance`'s unregistered node. `FailedPrecondition`: `StartMaintenance` off
the table; `only_if_target` for another node, as today; with #13, a forgotten node in
`AttachMaintenance` and `StartMaintenance`, and the forgotten node's own `ListMaintenance`.
`Aborted`: 32 lost compares. `PermissionDenied`: the caller.

**The Rust client.** `pb.rs`: `StartMaintenance*`, `EndMaintenance*`, `ListMaintenance*`,
`MaintenanceEntry`, with the tags above, and `Volume`'s 16 and 17. `Call::{StartMaintenance,
EndMaintenance, ListMaintenance}` and the same three `Answer`s.

**The model** (`meta.rs`). `VolumeMeta.hold: Hold {Unmarked, Client, Unheld, Maintenance}`;
`held()`, `has_work()`. `MetaRequest::{Publish, Restart, Unhold, AttachMaintenance,
StartMaintenance}`, each `{vol, node}`, and `EndMaintenance {vol, node, epoch}`. A refusal is
`Rejected` with the view. `meta::Bugs { maintenance_ignores_held, start_ignores_hold,
unhold_any_node, publish_keeps_unheld, end_any_hold, restart_keeps_maintenance }` (tests;
below).

**`Maintainer<K>`** (`maintenance.rs`; `K` is the volume's key: its name in the engine).
`new(node, limit)`. In: `on_list(now, &[(K, Epoch)])`, `on_view(now, K, Option<&VolumeMeta>)`,
`on_started(K, &VolumeMeta)`, `on_ended(now, K, Epoch, End)`, and `busy(K, bool)`: the
engine has a target or an attach of `K`, or is deleting it. Out: `Get(K)`,
`StartMaintenance(K)`, `EndMaintenance(K, Epoch)`, `Start {vol, view}`, `End {vol, why}`.
`End::{NoWork, Deposed, Gone, Detached, StartFailed}` (`Detached`: root's end, or a client's
attach, of a running target; `StartFailed`: a start that took its epoch and did not run,
after which `K` gets no `StartMaintenance` for 5 minutes from `now`: R2-B1, R3-N2).
`on_ended(now, K, epoch, End::StartFailed)` comes from the engine, at a generation's
`Detached` if it never reached `Attached`, or from the main thread's `Input::MaintainFailed {
volume, epoch }` for a start refused before its entry (the lifecycle). `on_ended` ignores a
`(K, epoch)` it no longer holds (R3-C1).

**`Input`s** (the engine's, new): `Attach` gains `kind`; `DetachMaintenance { volume,
generation, why }`; `MaintainFailed { volume, epoch }`. **`Output`:** `Maintain { volume,
token }`. **`Engine::maintenance()`** (new, read-only, as `holding()` is): the `Maintainer`'s
state, for the engine test (R9-B4). **`mod registry`** (in `ublk.rs`; R10-1):
`pub(super) fn reserve_maintenance(&mut self, volume, token, &attach_tx)`, the one maintenance
insert (R9-B1), and `pub(super) fn remove_generation(&mut self, G)`, which frees the place.
**`Node`** (in `ublk.rs`; R11-B1, R12-B2): beside #15's `Mutex<Registry>`, an
`Arc<Condvar>`, with a `cfg(test)` accessor that clones it (R13-L1), an `Arc<dyn Clock>`,
and `start_limit: Duration` (`METADATA_LIMIT`; a `cfg(test)` constructor argument sets it:
R14-L1); `pub(crate)` wrappers for `control.rs` and the startup thread, among them
`Node::reserve_maintenance`, the only maintenance insert outside `ublk`, and the 30-s wait;
`Node::remove_generation(G)` (the router's), which calls the registry's under the lock, then
`notify_all`, and a form of it taking the held guard, for the test. **`Clock`** (in
`ublk.rs`; R12-B1): `trait Clock: Send + Sync { fn now(&self) -> Instant; }`, the system
clock in production. `control::spawn` takes an `Arc<Node>` in place of `Attached`.

**Vectors.** New `op`s, each with `node`: `publish`, `restart`, `unhold`,
`attach_maintenance`, `start_maintenance`, and `end_maintenance` with `epoch`. Every `after`
gains `"hold"`: `""`, `"client"`, `"unheld"`, `"maintenance"`. `create` gains `"target"` and
`"hold"`: one seed in four starts with a target and no hold (S6).

**The control socket.** #15's lines are unchanged, and pass over a maintenance entry: `status
VOLUME` answers `ok none`, so the plugin attaches (S12). New, names checked as #15's:

```text
maintenance              ->  ok limit N [VOLUME STATE EPOCH ...]     (starting|running|ending, by name)
maintenance end VOLUME   ->  ok | error: VOLUME: no maintenance target here
                             | error: VOLUME: the maintenance target did not end within 30 s
```

#15's `attach VOLUME TOKEN-BASE64 [MS]` over a maintenance entry answers that same last line
when the entry is not gone within 30 s, and reserves nothing (R7-B2); if its own `MS` runs out
first, it answers `error: VOLUME: the attach's time ran out waiting for the maintenance target
to end`, and reserves nothing (R9-B3, R10-2).

`maintenance end` follows the lifecycle's root rows: `DetachMaintenance` with the entry's
generation, `ok` once the entry is gone; the engine then ends the record's maintenance (S11).

**CLI.** `flaminio-io node --max-maintenance N`. `flaminio-meta unhold|maintain <volume>
<node>`: the response as JSON, exit 0 when `applied` is false too. The drain's line, on
stderr: `N chunk replicas left, M moving`, then #78's and #13's parts as they print them, then
per volume `; V: in use on NODE`, `; V: maintenance on NODE`, or `; V: waiting: WHY`, each
followed by `, no progress for D` once `D`, the time since its replicas on the node last went
down, passes `Stall`. `WHY`: `no eligible node`, `no move pending`, `the metadata service is
older`.

## Tests

- **S1–S6, S8** (`meta.rs` unit tests; `meta/domain/domain_test.go`, `TestVectors`): every
  row applied and refused; a record without the mark, with and without a target; a `hold`
  value this version does not know, held (R1-S7).
- **S1–S8, S11** (`meta/service/service_test.go`, `stores`: in memory and on etcd):
  - a publish repeated on a held volume; a publish of an unheld volume to its target takes
    an epoch; an unhold for another node, and a second one;
  - `AttachMaintenance` on a held, an old, a never attached, an unheld, a deleting and a
    missing volume, to an unregistered node, and over a maintenance record;
  - `StartMaintenance` off the table; its token's claims; `Attach` in each mode over a
    maintenance record, `only_if_target` from its node taking a client epoch (R1-C8);
    `ChangeIsr` by the maintenance node; `DeleteVolume` under maintenance; `ListMaintenance`,
    and its cap;
  - `EndMaintenance` at its epoch, at another, on a client and an unheld record, for another
    node;
  - with #13: `AttachMaintenance` naming a forgotten node, and `StartMaintenance` and
    `ListMaintenance` by its `io`: `FailedPrecondition`; a forget racing a `StartMaintenance`;
  - races (`interleaveStores`): a maintenance attach and a publish, in both orders; an unhold
    and an attach; a `StartMaintenance` and a client attach; an `EndMaintenance` and a client
    attach;
  - each log line, its fields, and none on an unhold or an end that changes nothing.
- **S7** (`meta/service/auth_test.go`): each new method as an administrator, as `agent` and
  `io` of the node and of another, as `meta`.
- **S19** (`service_test.go`): a header rewritten without `hold`, over an unheld and over a
  maintenance record, is held. `main_test.go`: `unhold`, `maintain`.
  `control/csi/csi_test.go`: the unpublish unholds; an empty node id; `NotFound`, a success;
  with one instance, an unhold answered `Unimplemented` and a publish answered without `held`
  are `Unavailable` (R2-C3, R1-S5); with two, the first old and `preferred` starting at it, a
  publish and an unpublish both succeed through the second, in one call, and `preferred` is
  never set to the old instance (R3-B1, R4-N1); the node plugin's stage, its fake socket
  answering `error: VOL: the maintenance target did not end within 30 s` through the real
  `ControlSocket.command`, so that `device` sees `data plane: VOL: ...`, is `Unavailable`; the
  same words about another volume are not taken for it; its fake socket answering `error: VOL:
  the attach's time ran out waiting for the maintenance target to end`, likewise through
  `command`, gives `DeadlineExceeded` (R8-L2, R9-B3, R10-2).
- **`make contracts`:** the vectors regenerate with no diff; `interop.rs` lists, starts and ends
  as `io` against the Go service, and reads `held` and `maintenance`.
- **Simulator** (`sim/mod.rs`, `sim/tests.rs`). The new chaos runs only under
  `Config::maintenance(seed)`: `Config::random`, and every seed and hint of today, are
  unchanged.
  - **Chaos adds,** at random times and whatever the record says (R1-C2, R1-S1): unholds naming
    any node, the target included; maintenance attaches to any node; and a `Maintainer` per
    node, whose starts and ends reach the model through `META` later than the list and the view
    they come from, as a drain's read races a publish. Failovers use `Publish` and the target
    node's `Restart` as well as `Attach`, as the CSI controller and the agent do. No client
    submits to a maintenance target.
  - **New checks,** against the simulator's own record of holds, kept from the requests the
    model applied, under its own rules, each as the model should admit it: a client attach
    holds; an unhold leaves it not held only from its holder; a maintenance attach or start
    makes it a maintenance record at its node and epoch; an end leaves it not held only at the
    node and epoch it last recorded as maintenance (R2-C1). `maintenance over a held record`
    runs before the record takes the request: an `AttachMaintenance`, a `StartMaintenance` or
    an `EndMaintenance` that applies while that record says held. `hold lost` runs after the
    record takes the request: the model's `held()` differs from the record's (R3-C4). A
    maintenance target run
    twice at one epoch is #15's check (two target incarnations at one epoch), which covers
    maintenance starts.
  - **Campaign:** `campaign_with_maintenance`: the usual seeds in `make test`, 0..2 000 in
    `make sim`.
  - **Injected bugs** (`mutation!`, seeds 0..20 000 of `Config::maintenance`, `make sim`):
    `catches_maintenance_over_held` (`meta::Bugs::maintenance_ignores_held`),
    `catches_start_over_held` (`meta::Bugs::start_ignores_hold`: `StartMaintenance` applied
    whatever the hold, for its target),
    `catches_unhold_for_another_node` (`meta::Bugs::unhold_any_node`),
    `catches_publish_keeping_unheld` (`meta::Bugs::publish_keeps_unheld`: a publish to the
    named node of an unheld record changes nothing),
    `catches_end_over_held` (`meta::Bugs::end_any_hold`: `EndMaintenance` applied for its
    node whatever the record's hold and epoch),
    `catches_restart_keeping_maintenance` (`meta::Bugs::restart_keeps_maintenance`: `Restart`
    on a maintenance record naming its node keeps hold = maintenance, the row as approved;
    R2-C4),
    `catches_maintenance_without_run_mark` (`sim::Bugs::maintenance_skips_run_mark`: a
    maintenance start neither checks nor raises the node's run mark; #15's chaos event that
    hands a restarted node its last start hands maintenance starts too). Each one's first seed,
    from `scripts/dev/mutation-search.sh`, is written above it.
  - **Scenarios** (scripted): each bug's shortest run; a client attach over a running
    maintenance target, whose move the client's target ends; the agent's `only_if_target`
    restart on the maintenance node, taking a client epoch (R3-N1).
- **S9–S14** (`maintenance.rs` unit tests; `flaminio-node/tests/engine.rs` and
  `flaminio-io`'s node tests, a fake metadata client):
  - each condition of S9 failing starts nothing; no work takes no epoch, and ends the record's
    maintenance;
  - the run mark (R1-C1, R1-S2): no `Hello` at E before `Input::Marked`; with a recording
    `MarkFs`, #15's file steps happen before `Input::Marked`; a failed mark lets no `Hello`
    leave;
  - **`maintenance_entry_lifecycle`** (`flaminio-io`'s node tests, with the engine's outputs
    and the socket's commands scripted, and a fake clock; R6, R7-B1): the node's half only.
    The node under test is built with its own `mpsc::channel::<Input>()`, and the test keeps
    the receiver, so it reads what the node sent with `try_recv` (R10-5).
    Table-driven, one case per row of the lifecycle, each row one event. Each case drives an
    entry to the row's state, delivers the row's event, and checks the next state, what the
    node sent on the engine's channel, the place count, the socket's answer and who still
    waits. After every case it checks the node's side of the invariants: one place freed per
    entry; no client attach refused or made to wait except by the 30-s bound (no `limit` or
    `is starting` answer, no reserve while an entry of the volume stands, and an error only
    after 30 s: R9-B2); and every detach a `DetachMaintenance` with the entry's generation.
    Named cases, each failing without its rule:
    - a `Detached` routed right after `Attached`, before the main thread reads its waiter;
      the entry and its place are gone (R4-C1). A `Detached` routed ahead of its `Attached`:
      removed once, the late `Attached` dropped (R7-N2);
    - a time-out with the entry `starting`: `DetachMaintenance{G}` sent, no `Input::Marked`;
      a client attach meanwhile waits, and reserves only after `Detached{G}` (R6-B2);
    - a time-out behind the removal, and a `Detached{G}` taken from the waiter: nothing sent
      (R6-B3). These two cases run on real time, as the main thread's wait does (R13-L2), with
      `Node` built with a `start_limit` of about 200 ms (R14-L1);
    - **a startup attach over a maintenance entry** (R14-L2, R15-C1), driving the startup loop
      with an interval of a few milliseconds:
      - with a running maintenance entry of the volume, `startup_attempt` sends
        `DetachMaintenance{G}`, waits for the entry to be gone, then reserves and queues its
        attach on the test's receiver, and the volume attaches; the retry does not stop on the
        maintenance entry, and the main thread's channel is served throughout the wait;
      - a wait that runs out returns `Err`, logged, and the next attempt follows after the
        interval; a device entry of the volume, in any state, made during the wait ends the
        retry at its `done()` re-check, with no reserve (the "stops at `done()`" case, apart
        from the refusal below: R17-L3); a failed first attempt (`Err` before the retry thread
        starts) is logged and retried after the interval (R16-C1);
      - the startup retry and a socket client attach at once over one maintenance entry
        (R16-C1, R17). The test lines both up with the `cfg(test)` waiter count (both blocked
        in the 30-s wait, the count at 2), then removes the entry. Two `cfg(test)` gates order
        the reserves: the startup runner's sits between its `done()` re-check and its reserve
        (R17-L3), the socket runner's after its wait and before its reserve. Each runner
        raises a `cfg(test)` "reached gate" signal when it parks at its gate (R18-L1). The
        test opens the chosen winner's gate first. It holds the winner's metadata phase
        unanswered, so its entry stays `starting`, until the loser's reserve is refused, then
        releases it (R17-L2). Before it opens the loser's gate, the test waits, with a bounded
        timeout, until the registry shows the winner's device entry `starting`, read the way
        this test already reads entry states and the place count, so the winner has reserved
        (R19-L1, R20). Both orders run:
        - socket first: the test waits for the startup runner's "reached gate" signal, so that
          its `done()` re-check has passed before the socket client reserves, and only then
          opens the socket gate. The socket client reserves; the startup gate opens, and its
          reserve is refused with #15's answer over a starting entry, `error: VOLUME is
          starting here` (#15 design.md:78, the "already attaching" case). `startup_attempt`
          returns `Err`; the retry logs it and ends at the first `done()` after the `Err`,
          while the socket client's entry is still `starting` (held), with no further attempt
          (R18-L3). Then the socket client's metadata phase is released, and it attaches. Had
          that held attach failed instead, the startup `--attach` would have ended and nothing
          would retry it: #15's rule, and #15's accepted cost;
        - startup first: the startup attempt reserves; the socket client's reserve is refused
          with that same answer; then the startup's metadata phase is released, and it
          attaches.

        In both, one `DetachMaintenance{G}` is sent and one reserve made. The refusal is a
        conflict between two clients, as on #15, consistent with S5 as narrowed: not a refusal
        for maintenance;
    - **`reserve_maintenance`** (R6-B4, R9-B1): when it returns `Ok(G)`, the test's receiver
      already holds `Input::Attach{G}` (read with `try_recv`, no other step between); a refusal
      queues nothing. A reserve that queued the attach later fails it. That no other path
      inserts a maintenance entry is the compiler's: the entry's constructor and the entries
      are private to `mod registry`, its methods are `pub(super)`, so only `ublk` calls them,
      and `control.rs` reaches the registry through `Node`'s `pub(crate)` wrappers, whose only
      maintenance insert is `Node::reserve_maintenance` (R10-1, R11-B1, R12-B2);
    - two client attaches at once over a maintenance entry: one `DetachMaintenance{G}` sent,
      the second waits (R6-B1, R7-N1); a client attach over a start in flight, its detach
      queued first (R3-C2);
    - **the 30 s** (R7-B2): the scripted engine withholds `Detached{G}`; past 30 s on the fake
      clock root's end and a client attach each answer `error: VOLUME: the maintenance target
      did not end within 30 s`, the client reserves nothing, the entry stays `ending`, and a
      later `Detached{G}` removes it and frees its place;
    - **gone wins at 30 s** (R8-N1, R9-B5, R10-4, R11-B2): root's `maintenance end` and a
      client attach wait, in `Node`'s loop the lifecycle's rules give. The node is built with
      a fake `Clock`, whose condvar slot the test fills from `Node`'s accessor once `Node` is
      built, so that its `advance` calls `notify_all` on `Node`'s condvar (R12-B1, R13-L1). The
      main thread's waits stay on real time (R13-L2). The test
      waits until both waiters are blocked (a `cfg(test)` count of threads in that wait, raised
      under the lock just before it, reaching 2), then takes `Node`'s registry lock. In that
      one hold it calls the router's own removal in its held-guard form, which frees the place
      and calls `notify_all`, and moves the fake clock past 30 s; then it lets the lock go.
      Root is answered `ok`, the client reserves, and the place is freed once. A wait that
      reads the clock before the state fails it;
    - a `Maintain` refused over a stopped device entry of the volume: `MaintainFailed` sent, no
      detach, the device entry untouched (R4-C3).
  - **`maintenance_generation_end`** (`flaminio-node/tests/engine.rs`, the real engine and a
    fake metadata client; R7-B1): the engine's half, which the node test cannot see. Each way G
    ends, on its own: an attach refused (a bad token, another epoch, the local fence, a record
    no longer a maintenance record naming the node); `DetachMaintenance{G}` before the
    `GetVolume` answer, between `MarkEpoch` and `Input::Marked`, and after `Attached`; no work;
    deposed; the volume gone. For each:
    - exactly one `Detached{G}`, the last output for G, and exactly one report, seen in the
      `EndMaintenance` calls on the fake metadata client, counted, and in
      `Engine::maintenance()`'s state: one place freed, and after `StartFailed` one back-off
      (R8-L1, R9-B4; the engine logs with `eprintln!`, so log lines are not counted: R10-3);
    - the report holds when `Detached{G}` is taken (R9-B4): straight after the `handle` or
      `turn` whose `take_output()` holds `Detached{G}`, before any further input or turn, the
      test reads `Engine::maintenance()` (new, read-only, as `holding()` is): after
      `StartFailed`, the volume backed off; after any end, its place free. An engine that
      reports at the head of the next step (the injected late reporter) fails it;
    - `StartFailed` exactly when `Attached{G}` never came (a back-off in
      `Engine::maintenance()`), else the S11 kind (none); no `AttachFailed` for kind
      maintenance;
    - `EndMaintenance` at G's epoch, as S11 says: none after deposed or gone; after a
      `StartFailed`, no `StartMaintenance` for that volume for 5 minutes on the fake clock (one
      back-off, no second epoch: R2-B1);

    and, in the same test (R5-2 restored): a `DetachMaintenance` of another generation, one
    over a client's target of the volume, and one after G ended each emit nothing and leave
    the target running; a `Marked{G}` after G was dropped starts nothing and sends no `Hello`.
    An engine whose `DetachMaintenance` acts by volume, or reports in a later step than
    `Detached{G}`, or reports twice, or emits `AttachFailed`, fails it.
  - **`maintenance.rs` unit tests** (R8-L4): an `on_ended` repeated for a `(K, epoch)`, and
    one for a `(K, epoch)` the `Maintainer` no longer holds, change nothing: one place freed,
    one `EndMaintenance`, one back-off.
  - no device and no slot: the entry counts against `--max-maintenance` only; `Io` and a fast
    read refused;
  - the end at no work, on a newer epoch, and on `NotFound` (a client's target too);
    `EndMaintenance` after the end at no work and root's end, none after deposition or a gone
    volume;
  - not started again without work, started once a move is requested and a new maintenance
    attach names the node;
  - the socket's attach over a maintenance target, and over a start in flight; a token at or
    below its epoch refused, the target running on (R1-S8);
  - a device's attach on a maintenance epoch refused;
  - after a restart, a new epoch; an old token refused at the run mark;
  - a listed volume id that fails `ValidID`: dropped and logged, the others started (R1-S9);
  - three named, two run, the third after one ends; `--max-maintenance 0`;
  - `control.rs`: both lines; root's end ends the record's maintenance; `status`, `list` and
    `detach` pass over a maintenance entry.
- **S15–S18** (`control/drain/drain_test.go`, in-memory service, a fake clock):
  - in use and not in use volumes; the ranking; none eligible, and the drain not complete; a
    refusal as held; a forgotten candidate; a volume deleted meanwhile; an older service;
  - a not held volume whose chunks on the node all wait on releases: no maintenance attach,
    `no move pending`, #13's reason shown (R1-C6);
  - a dead node, asked again at once; a node that took no epoch for `Stall`, asked again
    elsewhere;
  - a node that took an epoch, its chunk still copying past `Stall`: not asked again (R1-C7);
  - a failed start (the record, the node's maintenance record with a move pending, now no
    longer a maintenance record, the move still pending): asked elsewhere at once (R2-B1,
    R3-C3); a start that fails between the drain's ask and its next step, with no read scripted
    between them: asked elsewhere at that next step (R4-C2); an ask applied and the next step
    finding the record still standing at the same epoch (the node not yet started), the flag
    true from the ask: nothing asked (R5-1);
  - a node re-asked after an end at no work, and a start retried after a lost answer (two
    epochs), each copying past `Stall`: not asked elsewhere (R3-C3); a node re-asked when both
    clocks were long past `Stall`, and not yet started at the next step: not asked elsewhere
    before `Stall` from that ask (R4-C4); the sighting's time kept across the node's new
    epoch;
  - two `Drainer`s sharing a volume, and a `Drainer` started over an existing maintenance
    record: no second ask while the replicas go down, and at most one maintenance epoch per
    `Stall` (R1-C3, R1-S3); the node undrained: no ask, `Run` ends;
  - the no-progress clock, on an in-use volume and under maintenance (R1-C10);
  - each line, #78's and #13's parts kept; completion, on `CheckDrain` with #78.
- **S20:** review. **Targets:** `make lint test race contracts sim`.

## Lab

A later step, not run with the build. `make drain-unattached` (new), with fl-c up:
1. Two volumes with replicas on fl-b: one written with verified data on fl-a, then detached
   (`vol detach` unholds it); one never attached.
2. `metactl drain fl-b` completes, and each kind of line is seen.
3. Each volume is attached on fl-a: the data verifies, and no chunk names fl-b.
4. A second round (R1-C11): `metactl undrain fl-b`; two new volumes as in step 1; `metactl
   drain fl-b` again. Once every replica on fl-b has its move requested (the line shows as many
   moving as left: a node down before that leaves a chunk "no node to move to", which ends
   today's drain), kill the data plane of the node whose `maintenance` socket line lists a
   running target (fl-a or fl-c), and start it again a minute later. The drain completes, the
   maintenance target started again there or on the other node, and each volume verifies as in
   step 3. A pod started on a volume under maintenance mounts and verifies.
5. `make drain-test` passes.

Nothing is reformatted. Restore: `metactl undrain fl-b`, the four volumes deleted
(`delete_volume`), `metactl tombstones` empty. fl-b keeps the moved copies until then (#13).

## Docs

- `docs/replication-protocol.md`: §2 gives the table; §7's last line; §9, the new checks.
- `meta.proto`'s comments. `docs/ARCHITECTURE.md` §15.1: a maintenance node hosts a target.
- `docs/lab.md`: drains, `vol detach` unholds, unholding a record that predates this, `make
  drain-unattached`; the rollout order and the rollback order (the CSI controller and
  `lab.sh` before the metadata service: R3-B1). `README.md`, the Makefile's help: the new commands.
- `docs/followups.md` §11: done, and what stays open (Risks). `docs/performance-plan.md`,
  benchmarks: nothing.

## Risks

**Findings against the spec**
1. **S8, S11, "finds the volume gone".** The engine drops a `NotFound` answer
   (`engine.rs:1550`, `:1518`); only the model deposes on it. Designed: it reaches the target,
   whatever its kind. A client's target of a deleted volume then ends within a poll, where
   today it waits for a replica's `Stale`. Open: for maintenance targets only?
   Settled at approval, 2026-11-13: whatever its kind, as designed (see decisions.md, Settled at approval).
2. **S4 against S5.** An agent's `only_if_target` on a maintenance record naming its node is a
   client attach by the Terms, so never refused; by S4 it kept the kind. Designed at approval:
   refused. Changed after approval (R1-C8, R1-S5): S5 wins, and S4 says so. Refused, it locked
   out a pod whose node the drain named after a wrong unhold.
3. **S19, "mixed versions stay safe".** An older instance's `unless_target` changes nothing
   on an unheld record naming the node (`service.go:716`): a client then uses a volume the
   record calls not held, and a maintenance attach deposes it. Designed at approval: no unhold
   before every instance is new, not enforced by code. Changed after approval (R1-S5): the CSI
   controller refuses a publish answered without `held`.
4. **S17 with S14.** A volume waiting for a slot shows no progress for 5 minutes. Designed:
   it is asked again only on another node, since this one took no epoch.
5. **S3, "the reason given"** for a volume being created or deleted is `NotFound`.
6. **S1 on an unheld record:** an agent's restart holds it again. Availability only.
7. **S17, asking again on a stall** (R1-C7). As approved, it restarted a chunk's copy whenever
   the copy took longer than `Stall` (a large `chunk_bytes`, a slow link, #75's throttle), so
   such a chunk never moved. Changed after approval: a node that took an epoch is not asked
   again. Left: a drain started over a target already copying cannot tell it from one that
   never started; after `Stall` without progress it asks another node once, and that chunk's
   copy starts over there. A failed start is seen as the node ending its record with a move
   pending, and asked elsewhere at once (R2-B1); epochs are not counted, since the drain's own
   asks and a start retried after a lost answer raise them too (R3-C3). A node that restarts
   its target again and again without ending the record (a crash loop that keeps its
   heartbeat) is not asked elsewhere while it stays alive: the line reports no progress.

**Against #15's design**
- **#15's attach path, changed for kind maintenance only** (R6): one new method of #15's
  registry, `reserve_maintenance`, inserts the entry and queues its `Input::Attach` in the
  caller's one hold of the registry's lock (R9-B1); the engine's last output for a
  generation is `Detached`, an attach it refuses included (#15 gives a device's attach
  `AttachFailed`); the main thread's wait ends at `Input::Marked`. A device's attach is
  unchanged.
- **#15's layout, changed** (R10-1): `Registry` moves into a module of its own, `mod registry`
  in `ublk.rs` beside `Node`, with a private field, a private maintenance-entry constructor
  and `pub(super)` methods; `control.rs` reaches it through `Node`'s `pub(crate)` wrappers,
  holding an `Arc<Node>` from `control::spawn` (R11-B1, R12-B2). `Node` also gains a condvar
  beside its `Mutex<Registry>` and an `Arc<dyn Clock>`: #15 adds no clock (R12-B1). #15 does
  not place the registry in a module; whichever of #15 and #17 lands second makes this change.
- **The maintenance start reserves on the main thread** (lock order: the registry before the
  engine): the registry's one entry per volume covers S9's "runs no target", a device entry
  still starting included. A start refused there has taken an epoch, so the scan's pass-over
  and the back-off after a failed start (R2-B1) are what keep a node from taking an epoch at
  every scan for a volume it cannot start.
- **Not code yet:** `Input::Attach`'s generation, `pending_delete`, the engine's token key and
  the run mark (`MarkEpoch`, `Input::Marked`, `MarkFs`) are taken from #15's design; its branch
  is 72 commits behind main.

**Safety, security, compatibility**
- **Fencing:** every start is a new epoch, and a maintenance target is deposed as any other.
  Ending at no work is a detach: the next target reconciles.
- **Who must not:** no node names itself; `io` takes an epoch only where an administrator
  named it, and ends only its own maintenance at its own epoch; no agent, and no administrator,
  gets a maintenance token; with #13, no forgotten node. S16 is the drain's rule, not the
  service's: an administrator can name any registered node, as with `Attach`. The choice
  lasts as long as the work: the record's maintenance ends with it (S11). `StartMaintenance`
  does not check S16 again (R1-S4's further option, not taken: the end makes it unneeded).
- **A record whose node died** stays a maintenance record until a client attach, another
  maintenance attach (the drain's, at once, since the node is not alive) or the node's return,
  which starts a target only with work and ends the record at none.
- **Root's end** lasts until the drain asks again, at its next step, which may name the same
  node.
- **`ListMaintenance`** reads every header, per node, every 5 s, and returns only the volumes
  under maintenance there, at most 1024. An index is a follow-up.
- **Still open after this:** a held volume whose target does not run; an unattached volume
  short of replicas with no drain.

**The spec's open questions, as designed, and what the other answer changes**
Settled at approval, 2026-11-13: 1 to 10 as designed below (see decisions.md, Settled at approval).
1. **After #15.** Before it: S9–S14 cannot be built; the service and the drain can.
2. **The record.** VolumeAttachments instead: no `hold`, no `UnholdVolume`, and S3's
   conditional write goes.
3. **The data plane starts it.** A socket command instead: `StartMaintenance` is the agent's,
   the scan moves to the CSI node plugin, the socket gains `maintain VOLUME TOKEN-BASE64`.
4. **Old records are held.** Not held: `Held()` counts `""` as not held.
5. **A forced detach unholds.** Else the unpublish unholds only for a node that is alive.
6. **The node rule kept.** Wider: the drain's candidates only.
7. **Kind from the record.** A claim: a token format change, in every decoder.
8. **No operator.** Nothing here changes.
9. **2 and 5 minutes:** `--max-maintenance`'s default, and `Stall`.
10. **An ADR:** yes, drafted as Proposed.

## ADRs

- **0002** (etcd): consistent. Each new write is one conditional transaction.
- **0005** (leases): consistent. A maintenance target holds leases as any target.
- **0001, 0003, 0004, 0006:** not touched.
- **0014, Proposed, new** (numbered 0007 until the plan review's round 1: 0007–0011 are the
  audit's, 0012 #12's, 0013 #16's; R1-C12): the record says whether a client holds a volume,
  and a target that no client asked for is admitted only while none does. It adds transitions
  to protocol §2, and widens which node becomes a target: a protocol and authorization rule,
  costly to undo. Accepting it is a step of its own: approving the plan does not accept it.
  Settled at approval, 2026-11-13: the rule as ADR 0014 states it; ADR 0014 stays Proposed in docs/adr/ until accepted in its own step (see decisions.md, Settled at approval).

## Parallelizable?

Yes. The Go unit (domain, service, CLI, CSI controller, drain) and the Rust unit (model,
simulator, client, engine, socket) build against Contracts alone. They meet at `make
contracts`: the vectors come from the Rust model, and the interop test needs both. Only the
Rust node part (engine, main thread, socket) waits for #15.
