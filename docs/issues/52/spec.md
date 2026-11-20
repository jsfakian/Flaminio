# Issue #52: Incremental backups to S3 — specification

The existing [intent](intent.md)'s manual single-snapshot worker/repository increment.
[Design](design.md), [contracts](contracts.md), [ADR proposal](adr-proposal.md).
**Approved 2026-11-13; not implemented.** Changed after approval in plan review rounds 1 and 2
(decisions.md).
Schedules/retention, PVC restore and later steps remain open under #52.

## Terms

- **SnapshotRef:** source cluster/volume incarnation, committed snapshot name/cut generation,
  geometry and encryption/key descriptor. Snapshot g reads layers below g (#51).
- **Candidate diff(a,b):** sorted cluster-aligned ranges from layers [a,b); every real change
  is included, false positives permitted by #51. It is not a byte-exact change oracle.
- **Reader grant:** worker-bound, snapshot-pinned authority on registered source nodes, separate
  from a target session. It grants no current-data read, write, fence, attach or membership action.
- **Repository block:** up to 4 MiB of snapshot stored bytes at a fixed volume offset; holes
  resolve as stored zeros. Its keyed content identity is scoped to one repository.
- **Complete archive:** authenticated standalone manifest graph plus its immutable exact-version
  data/key descriptors. Plaintext restore of encrypted archives additionally needs retained keys.

## The invariant

A published archive resolves exactly one committed snapshot, never a mixture of incarnations,
partial source replicas or unfinished uploads. Replica reads cannot steal/fence the live target.
Earlier archives remain intact through interruption. No grant, timer or offline token permits a
new snapshot read after its explicit revocation/deletion barrier completes; already disclosed
bytes cannot be revoked. Observation, backup and retry never relax live replication guarantees.

## Behaviour

**S1. Scoped source and base.** Backup names one committed snapshot of one volume incarnation.
Choose the newest complete compatible base also retained by source replicas; otherwise explicitly
perform a full backup. Compare cluster identity, FirstEpoch, geometry, encryption/data identity
and snapshot ordering, not names alone. Incomplete/failed repository enumeration is an error,
not evidence of no base. A candidate diff may over-read unchanged 4 MiB blocks; report that cost.

**S2. Independent read authority.** An administrator authorizes a named worker/snapshot operation.
Metadata pins the snapshot/layers (and, once S8's encrypted backups are admitted, the required key
references) and records every source before issuing its grant. An encrypted volume is refused
(S8). A dedicated mTLS reader protocol serves only that immutable snapshot and allowed
ranges; it never enters target Hello/session fencing. A new connection validates active grant
state online. Unknown/wrong worker, node, incarnation, snapshot, bounds or grant generation fails
before any data is returned. The node takes the worker's identity from the worker's certificate;
the worker accepts only the registered source node's data plane as the server. Failover does not
silently change the snapshot or worker authority.

**S3. Revoke and delete.** Close grants to new sources/requests, invalidate source read admission,
and drain/cancel existing read permits before releasing pins or completing volume/snapshot
deletion. Every registered source learns of closure by asking the metadata service for its own
node, with or without a session and after a restart, and acknowledges it; unavailable holders
block unless their node is forgotten (#13 S19), the exclusion proof. No local expiry or liveness
observation substitutes for revocation. A restarted source validates metadata before admitting
old tokens. Once closing begins, no new grant or source is admitted for that volume, unless an
administrator reopens it after its deletion stopped (no grant open, the volume not marked
deleting). Deletion's externally absent state cannot precede this barrier, on every path to it
(#73's override included), and the barrier starts only after deletion's own admission checks: a
refused deletion closes nothing. Ordinary application I/O gains no metadata read; background
reader admission may consult metadata.

**S4. Complete replicas and bounded reads.** Read/diff only through source replicas proven complete
for that snapshot (#51's completeness evidence) and the registered chunk/source generation, never
a chunk of an earlier volume of the name; a failover does not stop such reads. Leaving the ISR,
re-creation, resync or move invalidates that source proof; reselect/revalidate without restarting
completed object uploads or trusting partial data. Reads are capped, fairly scheduled and
flow-controlled beside client I/O; no backup read takes the target's lease/session or waits on S3
in the engine.

**S5. Repository format and privacy.** A versioned repository has immutable 4 MiB blocks under
keyed BLAKE3 identities, optional bounded zstd compression and XChaCha20-Poly1305 encryption,
plus an encrypted complete manifest graph per archive. No other backup manifest is required to
read it; sparse all-zero blocks need no data object. Manifests authenticate offset/length/object
version and source/key identity. No payload, volume names or keys are clear bucket metadata;
format headers, object lengths/equality and timing remain observable and are documented.

**S6. Verified immutable publication.** Publish the final archive root only after every referenced
block/page and key descriptor is durable and verified under its exact object/version identity.
Existing object names/ETags alone are insufficient. Never overwrite a previous archive or rely
on the mutable newest version of a key. Bad/missing/truncated/substituted/oversized data fails
with its object identity, before wrong bytes are restored. Unknown format/crypto versions refuse.

**S7. Incremental/resume.** Resolve unchanged offsets from the base's complete map, read only blocks
intersecting candidate changed ranges and remove entries that become all-zero. Resume uses the
same operation/source identity and verifies already present immutable objects before reuse.
A killed worker/unreachable store leaves no listed partial archive and no damage to older ones;
retry completes without reuploading verified arrived content. Lost final acknowledgements resolve
by exact root/operation identity, not by publishing another ambiguous point.

**S8. Ciphertext and key lifetime.** *Deferred after approval (round 1, R1-C6, R1-S1), not
settled:* #50 and #51 (clear) admit no snapshot of an encrypted volume until its keys are pinned,
so none can be backed up here. This increment refuses an encrypted volume's grant; the rule
below, its retained-key records and its tests wait for the plan that lifts #51's refusal.
Encrypted snapshots are read as their stored ciphertext/zero representation; workers never
request a DEK. Retain/copy the encryption format/data identity,
wrapped-key/source/version references and required integrity metadata. Durable backup references
pin the keys needed by retained archives after ordinary live-volume deletion. Do not claim XTS+
CRC protects malicious modification/replay; #50/proposed ADR0009 determine that profile. In this
increment encrypted restore emits ciphertext only; plaintext recovery/escrow integration remains
#54's later work. Report that capability and do not certify unknown key availability as protection.

**S9. Repository-only listing, verification and output restore.** Bucket, repository descriptor,
repository key and authorized object-store credentials suffice to list/read complete archives
without source metadata. Verify reads every referenced block; optional source comparison requires
a live S2 grant. Restore writes a separately confirmed new file/device with matching geometry,
never overwrites an arbitrary path or runs mkfs. Sparse output is zero-correct even on a nonzero
pre-existing device. Missing/corrupt data leaves an explicitly incomplete output, never success.

**S10. Worker and client boundaries.** Use the proposed worker/repository implementation and
credentials in the ADR; no credentials/keys on argv, logs or bucket records. Object-client calls,
including retries/list/head/get/put and later multipart, pass a common admission/byte-observation
hook for #59. Payload counters do not claim TCP/TLS wire usage. Endpoint validation/TLS peer
checks and the qualified durable/version/conditional-write store profile fail closed. Offline
repository access never becomes live-volume mutation authority.
Settled at approval, 2026-11-13: the proposed worker, client and libraries (see decisions.md, Settled at approval).

**S11. Evidence, tests and docs.** Test publication/pins/revocation and source changes under crashes,
failover and concurrent deletion; fuzz all untrusted repository/protocol decoders and size limits.
Use a serverless test object store locally. Later authorize a qualified S3 lab with licence and
resource limits, measured worker/app I/O cost and restore/corruption/resume/ciphertext checks.
Report archive completion and actual last verified restorable-point age; schedule interval is
not RPO. Preserve the ADR approval gate and state which full-issue phases remain unimplemented.
Settled at approval, 2026-11-13: the ADR is accepted with the plan (see decisions.md, Settled at approval).

## Tests

- **S1–S4:** metadata/reader/service/simulator tests for wrong source/incarnation/worker, partial
  snapshots, diff coverage/false positives, moves/failover, no target-session disruption, read
  bounds/fairness and deletion waiting for all registered holders, including restart/offline ones;
  per-method roles (#73's table); a source with no session and a restarted one ack; a forgotten
  source unblocks deletion; a refused protected deletion leaves grants untouched; an encrypted
  volume refused; reads across a failover; an abandoned deletion followed by a reopen and a new
  grant; the reader's simulator mutations.
- **S5, S6:** repository/crypto golden vectors, version-pinned substitution/AEAD/hash errors,
  zero/final-block/large-map pages, decompression bombs and root-before-object mutation caught.
- **S7, S8:** injected store/worker faults after every object/root step, existing object altered,
  resume without duplicate upload; S8's encrypted profiles and retained key references deferred
  with S8.
- **S9, S10:** repository-only list/verify/restore, unsafe destination refusals/zero fill/partial
  output, credential redaction/TLS profile and universal client-hook coverage on retries.
- **S11:** serverless suite via Makefile; verify's restorable-point age; later lab reports real
  enforcement, bandwidth/overhead, data equality and unsupported remaining phases. No such
  runtime evidence exists from drafting.

## Open questions

The worker/client/library/repository byte format, reader revocation/deletion availability tradeoff,
object-store TLS/profile/licence and #50 key/reference seams remain Proposed. Resolve #51's stable
snapshot/root and canonical diff contract before implementation. No approved ADR or risk is implied.

Settled at approval, 2026-11-13: the worker, client, libraries and repository byte format as proposed (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the registered-source closure barrier; deletion waits for unreachable sources (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: the proposed S3 profile (mutual TLS 1.3, SigV4); SeaweedFS in the lab (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: pins keep the key references; the worker reads ciphertext only (see decisions.md, Settled at approval).
Settled at approval, 2026-11-13: #51's snapshot and diff interface as contracts.md proposes (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): #52 uses #51's completeness evidence and #61's store UUID as their plans settle them; its source proofs are built only once both are implemented, and a source without either is refused (see decisions.md).
