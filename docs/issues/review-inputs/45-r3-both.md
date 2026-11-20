# Plan review: issue #45, round 3 (incremental), design-code and design-security

Scope: `git diff ba9d1d5..1ad8f45 -- docs/issues/45/ ':!docs/issues/45/report.md'`, checked against
the code in /home/jsfakian/Documents/src/flaminio-45 and the plans of #13, #17 and #82.

## Earlier findings

- R2-B1 (Medium): fixed. FORGET is the only path, and the check sits in `ForgetNode`'s mark
  transaction. That transaction now has 4 or 5 compares and 1 to 3 writes, well under 120. The
  test changes listed for #13 are wrong: R3-C1.
- R2-S1 (Medium): fixed. The `alive/<id>` absence compare is a necessary condition, and the
  runbook asks for outside evidence, as #82's S1 does (wording: Nit 1).
- R2-C1 (Medium): fixed for the rebuild's own asks. S16 ∩ Active is a subset of #17's S16, and a
  record stands while S16 allows its node, as #17's S17 says. The same hazard remains through a
  drain's ask: R3-C3.
- R2-B2 (Medium): fixed. NOSPACE and an expired leaf now add zero. Three gaps are new: R3-S1,
  R3-C2 and R3-C4.
- R2-S2 (Low): fixed as the orchestrator decided. One consequence is not stated: R3-B1.
- R2-C2 (Low): fixed. `node.sh list VOLUME` matches #13's lab (#13 design.md:1185), and step 1
  now checks for Active and alive nodes.
- R2-N1: fixed.
- R2-N2: fixed. The back-off triggers agree with S11 and S15.
- R2-N3: fixed.
- R2-N4: fixed. A non-owner asks nothing until it holds the ownership again, then rebuilds its
  sightings from the records. #17's S17 allows this (progress only).

### R3-C1 — Medium
**The change to #13's tests says "first make the node Failed and not alive". Done that way, it
breaks two of #13's race tests and changes a third.** decisions.md:208-209 states the change.
Under #45, Create and Move exclude Failed nodes, and Move refuses a Failed destination before
anything else (design.md:127, :133-135). `Place` ignores a `PreferNode` that is not a candidate
(`meta/domain/domain.go:367-368`). Creation candidates must also be alive (`service.go:399`).
- `TestForgetRacesAPlacement` (#13 design.md:1038-1044): the creation never places on a node
  that is already Failed. So the "first claim fails, exactly two claims" assertion fails.
- `TestForgetRacesAMoveBack` (#13 design.md:1020-1026): the move is refused with `node failed: `
  before the hooked header read. The test then passes even with the mark or gate compare
  removed, so it no longer proves #13's S19.
- `TestForgetNodeRefusals` (#13 design.md:1004-1008): a move to the node now gets `node failed:
  `, not `node T is forgotten`. The placement refusal now comes from the Failed check, not from
  the mark.

**Fix:** state the test changes exactly:
- In the two race tests, the hook commits FAIL, lets `alive/` expire (Mem's clock), then runs
  `ForgetNode`. The move or claim then read the node Active and alive before both, and with #78
  "fails with the gate compare removed" still holds.
- `TestForgetNodeRefusals` expects `node failed: ` for the move and placement, and keeps the
  mark's own refusals (`RegisterNode`, undrain, the `io` calls).

### R3-B1 — Low
**An unheld volume with a pending move to a destination gone for good never gets a maintenance
attach. The plan only says its chunks "wait".**
- A move is never withdrawn (intent.md:54).
- #13's `ForgetNode` clears releases, fence holds and tombstones, but not `moving` (#13
  design.md:170-183).
- ADMIT of a dead node gives Missing, which is still "destination lost" (spec.md:127).

So under the narrowed S12, the volume's other chunks keep reduced redundancy for good, and they
had carryable moves (decisions.md:231-233; design.md:198-203). The destination stays named in a
move, so it is never "re-protected". The runbook requires that before forgetting it
(design.md:352).

**Fix:** say in Risks and the runbook that this wait has no end under #45 for a destination gone
for good. Name the operator's remedy: attach the volume, or `flaminio-meta maintain` with root
ending the target once its other moves are done (#17 S14). Or record a follow-up for withdrawing
a move.

### R3-S1 — Low
**The certificate rule stops credit without moving ownership, and it reads the wall clock.**
- An owner whose leaf has expired writes no checkpoint (design.md:105-114). Nothing makes it give
  up `rebuild-observer`, a 5 s TTL key renewed separately (design.md:97-99). Nodes keep
  heartbeating through instances whose certificates are valid, since the client tries endpoints
  in turn (`flaminio-metaclient/src/lib.rs:6`). Yet no instance credits, and timeout failure
  stops cluster-wide. This is realistic with a leaf loaded once at start
  (`meta/cmd/flaminio-meta/main.go:116`, `pkg/pki/pki.go:285`).
- `NotBefore`/`NotAfter` are compared with wall time. An instance clock behind real time keeps
  crediting after nodes refuse the certificate. That contradicts "No wall jump adds credit"
  (design.md:117-118).

**Fix:**
- An owner outside its leaf's validity releases the ownership: it deletes its key at its
  revision and stops renewing. Test it with two instances.
- State that the validity check trusts the instance's wall clock, an exception to "No wall jump
  adds credit". Or also stop at a monotonic deadline taken at start.

### R3-C2 — Low
**"Credited only if it commits within the bound" and "a late checkpoint adds zero" cannot hold as
written.** The credit travels in the checkpoint's own put (design.md:105-112). A checkpoint that
commits after its 1 s context expired still adds its credit, which the reread then finds
(design.md:115-116). Where the claim is made:
- decisions.md:223-224: "only if it commits within the bound";
- the test row, design.md:299: "a late checkpoint … add zero";
- adr-proposal.md:31: "late probes … earn zero credit".

The over-credit is at most 1 s, within the definition's unobserved bound. But an implementer
cannot write the test as stated.

**Fix:** define "late" as more than 2 s since the previous committed checkpoint, credited zero
in the put. State that a checkpoint committing after its context keeps its credit of at most
1 s, and that the next interval is anchored at the reread that finds it. Test both.

### R3-C3 — Low
**R2-C1's protection can be bypassed through a drain's ask.** #17's drain still chooses by S16
alone, ranked by most chunks, then name (#17 design.md:443-444). A drain of another node can
therefore attach maintenance on a Failed node that is alive and holds the most chunks. The
rebuild then keeps that record as standing (design.md:209-213), and the copy reads the disk that
was declared failed (design.md:216-218). #45 already changes the drain's destination choice to
exclude Failed (S7, design.md:127).

**Fix:** apply "also Active" to every new ask under #17's S16, the drain's included, with a
drain test. Or name the drain case in Risks.

### R3-C4 — Low
**The probe adds a retained etcd revision every second, and nothing compacts them.** One write a
second by the single owner is cheap next to heartbeats: 3 nodes every 3 s make 1 write a second
(`flaminio-io/src/node.rs:331`). But the lab's etcd runs with no `--auto-compaction-*` flag
(`scripts/guest/etcd.sh:45-52`), and nothing in `meta/` compacts. So the checkpoints double the
lab's history growth (about 86,400 revisions a day) toward the quota, and the quota is the
NOSPACE alarm that the probe now counts as an outage.

**Fix:** state the requirement in the runbook and `docs/lab.md`. Add periodic auto-compaction to
`etcd.sh`, or record a follow-up.

## Nits
1. spec.md:156, design.md:350-351 and decisions.md:215 say "as #82 requires before a node-wide
   action". #82's node-wide recovery is its controller's out-of-service taint (#82 spec Terms,
   S2), not an administrator's forget. Say "the standard #82 sets for node-wide recovery", so
   no one wires #82's evidence provider into FORGET.
2. design.md:105-108: the probe requires a valid serving leaf, but `--insecure` loads none
   (`main.go:111-129`). Say whether such an instance credits, or never does.

Reviewed commit: 890f77b64761b9a6e9d25fbffe14e1c1f72eee64
