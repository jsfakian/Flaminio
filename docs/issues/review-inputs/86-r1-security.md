# Issue #86 plan review — round 1, security focus

Scope: `docs/issues/86/` (intent, spec, design, adr-proposal) in
`/tmp/flaminio-plans-2026-11-12/86`, checked against the code of that worktree, plus the shared
bounds and codec contract with #80 and #81.

What holds (not findings): the numbers are right. Body = 13+8N+8T; N=T=65,536 gives 1,048,589
against `MAX_BODY` 1,048,704 (`flaminio-net/src/frame.rs:16`). The audit figures 524,317 and
1,048,717 match `docs/audits/2026-11-12/fenced-repro-results.txt`. The cited code sites say what
the plan says they do (`replica.rs:64-79,129-143,232-238,248`, `frame.rs:285,342,466`,
`target.rs:899-909,1311,1337-1347,2480`, `flaminio-node/src/replica/mod.rs:221-264`).
`Conn::send` is the only network encode site (`flaminio-net/src/session.rs:60`). The coarse
prune rule (clear only if M is absent or M < p; restart cover by epoch only) is equivalent to or
more conservative than today's push-then-retain. Old producers already emit ordered rows and
disjoint extents, so the stricter decoder does not reject them. #80 and #86 agree on 65,536, on
`Frame::encode_checked() -> Result<Vec<u8>, EncodeError>`, on `SessionError::Encode` and on zero
bytes sent on refusal. #81 relies on nothing in #86.

### R1-S1 — Medium

**The quarantine of an unsupported or corrupt legacy volume has no defined behaviour and no test.**

Spec S1 and S4 and design.md:41-48 add a new state: a volume "marked unusable" at startup, with
"a bounded unsupported-volume reason". Nothing says what that state does:

- Whether `ReplicaService::hello` refuses it before or after the existing old-incarnation
  cleanup. Today `store.delete_volume(uuid, Some(first))` runs at
  `flaminio-node/src/replica/mod.rs:201`, before the core is consulted. If the quarantine check
  comes later, a Hello carrying a newer `first_epoch` deletes the quarantined chunks, which the
  plan promises never happens automatically. If it comes earlier, a quarantined earlier
  incarnation blocks a new volume of the same name on that node until an operator acts. The
  plan picks neither.
- Whether the volume stays out of `self.vols` and the core. If it does, `hello()` takes the
  new-volume path with a core fence of 0, and only the store's `ChunkExists` / `StaleEpoch`
  errors stop it. That backstop is accidental, not designed.
- How an operator sees the reason and how the state ends (restart after cleanup?).
- `ReplicaService::new` today narrows with `as Block` and `expect`s
  (`mod.rs:138-144`); the design says "checked" but not what a failed check does to the other
  chunks of that volume.

The test table has no row for it. The S1/S2/S4 row is core unit tests in `repl/src/replica.rs`
only; S4's "quarantine an unsupported legacy volume while serving supported ones; no automatic
erase" has no node-level test.

Fix: state the rule. A Hello for a quarantined volume is refused before `delete_volume`, before
any core or store mutation, and opens no session; the durable fence and bytes are untouched;
other volumes serve; the reason is reported through a named surface; the state ends only by
operator action plus restart. Name a `flaminio-node/src/replica/tests.rs` test that starts on a
store holding one over-budget (or bad-geometry) volume and one good volume, and asserts all of
that, including a Hello with a higher `first_epoch`.

### R1-S2 — Medium

**The plan understates when promotion happens and how long it costs; the ADR's consequence is wrong.**

Sticky coarse mode is an open, Proposed choice; the choice is not the finding. The stated
consequences are:

- adr-proposal.md:43 says full covers raise takeover work "until records are settled".
  Under design.md:36-37 and :57 the mode stays Coarse after a clear, and every later write sets
  `covered` again with M = its own version. The prune a write carries is never above that write
  (`target.rs:1337`, the oldest write in flight), so the cover survives until the next Settle,
  and Settle is sent only at reconciliation and join (`target.rs:2262-2271,2578`). So after one
  promotion, every later takeover digests every chunk written since the previous one, whole, for
  the life of the replica process. One coarse ISR member is enough, since the target unions the
  reports. Client I/O on a chunk waits for `Phase::Ready` (`target.rs:1713`).
- design.md:164 names the trigger as "attack/fragmentation". No attack is needed. The prune bound
  is the volume-wide oldest write in flight but is applied only to the written chunk
  (`replica.rs:231-238`). One write stalled on a slow ISR member holds the bound while writes
  to chunks that member does not hold keep completing and keep their records. B−N such writes
  promote: about a second at 65k IOPS. Separately, every chunk written since the last Settle
  keeps at least one record, so R is at least the number of written chunks.
- Spec S2 and design.md:33-35 cover every held chunk at promotion, including chunks with no
  record and no restart cover. That is not needed for safety: such a chunk reports empty in
  precise mode. It turns one hot chunk into a whole-volume reconciliation at the next takeover.

A tenant workload, or one slow peer, can therefore move a volume permanently to
whole-chunk-per-written-chunk failover. That is safe, and no worse than the existing restart
case, but it is routine here where restart is rare, and the plan presents it as an attack
residue that settles.

Fix: correct adr-proposal.md:43 and design.md:164, and add the two triggers and the lasting
effect to spec S9 and the docs list. At promotion, give `covered` only to chunks that have
records. Either record the remaining cost as the accepted trade, with a test that pins the
report after promotion, Settle and one further write, or bound it: a chunk whose marker has
cleared represents nothing, which is the same state as an empty precise chunk.

### R1-S3 — Medium

**S7 "preserves" a refusal that does not exist in this tree, and names a test for it.**

Spec.md:63-65, design.md:75-78, the S7 test row (design.md:141) and adr-proposal.md:26-27 say an
explicit empty row is "known fresh" only, and that "#13 NotHeld / #61 bootstrap refusal" for a
missing holder is preserved. Neither exists here: there is no `NotHeld` anywhere under
`dataplane/`, and #13 and #61 are unimplemented drafts in other worktrees. What the code does:
`hello()` creates every advertised chunk the node lacks, blank, with no check of why it is
missing (`flaminio-node/src/replica/mod.rs:239-256`). An ISR member that lost its disk
therefore creates blank chunks and, under this plan, sends explicit empty rows, which the target
takes as clean. That is the same trust as today's missing row, so it is not a regression, but:

- the wording tells an implementer and a later reviewer that a protection is in place;
- the test "explicit known-fresh empty rows versus missing-holder bootstrap refusal" cannot be
  written against this tree, and cannot fail without #13/#61;
- intent.md:40-41 says not to pretend other plans are implemented.

Fix: say plainly that an explicit empty row carries exactly the trust a missing row carries
today, and that telling a fresh allocation from a lost one is #13/#61's work. Drop the
bootstrap-refusal half of the S7 test, or mark it as owned by #13/#61 and conditional on them.

### R1-S4 — Low

**The maximum-Version cover is built from a peer field that nothing validates.**

Spec S3 calls the prune/Settle bound "authenticated", and the protocol document assumes every
applied write has an epoch at most the fence. Neither the core nor the node checks that a
Write's `version.epoch`, its `prune.epoch` or a Settle's `below.epoch` equals the session's
epoch (`replica.rs:224-255`; `flaminio-node/src/replica/mod.rs:361-369,919-937` uses only
`version.seq`). Today a Write with a far-future version leaves one unprunable extent of at most
256 blocks. Under the design, in coarse mode it sets M so that no later target's Settle can
exceed it, and the whole chunk stays covered across every later epoch until the replica process
restarts. A target that has since been fenced out keeps that effect. It needs a token-holding
target, and the result is over-reporting, so availability only.

Fix: in `ReplicaCore::handle`, drop a Write or Settle whose `version.epoch`, `prune.epoch` or
`below.epoch` differs from `m.epoch`. Honest targets always send their own epoch
(`target.rs:1341-1348,2480-2490`). Then M is bounded by the session epoch, any newer epoch's
Settle clears it, and S3's "authenticated" becomes true. Add the unit case and include it in the
S3 oracle.

### R1-S5 — Low

**A reply to Hello that is not Fenced still opens the session and extends the lease.**

`target.rs:906-909` sets `session.open` and `peer.confirmed_until` for any reply matched to a
Hello, before the body is examined. A non-Fenced, non-Stale body then reaches the `_` arm
(`target.rs:988-999`), which fails the session but leaves `confirmed_until` raised. Spec S7
wants no session-open or lease fact without a valid Fenced reply. Design.md:91-94 lists only
checks on a Fenced body, and the S7 tests cover only malformed rows.

Fix: add "the body is Fenced" to the validation that precedes `session.open` and
`confirmed_until`, keep the Stale check first so a higher fence still deposes, and add a target
test that an `Ok` answer to Hello grants neither.

### R1-S6 — Low

**Who adds `encode_checked` if #86 lands first is not stated.**

Spec S6 and design.md:100 "extend" #80's `Frame::encode_checked`. #80's design.md:111-113 says
#86 must land before its transport guarantee is complete. Spec S9 and design.md:45-46 say the
runtime defence works without #80. If #86 is implemented first there is nothing to extend, and
`EncodeError`, `SessionError::Encode` and the `Conn::send` change are specified only in #80.

Fix: state that whichever of the two lands first adds `Frame::encode_checked`, `EncodeError`,
`SessionError::Encode` and the `Conn::send` switch with exactly this signature, and the other
extends it; or fix the order.

## Nits

- spec.md:90 names the mutation `bypass_budget`; design.md:144 names it
  `bypass_uncertainty_budget`. Use one name.

Reviewed commit: 412bf8ae49616116170f5b125d6c0d63d5f10488
