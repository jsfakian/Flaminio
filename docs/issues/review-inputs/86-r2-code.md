# Issue #86 — plan review, round 2, code focus

## Earlier findings
- R1-C1: fixed (per-chunk, non-sticky coarsening; bound N+R<=B checked below).
- R1-C2: fixed (quarantine specified, node tests named, constants tied by compile-time assertion).
- R1-C3: fixed (empty row = today's missing row; refusal test dropped).
- R1-C4: fixed (Request::Hello { chunks }, captured in Target::open_session).
- R1-C5: fixed ("whichever lands first"; compatible with #80 design lines 104-113).
- R1-C6: fixed (restart() and add-before-insert specified and tested).
- R1-C7: fixed (catch classes, with_budget, explicit Default).
- R1 Nits: mutation name and tag/body/ADR spacing fixed; other missing spaces remain (R2-C1).

## Bound N+R<=B and the checks
Verified by walking every transition in design "One bounded arena": write on a precise chunk
(room: allocate; full: coarsen only that chunk, which frees its k>=0 records and uses the reserved
slot); write on a coarse chunk (marker only); prune clearing a cover then retaining (re-checks
N+R<B); add/recover at N+R=B (a record chunk exists, else N=B and the add is refused; coarsening
frees >=1 and the new chunk takes one slot, also for a Hello adding many chunks); remove;
restart() (R=0). T<=covered+R<=N+R<=B holds, so body<=1,048,589. The sequential-fill number
(4 chunks whole, 508 chunks x 128 = B-N) is right. The shadow checker plus
drop_on_coarsen / coarse_prune_early / bypass_uncertainty_budget / accept_invalid_fenced still
prove it; the epoch-drop rule honestly has no mutation. Cited lines (replica.rs:223-250,
target.rs:906-909,1351,2262-2271,2578-2588, mod.rs:127-146,194-212,239-257, engine.rs:1926,2364,
sim/tests.rs:238, frame.rs:16-20,196,322,342,378,474) match the code. #80: 65,536, MAX_BODY,
encode_checked signature, SessionError::Encode and the start_target ownership all agree.

No finding of Low or higher.

### R2-C1 — Nit
Missing spaces still change the reading in the new and old text: "Use65,536", "gives1,048,589",
"MAX_BODY1,048,704" (adr-proposal.md, design "Checked codec"), "Safe audit7b94e2d probes" and
"on1/2 legal1GiB chunks" (design Facts, spec Tests), "uses65,537"/"and131,086"/"legal1GiB"
(#80-style probe text), "Fenced v1: N/T≤65,536". "T=checked sum lengths≤65,536" (design
"Checked codec") should read "T = the checked sum of the rows' extent counts ≤ 65,536". Fix: add
the spaces and reword.

Reviewed commit: 344106d41ed47b944ae2d8c0d44c32e463c6aaca
