# Issue #70 plan review — round 4 (incremental), security focus (design-security)

Scope: `git diff 82b7bec..91caea0 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70`. HEAD 294ded6 changes only `report.md`. Line numbers are at HEAD.

## Earlier findings

- R3-S1: fixed. The accepted text is restored word for word (design Risks and ADR Consequences
  match a3dbb5c). The wait is listed apart, as not accepted, in the design's header and Risks, the
  spec's header and Settled questions, the ADR's marked note and date line, and decisions.md's
  "Changed after approval". One leftover: the spec header lists too little (R4-S3).
- R3-S2: fixed as asked. `blkid -p` is named (`node.go:108`, `mount.go:37–40`); NodeUnstage
  waits behind `n.mu` (`node.go:135`); "mkfs" is corrected; a release path is stated;
  `TestNodeStage` is added. The new text is wrong in places: the release path (R4-S1), the
  narrowing (R4-S2) and the scope (R4-S4).
- R3-S3, R3-2: fixed. A `( )` subshell runs only the EXIT trap set inside it (`lab.sh:1158`).
  `die` exits 1 (`scripts/lib.sh:21–24`), so the subshell fails and the parent's `set -e` and
  trap take over. `KEEP=1` skips `:1200–1203`. The exec check runs before the cleanup, and the
  block pod is in `flaminio-test`.
- R3-S4: fixed in S6. The old wording is still in decisions.md and the ADR (R4-S5).
- R3-1: fixed. The check runs on the raw reply before `chunk_meta` (`engine.rs:1555`, `:386–395`,
  `:1567–1574`), and the test asserts the node table and no ListNodes call.
- Round-3 code nits: fixed (S9 says refusals name the flag; the sim sentence is corrected).

## Findings

### R4-S1 — Medium

**In two cases the documented release paths do not free the node. The release that does work is
untested.** The release text is at design.md:248–251 and decisions.md:20–29. The README note and
`docs/followups.md` will carry it (design.md:223–225).

- **Restart the data plane:** this fails the waiting I/O, so NodeStage returns. But kubelet
  retries NodeStage while the pod is still scheduled on the node. Each retry attaches again under
  a new epoch (`node.go:58–77`). The replicas still refuse, so the next `blkid -p` waits under
  `n.mu` again. The node is free only if the pod is removed before the restart: it stays
  Terminating, and kubelet then sends NodeUnstage instead of another NodeStage.
- **Free slots:** this cannot work when the growth placed more of the volume's chunks on a node
  than its whole slot table holds.
  - #80's preflight refuses the whole Hello unless the node has a slot for every missing chunk
    placed there (#80 design.md:93–97).
  - The slot table is fixed at format time: 2 × data bytes / 64 MiB, at least 64
    (`flaminio-format/src/layout.rs:111–114`, `consts.rs:47–53`).
  - #80 admits up to 3,881 chunks at RF 2 (#80 design.md:29–32). On a two-node site every node
    holds a replica of each chunk, so a growth to that size needs close to 3,881 slots per node.
    That is more than the whole table of a node with under about 121 GiB of data.
  - Deleting volumes then frees too little. Chunks placed only on such nodes, prefix included,
    never serve again: nothing shrinks a volume, and committed steps stay (design.md:62).
  - This is the design's own abuse case: a tenant "asking #80's largest size" (design.md:246–247).
- **No test of the release:** `a_suffix_chunk_without_a_slot_waits` (design.md:188) stops at the
  wait. No test shows that freeing a slot lets the target reconnect (`target.rs:829–836`) and the
  waiting I/O complete.

Fix:
- Write the restart release as: scale the workload to 0 or delete the pod, then restart the data
  plane on the target's node.
- State that freeing slots helps only when each refusing node's slot table can hold the volume's
  chunks placed there, and that otherwise the volume cannot be served again. Add this to the open
  question in decisions.md.
- Extend `a_suffix_chunk_without_a_slot_waits`: free a slot on the full node, advance time past
  `reconnect`, and check that the waiting I/O completes.

### R4-S2 — Medium

**The narrowing says to wait for #72, but neither plan makes expansion charge #72's ledger.**

- design.md:251–252 and decisions.md:24–25 say: where tenants share data nodes, "turn
  `--volume-expansion` on only once #72 lands".
- This plan builds no charge: "once they land, a step charges … #72's per-node capacity
  ledger …; this step builds neither" (design.md:71–73). intent.md:34–37 keeps
  "expansion-aware capacity reservation (#72)" as a follow-up.
- #72's step covers only "admission of creates and moves" (#72 spec.md:4) and has no growth rule.
  Its intent says "#70 must charge its new chunks through this ledger" (#72 intent.md:48).
- So after #72 lands, ExpandVolume still places chunks without the ledger. An operator who
  follows the README note turns the flag on, and the cross-tenant wait is still there.

Fix: tie the narrowing to the follow-up that makes ExpandVolume charge #72's ledger, not to #72
landing. Name that follow-up in `docs/followups.md`. Use the same wording in the design, in
decisions.md and in the README note.

### R4-S3 — Low

**The spec's header understates what changed after approval.** spec.md:5–6 says only "S6's last
two sentences were added after approval". `git diff a3dbb5c..91caea0 -- docs/issues/70/spec.md`
shows more:
- S6's third sentence was rewritten in round 2 ("refuses the volume's session before its fence
  moves, like an unreachable replica").
- S9 changed in round 3 ("every refusal then names the flag"; "turns the flag on in a second
  rollout").
- The S5/S6/S8 and S7/S8 test lines gained cases.

Fix: say that S6, S9 and the Tests changed after approval, and that only S6's last two sentences
describe the new, unaccepted risk.

### R4-S4 — Low

**The stated scope misses NodeStage's mount, and it assumes the last chunk is the one waiting.**

- The design says: "For ext4 the first to wait is NodeStage's `blkid -p` …, which reads the
  device's last sectors, in the last suffix chunk; then NodeExpand's `resize2fs`"
  (design.md:240–242).
- But NodeStage also runs `mount` under `n.mu` (`node.go:89`, `:124`; `mount.go:69–77`, a child
  process). Mounting ext4 reads the group descriptors and the journal, and the journal can be
  replayed.
- The waiting chunk can be a middle prefix chunk, as in the design's own RF 1 test (chunk 1,
  design.md:188). Then `blkid -p` passes and `mount` waits under `n.mu`.

Fix: state the scope as "every NodeStage step that reads the device under `n.mu` (`blkid -p`,
`mount`), then NodeExpand's `resize2fs`". Drop "in the last suffix chunk".

## Nits

- **R4-S5 — Nit.** decisions.md:21 says "When no replica of a chunk … has a slot", and
  adr-proposal.md:55–56 says "a chunk none of whose replicas has a slot". A replica with a slot for
  that chunk still refuses if it lacks slots for the volume's other missing chunks. Use S6's
  wording: "none of whose replicas accepts the session".
- **R4-S6 — Nit.** design.md:76–78 says a binary-only rollback that keeps the flag "fails
  closed". In fact the older instance exits with status 2 (`flag.ExitOnError`, `main.go:84`) and
  restarts in a loop (`meta.sh` `Restart=on-failure`), so it serves nothing. Say so, and make the
  rollback rule "remove `--volume-expansion`, then roll back the binary".

Reviewed commit: 294ded688f7aef1e4de38e3f1d7e33d45fbb340d
