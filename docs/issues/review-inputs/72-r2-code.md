# Plan review #72, round 2, design-code

## Earlier findings
Round 1 (code) findings on the cut text (class lock, grants, NoSpace, routes, Hello v2, ALPN, balancing, locality, device classes, new Volume/ChunkMeta fields): fixed by the cut. A grep of intent, spec, design, contracts, ADR and decisions finds no surviving text except the "cut" notes in decisions.md and the rejected option in adr-proposal.md. Tag 72 on `NodeHeartbeatRequest` and `Node` is free in api/proto (Node ends at 5, request at 1) and no other draft uses 72. Transaction table: all rows fit 120 compares and 120 writes (create claim 118/117 at k=116; ChangeIsr 1+n+(119-n)=120 compares, 119 writes). `InitCapacity` needs an `auth.go` rule (default is deny) and the plan has one. `CheckIsrChange`/`ApplyIsrChange` and the vectors are untouched. The worked example (room 2, 2242) is right.

## Findings

### R2-C1 — Medium: the completion-release condition is ambiguous and, read literally, releases on a plain ISR change
design.md "ChangeIsr": "a named chunk whose `Moving.From` is not in its `NewReplicas`". On a change with `ChangeReplicas` false, `NewReplicas` is empty, so every chunk with a pending move would qualify and its source would be released. The real completion test is `ChangeReplicas && Moving != nil && From not in NewReplicas` (`meta/domain/domain.go:277`). The spec text (S3) says "drops the source from the replicas", which is right; the design restates it wrongly. No test covers a plain ISR change on a chunk with a pending move.
Fix: define the condition as ApplyIsrChange's, ideally by a `domain` helper both use. Add to `TestMoveCompletionReleasesSource` a plain ISR change (ChangeReplicas false) on a chunk with `Moving` set: releases nothing.

### R2-C2 — Medium: absence is not compared where a ledger is read absent, so a concurrent `InitCapacity` leaves a permanent over-charge; the exactness claims do not hold
capacity-contracts.md table: `ConfirmDeletion` compares "the node's ledger if it exists"; design "ChangeIsr": "absent or unreadable, skipped". With no compare, this interleaving commits: ConfirmDeletion (or the completing ChangeIsr) reads the ledger absent; `InitCapacity` lists the tombstone as unconfirmed (or the chunk with its source) and creates the ledger with that charge; the confirmation commits with no release. The ledger holds a charge nothing will ever release, and the plan offers no reset (design Risks, Compatibility). The same permanent over-count follows from the listing order `volumes/`, `chunks/`, `tombstones/` when a deletion lands between them: counted twice, released once ("counted once or twice"). The spec/test wording ("exact rows", "never under-counts") hides that over-counts are permanent. ConfirmDeletion with an unreadable (present, unparseable) ledger is not specified at all (ChangeIsr says skip).
Fix: when the ledger read absent, compare its revision 0 in the same transaction (one compare, within budget); a failed compare reruns. State the unreadable case for ConfirmDeletion (skip, log). Say plainly that the double count is possible and permanent until a reset exists, or add a reset (an `InitCapacity` option to recompute a named node), and add an interleaved test: Init creates the ledger between the read and the commit of a confirmation and of a completion.

### R2-C3 — Medium: existing fixtures and lab checks break and are not in "Files touched"
Every node that does not report and have a ledger is now excluded (S1, S2), so: `addNodes` (`meta/service/service_test.go:58`) heartbeats with no report and registers into a ledger only if RegisterNode creates it, so every existing create/move/drain/CSI test needs reports; `domain_test.go` builds `NodeLoad{ID:..}` with `Room` 0 (lines 146–302), which `placeChunk` would read as "no room"; the design does not say how a zero `Room` is treated or that these tests are updated. In the lab, `scripts/checks/reconcile.sh`, `resync.sh`, `scripts/guest/meta.sh`, `confirm.sh`, `scripts/tools/lab-deletion-test.sh` and other targets that call `metactl create` against already registered nodes fail until `capacity-init`; only `meta-smoke` is changed. Creates in those checks may also be refused at 80 % of a small lab disk.
Fix: list the test-helper changes (`addNodes` heartbeats a report; `Room` semantics in `Place`, e.g. a constructor or test default) and run `capacity-init` (or register fresh) in each lab check that creates, or state that they are run after `make meta-smoke`; add the check that they pass.

### R2-C4 — Low: every create attempt rescans all chunk records
design.md "createVolume" step (2) repeats `nodes()`, the full chunk listing, reports and ledgers on each of up to 32 attempts. A ledger conflict is a new retry source (creates used to have no compare beyond the id), and each retry costs a full scan of every chunk record. A busy node whose ledger changes with each confirmation or completion can make creates cost 32 scans, then `ABORTED`.
Fix: scan chunks once outside the loop (the counts only order placement; the ledger decides admission) and reread only reports and ledgers per attempt; or say why not.

### R2-C5 — Low: rules without a named test
- S5 contention: no test of 32 failed attempts giving `ABORTED`, or of a create succeeding after one ledger conflict (the claim's "both absent means ledger conflict" branch).
- `ChangeIsr` "accepts and refuses exactly as before" (spec Tests S3–S5) names no test; S9's test only covers `CheckIsrChange` in the domain, not that the service writes the same chunks; a ChangeIsr ledger-compare failure rerunning is untested.
- Unreadable ledger (corrupt value) in create, move, completion and confirmation: only "underflow" is tested.
- S8: drain on `FAILED_PRECONDITION` (no report/ledger) is only implied.
Fix: name them (`TestCreateRetriesOnLedgerConflict`, `TestCreateContention`, `TestChangeIsrWritesAsBefore`, `TestUnreadableLedger`).

### R2-C6 — Low: lab step 4 depends on the disk size
Step 4 moves a 256 MiB chunk to fl-a "between soft and hard"; the hard-minus-soft band is 15 % of T. Below about 1.7 GiB of data capacity the move is refused, and the plan states no minimum disk. Step 3 also needs R × 256 MiB to fit slots budgets. Fix: state the minimum T (or choose the chunk size from T) and fail the check with the figures if the band is smaller than one chunk.

## Nits
1. capacity-contracts.md, Transactions note and adr-proposal.md name 116 as the limit; the claim fits 118 nodes by compares (120 − 2). Say 116 is chosen for headroom, not derived.
2. design.md "Charging and release" says `ChangeIsr` bound "first `119 − len(changes)` get their ledger read" by node; the spec says "by node id (55 for 64 changes)"; the test says "56 sources" — say "56 distinct source nodes".

Reviewed commit: 8ad446a (HEAD; the diff reviewed is 813d9a5..629bc32)
