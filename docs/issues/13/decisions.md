# Issue #13: Replicas left behind by moves — decisions

Settled in plan review rounds 1–4. Reviewers treat these as settled.

## Choices

- **Releases live in the chunk record** (`released`), written by the drop's own put: atomic, no
  extra etcd operation, no new key.
- **A move to a released node is refused as other moves are** (`Applied: false`), not with an
  error: the drain already handles it.
- **The tombstone names released nodes and fence holders:** the id stays taken until they
  delete; a creation has nothing to clear.
- **A confirmation is conditional on the revisions listed,** all or nothing: a late one never
  removes a newer release.
- **Only the node's own agent confirms releases;** administrators only see them: a second
  confirmer is what lets a deletion hit a live or new chunk.
- **The agent is the only sweeper,** of tombstones too (issue #14's step 1, taken in): one loop
  orders tombstone confirmations against release deletions.
- **A new certificate component, `sweep`,** the agent's alone; the CSI node plugin keeps `agent`
  and loses the sweep's calls: an old CSI sweep is refused, and the CSI pod cannot confirm.
- **One agent process per node** (`flock`, in a directory the CSI pod does not mount), and one
  `delete-chunks` per volume in the engine: a second process, or a request left by a dead agent,
  would act after the next confirmation.
- **No release token:** the data plane re-reads the chunk record at deletion. Safe because a
  released chunk cannot move back, and only this node's single agent confirms, after the delete.
- **A deletion re-checks per chunk** (deadline, local view, fence not risen past the epoch read,
  no tombstone deletion since) and deletes at the durable fence, not `u64::MAX`: a request that
  outlives its answer cannot touch a new volume's chunk.
- **The grace is 10 s per (volume, chunk),** reset when unlisted or confirmed: above the 7 s a
  drop's proposer needs. The "two sweeps" rule is cut: 10 s at 5 s implies it.
- **No session rule in the data plane:** a target keeps its session to a node for the node's
  other chunks, so the rule would block every deletion; the view rule plus S9 suffice.
- **A `Hello` at the fence creates no chunk;** only one that raises the fence, or a resync, does.
- **A `Hello`'s creations and fence commit in one batch;** one too large for a batch is
  refused: no multi-batch path to get wrong.
- **A failed `Hello` restores the volume's whole core state** (fence, session, chunks, filters);
  missing chunks come from the store.
- **The node records a volume's shape from a `Hello`** even with no chunk of it: a resync after a
  restart can create the chunk.
- **A fence-only record keeps the fence after the last chunk** (a slot state, a journal record,
  incompatible bit 2): within one volume, an older target must stay refused after a restart.
- **Fence holders are listed in the volume's header**, added at the first confirmation, so the
  tombstone deletes their records.
- **"A volume created again starts above every earlier epoch" is issue #30's,** not #13's, and
  so is the vectors' first epoch. #13 gives #30 `Store::delete_volume`, which frees fence-only
  slots too.
- **No path for copies no record names:** they are deleted by hand once in the lab; this change
  makes none, but in its rollout window.
- **`ListReleases` scans every chunk record, at most 1024 per call, no cursor:** small for the
  lab; the rest come in later sweeps.
- **`delete-chunks` takes at most 119 chunks:** one confirmation's worth, under the socket's line.
- **The drain still ends when nothing is placed on the node;** it waits instead of failing when
  only released nodes remain, and shows the releases held.
- **The simulator only confirms the releases it makes;** no new check: its `Hello` creates no
  chunk, so the window is the data plane's, covered by node tests.
- **Issue #14 lands first;** #13 removes `vol-confirm`, `flaminio-meta confirm` and the stop for
  fl-c, and moves #14's `tombstones` calls to `sweep`.
- **The hand cleanup** deletes a volume's copies on a node only when no record names the node for
  it: otherwise it could delete a live or released copy.
- **Metadata and data-plane rollback is unsupported** once a release, a fence holder or a
  fence-only slot exists. Simpler than a two-step rollout.

## Accepted risks

- **Copies no record names** (from before this change; releases erased by an old metadata
  instance in the rollout) hold space until deleted by hand; a later volume of the name drops
  them (#30). Follow-up: their deletion.
- **A node gone for good:** its releases stay, and a deletion of their volume leaves a tombstone
  nothing confirms; the id stays taken. Follow-up: forgetting a node.
- **The service still accepts an administrator's tombstone confirmation;** no tool sends one
  (issue #14).
- **No node sweeps** from the metadata service's rollout until the agents start: no volume
  deletion in the lab meanwhile.
- **Fence-only slots can leak** when an old instance rewrites a header, or a header does not
  parse, dropping `fenced`: harmless with #30 (stale), and bounded by one per volume and node.
- **The copy listing exposes name hashes** to root on the node and to the CSI node pod, which
  shares the socket's directory.
- **Root on any node can sign capability tokens** (every node holds the token key), and can
  `delete VOLUME` without a token: as before. Follow-up: a token for `delete VOLUME`.
- **A root caller of `delete-chunks` can skip the grace:** it can make the drop's proposer fail
  its session early, not lose data. A request outliving its agent cannot act after a later
  confirmation (one per volume).
- **Fence-only slots draw from `chunk_slots`:** exhausted, a last-chunk deletion fails and is
  retried.
- **A target of a deleted volume still running** can create chunks on a node that never held it:
  the incarnation follow-up.
- **The listing reads every chunk record every 5 s per node:** an index by node is a follow-up.
