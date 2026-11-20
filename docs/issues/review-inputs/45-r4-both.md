# Plan review: issue #45, round 4 (incremental), design-code and design-security

Reviewed commit: d490961d493d58f06ca26073d03f642ebe21c674 (HEAD of /home/jsfakian/Documents/src/flaminio-45; the reviewed diff 890f77b..ee3ee5b is the round-3 fix, d490961 adds only report.md).

## Round-3 findings
- R3-C1 (Medium): fixed in substance (state + alive first, hooks commit FAIL then expire then FORGET). Two gaps remain: R4-4.
- R3-B1 (Low): fixed. Stated in design.md Risks (:399-402) and the runbook (:375-380), as "not accepted", consistently. spec.md/intent.md only say "reported" (intent.md:126), not "no end"; acceptable.
- R3-S1 (Low): fixed by removing the certificate and wall-clock check. No owner-side stop is left, so no give-up rule is needed. `--insecure` answered.
- R3-C2 (Low): fixed in a new form (late = >2 s since the previous tick, zero before any write; a write committing after its context keeps credit). The reply-loss rule is under-specified: R4-6.
- R3-C3 (Low): fixed. S7, the Tests row, the drain test row (design.md:322) and decisions record "also Active" for the drain's new asks as a #17 change. Standing records stand by S16 alone, as #17 says. The drain.go change is not in design.md's file list (Nit 1).
- R3-C4 (Low): fixed. At most one revision per 5 s, only during an outage; compaction in the runbook/lab.md, lab etcd.sh a follow-up (S20).
- R3-N1, R3-N2: fixed.

## New findings

### R4-1 — Medium
**The new credit rule changes the user-settled S5 definition and leaves S5 and decisions.md saying otherwise.**
- decisions.md:54-57 (settled 2026-11-13, the user's) defines the credit as "consecutive successful probes by the same live observer, each interval capped at 1 s", and decisions.md:60-61 and the VM-clock paragraph still say "the credit clock adds at most 1 s per probe". The new rule (design.md:105-119) credits up to 5 s after the last committed heartbeat although the service may be down for all of it, and a "successful observation" is now "some node's heartbeat was seen committed", not "the service answered". decisions.md:308-311 says the settled definition "keeps its numbers", and the round-3 header says "no spec rule is narrowed". The unobserved bound is now 5 s an outage (and 5 s again for each separate outage), not 1 s an interval.
- Spec S5 ("could have received the node's heartbeat") is not changed and is loosely met only within that 5 s.
**Fix:** record the change as a change to a user-settled item, flagged for the user to confirm; correct decisions.md:60-61 and the VM paragraph ("at most 5 s an outage"); state the per-outage bound in S5's gloss.

### R4-2 — Low
**A stale heartbeat can credit after a new owner starts or after a late tick.** The owner keeps "when the newest ModRevision last rose" (design.md:105-107), but its initial value, and its value after a >2 s late tick or an ownership change, is not defined. If it starts as "now" at the first read, the second tick credits up to 5 s with no heartbeat seen (a key may be up to 10 s old, the lease TTL). No test row covers it (design.md:317).
**Fix:** start unset; credit only after the owner itself sees the newest revision rise; reset on a late tick; add the test (an old `alive/` key, no rise: zero).

### R4-3 — Low
**Any heartbeat credits, not only one that could matter, and this is not stated.** The newest revision of any `alive/` key counts: a sticky-Failed node that heartbeats (S6), or an administrator's `NodeHeartbeat` (allowed, design.md:42-43), keeps crediting a Missing node that is truly unreachable. The invariant bounds the harm (copies only), and a node's own heartbeat makes it Active, so it is not a wrong credit for itself. But "no other node heartbeating credits nothing" is stated as safe (design.md:118-120) while the converse is not.
**Fix:** state it, or count only keys of Active nodes (the pass already lists `node-state/`); add a test row (a Failed node's heartbeat alone credits).

### R4-4 — Low
**#13 test changes: an ordering assumption and a lost refusal test.**
- `TestForgetRacesAPlacement`: the hook sits on `nodes()`' last read, the `List` of `drain/` (`service.go:239-256`). With #45, `nodes()` or the creation also reads `node-state/`. The first attempt must read the node Active before the hook, or it never places on it and the "first claim fails, exactly two claims" assertion breaks. Say where `node-state/` is read relative to the hook.
- `TestForgetNodeRefusals` now expects `node failed: ` for a move and a placement, so #13's `node T is forgotten` refusal and the mark compare on those two paths have no test (a forgotten node is always Failed, so the mark is behind the Failed check). The two race tests prove the compares only.
**Fix:** add one service test that writes the mark with the state not Failed (kv direct) and expects `node T is forgotten` for the move and the placement; state the `node-state/` read order for the placement hook.

### R4-5 — Low
**A linearizable `List` of `alive/` every second reads the values.** `kv.List` returns key and value (`meta/kv/kv.go:35`), and the value of `alive/<id>` is #72's capacity report JSON. 1 read a second by the owner, through etcd's quorum, grows with nodes and report size. Only `Rev` is needed.
**Fix:** say that the tick reads revisions only (a keys-only or count-limited read) or bound the size, in design.md; the shared state between the 1 s tick and the 5 s pass needs one owner goroutine or a mutex (name it).

### R4-6 — Low
**The credit write's reply-loss rule is not stated.** "anchors on what it finds and adds nothing twice" (design.md:~131-134): the owner holds accrued credit in memory and writes persisted + accrued. After a lost reply, the reread finds either the old or the new value; keeping accrued after the first, or dropping it after the second, is the whole rule. Without it an implementer can double count or lose credit.
**Fix:** write it: the accrued amount at the write's snapshot is dropped only if the reread value equals the intended one; else it is kept and rewritten against the reread sequence. Test both.

## Checks asked for
- Credit rule vs S5: consistent only up to R4-1. A heartbeat from the failed (Missing) node itself cannot credit it: its `alive/` key is absent by definition, and its first heartbeat makes it Active. A stale heartbeat crediting: R4-2. A Failed node's or an admin's heartbeat crediting: R4-3.
- "A cluster where no other node heartbeats credits nothing": stated (design.md:118-120, decisions.md:309-311), and fails closed (no timeout failure); safe. A cluster whose only live nodes are Failed ones is covered by R4-3.
- Clock written once per 5 s pass only while a node is Missing, late write keeps credit: sound (credit lost in a crash only delays failure) except R4-6. The Missing origin is read after the pass's write, so the earlier gap before recording is not credited (conservative).
- #17 change (S7, "also Active" for the drain): consistent with #17's S16/S17 (new asks only, standing records stand), needs `control/drain/drain.go` in the files list.

## Nits
1. design.md has no implementation-file line for the `control/drain/drain.go` change (only the test row, :322, and :19); name it, with the #17 dependency.
