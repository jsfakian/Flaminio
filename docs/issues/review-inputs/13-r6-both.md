# #13 plan review, round 6 (incremental), design-code and design-security

Scope: git diff 1de5da6..9f3aa0a in docs/issues/13/ (design.md, decisions.md). Read against
meta/service/service.go (createVolume 372-473, DrainNode 827-842, MoveReplica 866-918,
readHeader 275), interleaveKV (service_test.go:615-670), kv/mem.go (Put bumps Rev), and #78's
design.md (gate, mark, move).

## Round-5 findings
- R5-C1: fixed. The hook on the move's header Get (readHeader) is after the mark/gate and
  destination reads and before the chunk Get (service.go:876-884). Without #78 only the mark
  compare stops the move; with #78 only the gate compare does (ForgetNode bumps the gate with a
  new marker; the fixture has no marker, so the marker is new). ForgetNode writes no header
  (not a fence holder), so the header compare passes. Fixture stated.
- R5-C2: fixed. Each mark transaction is conditional: mark rev, drain/<node> rev, and with #78
  the gate read (new-marker case). Counts: new marker with #78 = 3 compares/3 writes; existing
  marker = 2/1; both within 120. Undrain or drain/redrain between read and Txn changes the
  marker revision, so the compare fails and it retries. Keeping the marker is safe: it already
  excludes the node, and the gate bump from when it was written stops earlier readers.
  TestForgetKeepsARunningDrain fails if the marker is rewritten (kv/mem.go:94-95 gives every
  put a new Rev, as etcd does).
- R5-N1: fixed (stacked wrappers; the outer hook is spent before ForgetNode's own step-2 Get).
- R5-N2: fixed in design.md and decisions.md (see R6-L1 for what the new loop leaves open).

## New findings

### R6-L1 - Low: the new createVolume retry loop has no test of its own, and TestForgetRacesAPlacement now describes the wrong outcome
- design.md:197-205 makes createVolume retry (up to 32 attempts, Aborted after the last). The
  loop and its outcome are new behaviour: today one failed claim returns.
- The test (design.md:1027-1031) still says "The claim fails, and no chunk of the volume names
  the node". With the loop, the first claim fails and the second attempt re-reads the mark,
  skips the node and succeeds, so CreateVolume returns OK. The text does not say which: the
  call succeeds, or fails Aborted. An implementer could assert either.
- Nothing tests the exhaustion path (mark moving on every attempt gives Aborted, not a hang or
  ErrExists), nor that a taken id or a live tombstone still returns ErrExists at once without
  retrying.
- Fix: state that CreateVolume succeeds on the second attempt, no chunk naming the node, and
  the claim made exactly twice. Add (inside that test, or a small second one) the exhaustion
  case: a hook that forgets a fresh node on each attempt, or at least the Aborted outcome, and
  an id-taken case returning ErrExists with one claim.

### R6-L2 - Low: "a claim that fails with both keys still absent" does not say how it is known
- design.md:201-204. The current code on a failed claim does one readHeader (service.go:424),
  not a read of both keys, and it branches on the interrupted-deletion case. A loop that
  retries must read the volume key and the tombstone key after the failure (a fresh Get each)
  before deciding to retry; the order against the existing deleting branch is not given.
- Without it, an implementer may retry on any failed claim (a taken id then waits 32 backoffs
  before ErrExists) or never retry.
- Fix: say that after a failed claim the volume key and the tombstone key are read again; the
  deleting-header branch is checked first, as today; both absent means retry, else ErrExists.

### Nit
- R6-N1. Step 2 (design.md:164-168) says the Txn compares drain/<node>'s revision but never says
  the step reads drain/<node> (and, with #78, the gate) first; say "reads the mark and
  drain/<node>".

Reviewed commit: 9f3aa0a
