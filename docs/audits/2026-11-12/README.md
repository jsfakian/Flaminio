# Correctness audit — 2026-11-12

Audited revision: `8aaa7a3a9151995d13abce717c7f7a9c6cf9977e` (`main`, after PRs #41/#42).
Review scope: architecture, accepted ADRs, replication protocol, format specification and the
implemented data/control-plane contract boundaries. Runtime code is unchanged by this review.
The accompanying architecture corrections and five new ADRs are drafts; **0007–0011 are
Proposed**, not silently accepted or implemented. ADR 0005 is not edited or superseded until
its replacement is accepted under the repository's ADR policy.

## Result and release assessment

**Nine confirmed defect groups: ten original desired-contract tests and the safe F09 probe.** Highest priority
is the io_uring borrowed-buffer error path (#85), unsafe node-wide shutdown assertion (#82),
and recovery failing above the 64-change metadata RPC boundary (#81). Fix and rerun the
reproductions before treating the current baseline as production-ready.

The existing suite passed, including simulation/mutations, crash consistency, Miri and full
address/thread sanitizers. The adversarial audit tests separately fail on the audited revision;
they test contracts absent from the baseline suite. Passing existing tests does not invalidate
those findings. No acknowledged-write-loss counterexample was found within the implemented
replication model's supported fault/clock assumptions. That is **not a proof** of absence of
such bugs or a certification of the real kernel, devices, Kubernetes deployment or future features.

## Confirmed findings

| ID | Priority | Finding and consequence | Reproduction | Tracking |
|---|---|---|---|---|
| F01 | P2 | A crash after the creating-header claim leaves the volume hidden; identical provisioning retries return AlreadyExists and cannot finish without cleanup | `TestAuditInterruptedCreateRetry` | [#76](https://github.com/jsfakian/Flaminio/issues/76) |
| F02 | P1 | Header Get and chunk List are not one validated snapshot. Delete/confirm/recreate can mix an old epoch/RF header with replacement chunks; loss of acknowledged data was not demonstrated | `TestAuditReadVolumeDoesNotMixIncarnations` | [#77](https://github.com/jsfakian/Flaminio/issues/77) |
| F03 | P1 | Creation's placement admission does not compare drain/topology state. New chunk placement can be published after a node was marked draining | `TestAuditCreateDoesNotPlaceOnConcurrentDrain` | [#78](https://github.com/jsfakian/Flaminio/issues/78) |
| F04 | P1 | CSI treats mount existence as compatible success, accepting another source or an incompatible readonly retry; staging/unmount ownership needs the same protection | `TestAuditPublishRejectsWrongExistingMount`, `TestAuditReadonlyRetryDoesNotAcceptWritableMount` | [#79](https://github.com/jsfakian/Flaminio/issues/79) |
| F05 | P1 | Metadata accepts partial final chunks that replica hello refuses, and per-node chunk counts above Hello's encoder assertion. A successful CreateVolume need not be attachable | `metadata_accepted_partial_chunk_volume_can_open`, `metadata_accepted_chunk_count_does_not_panic_encoder` | [#80](https://github.com/jsfakian/Flaminio/issues/80) |
| F06 | P1 | Target/adapter sends all pending ISR changes, but the Go API accepts at most 64. A 65-chunk peer-loss shrink is rejected and retried unsplit | `shrink_after_losing_peer_fits_metadata_rpc_limit` | [#81](https://github.com/jsfakian/Flaminio/issues/81) |
| F07 | P1 | Missing Kubernetes readiness plus missing Flaminio heartbeat triggers node-wide out-of-service without shutdown evidence or per-workload opt-in. A live partition can force-detach unrelated storage; downstream corruption was not induced | `TestAuditPartitionDoesNotAssertNodeShutdown` | [#82](https://github.com/jsfakian/Flaminio/issues/82) |
| F08 | P1 | `UringDevice::run` can return from fallible length validation with borrowed-buffer SQEs queued. A later call submits the returned batch's write and sees a stale CQE; freeing its buffers could cause a kernel use-after-free | `rejected_batch_does_not_retain_borrowed_buffers` | [#85](https://github.com/jsfakian/Flaminio/issues/85) |

| F09 | P1 | Legal geometry can accumulate more uncertainty than Fenced framing permits: one chunk exceeds the decoder extent limit, two chunks can panic the encoder on reconnect | `repro/fenced.py` safe in-memory probe | [#86](https://github.com/jsfakian/Flaminio/issues/86) |

P1 means address before a release exposing the affected contract; P2 means an availability
failure requiring recovery. These priorities are audit judgments, not existing GitHub labels.
F08's test keeps the borrowed allocation alive and reserves a read-only anonymous mapping
larger than 4 GiB without making 4 GiB resident. It uses a temporary regular file, demonstrates
bytes changing after the failed API returned, and does **not** intentionally dereference freed
memory. A later flush reports `short transfer: 4096 of 0 bytes`, exposing CQE contamination too.
Network request limits reduce its production reachability but do not make the safe Rust API sound.

Source boundaries:

- F01–F03: [metadata lifecycle/read/placement](../../../meta/service/service.go),
  [domain geometry/transition rules](../../../meta/domain/domain.go),
  [linearizable KV interface](../../../meta/kv/kv.go).
- F04: [CSI node lifecycle](../../../control/csi/node.go) and
  [mount interface](../../../control/csi/mount.go). The fake interface itself cannot inspect
  source identity or effective flags, which is part of the required fix.
- F05–F06: [target batching](../../../dataplane/crates/flaminio-repl/src/target.rs),
  [engine RPC adapter](../../../dataplane/crates/flaminio-node/src/engine.rs),
  [replica hello](../../../dataplane/crates/flaminio-node/src/replica/mod.rs),
  [frame bounds](../../../dataplane/crates/flaminio-net/src/frame.rs), and Go's
  `MaxChangesPerRequest`. The test metadata model does not enforce that production RPC limit.
- F07: [failover controller](../../../control/failover/failover.go). Kubernetes's
  [shutdown guidance](https://kubernetes.io/docs/concepts/cluster-administration/node-shutdown/#non-graceful-node-shutdown)
  requires verified shutdown/power-off before the node-wide taint. Multiple absence signals
  still do not establish shutdown. Epoch fencing protects Flaminio storage, not arbitrary pod effects.
- F08: [io_uring batch submission](../../../dataplane/crates/flaminio-uring/src/uring.rs).
  Unlike asynchronous commit submission, synchronous `run` does not prevalidate all lengths
  before mutating the submission queue. Its lifetime comment is contradicted by this early exit.

## Disposition of the eight design-review suggestions

| Original suggestion | Design action | Remaining implementation / evidence |
|---|---|---|
| D01 Conflicting takeover/reconciliation rules | Architecture §8.5/§11.4 follows the implemented full-fence/wait-and-reconcile gate. [ADR 0007](../../adr/0007-takeover-readiness-and-clock-contract.md) proposes superseding 0005; the old accepted file remains unchanged | Accept the superseding decision explicitly; keep early-read/membership/fast-read mutations caught |
| D02 Clock-stop assumptions | Send-time lease expiry and supported timing assumptions are explicit; process pause, host suspend and VM rollback are distinguished in ADR 0007 | [#83](https://github.com/jsfakian/Flaminio/issues/83); no suspend/VM-pause safety certification is claimed |
| D03 Metadata recovery authority | [ADR 0008](../../adr/0008-metadata-recovery-authority.md) requires an authority barrier, proven revision bound or specified recovery identity, incarnation/deletion/ISR/key inventory, quarantine and separate force recovery | #54 updated. Reserved descriptor pages and maximum epoch/sequence alone do not implement disk-only recovery |
| D04 Integrity and key deletion | [ADR 0009](../../adr/0009-encrypted-data-and-key-retention.md) separates confidentiality, authentication and freshness, and pins keys through retained snapshots/clones/backups/holds | #50/#51/#53/#54 updated; encryption and malicious-replay defense remain future work |
| D05 etcd replacement and controller availability | Architecture §5.4 requires learner catch-up and quorum-checked restartable replacement. §2/§5.5 distinguish storage progress from API-dependent pod recovery | #43 updated; #82 blocks unsafe node-wide shutdown assertions. This refines ADR 0002 rather than replacing etcd |
| D06 Block contract | [ADR 0010](../../adr/0010-block-device-durability-contract.md) scopes linearizability to blocks, disclaims multi-block atomicity, defers write-back, describes actual discard and conservative flushes | [#84](https://github.com/jsfakian/Flaminio/issues/84), #80, #46/#50/#75. Real ublk/FUA/FLUSH multi-queue conformance needs authorized hardware tests |
| D07 Scale and backpressure | Architecture §4/§16 removes unvalidated supported-scale inference, requires shared admission bounds, resource headroom and bounded queues with control priority | #72/#75 updated; #80/#81 show why simulator/model-only scale claims are insufficient |
| D08 Snapshot/backup commit boundaries | [ADR 0011](../../adr/0011-snapshot-and-backup-commit-boundaries.md) defines durable operation/barrier decisions, pins, manifest publication, group recovery and actual RPO | #51/#52/#53/#58/#59 updated; no implementation is claimed |

Accepted ADRs 0001–0004 and 0006 remain the selected language/backend/kernel/environment/edge
policy. The architecture now describes the implemented Go agent and shared engine mutex rather
than claiming the planned Rust agent and lock-free sharding already exist. The scope of future
clock, recovery, cryptographic-format and snapshot decisions is explicit in Proposed ADRs.

## Audit coverage and guarantees examined

This was a repository-wide contract-oriented review, with targeted source reads and executable
counterexamples, not a claim that every source line or possible interleaving was exhaustively
verified. Areas and evidence are recorded so further review can identify what remains unproved.

| Area | Reviewed contracts / source | Evidence and limits |
|---|---|---|
| Replication state machines | target fencing/readiness, per-block overlap serialization, durable all-ISR acknowledgement, committed shrink, pending joiners, resync filters/holes, uncertain-set reconciliation, stale sessions and fast-read completion | 20,000 seeded runs plus 93 release tests including caught mutations; F06 exposes an adapter limit outside the model |
| Simulator/checker | per-block linearizability search, optional unfinished writes, non-overlap order, fixed-rate clocks, crash/partition/restart and delayed local reads | Checks bounded histories, not multi-block transactions, arbitrary clock stop, Byzantine replicas or metadata restoration |
| Rust/Go metadata boundary | epoch/incarnation/token issuance, read-dependent CAS, chunk changes, deletion marks/tombstones, concurrent registration and placement/drain | Go/Rust vectors, real etcd tests and mTLS interop passed; F01/F02/F03/F05/F06 expose incomplete lifecycle/snapshot/bounds coverage |
| Replica runtime and storage | durable fencing before reply, session/epoch validation, commit reply ordering, live-write resync filter, checksum retry and read-plan cluster quarantine | Existing replica/store/engine tests and sanitizers passed. No raw-media power-cut campaign was run |
| Format/recovery | superblock/page pairs and checksums, feature refusal, checkpoint pages-before-header ordering, tail-batch data validation, replay ordering, journal reuse and poisoned-store behavior | 50,000 release crash cases; committed parser regressions; disk-only metadata reconstruction and new encrypted/tag formats are not implemented |
| Kernel/unsafe boundary | aligned allocation, held buffers, io_uring submission/CQEs/flush sequencing, staging mappings, local-read completion/revalidation | Miri covers aligned-buffer tests only; sanitizers cover test workloads. F08 contradicts one synchronous error-exit lifetime argument |
| Transport and authorization | TLS 1.3 mutual verification, peer identity, volume/node/epoch token binding, request framing bounds, metadata method authorization | Existing session/auth/token tests and vectors passed; encrypted at-rest authenticated freshness, key rotation and capability recovery identity are future contracts |
| CSI and controllers | publish/stage/unmount idempotency, formatting only without recognized signature, request geometry, tombstone confirmation, drain destinations, failover taints and API outages | F04/F07 confirmed with fake interfaces; real mount/ublk/cloud/Kubernetes behavior is not certified. #13/#14/#17/#18 remain existing open limitations |
| Operations/security | device claiming/probing, separate metadata authority, privilege boundary, certificate lifecycle, retained key/recovery dependencies and release diagnostics | #16/#43/#50–#54/#71–#75 already track major planned features; no penetration test, cryptographic proof or production-scale benchmark was performed |

## Validation executed

The following single command completed with exit status **0**, on the audited runtime code:

```sh
make lint test race hygiene contracts sim crash-test miri sanitize
```

| Check | Outcome |
|---|---|
| Lint (Rust/Go/proto/shell/actions and script checks) | Pass |
| Rust/Go tests and committed fuzz-regression inputs | Pass |
| Go race detector | Pass, COUNT=1 shuffled run |
| Hygiene/advisories/docs/static analysis | Pass; duplicate/yanked dependency warnings are not test failures |
| Generated contracts, vectors, fuzz lock and mTLS metadata interoperability | Pass |
| Simulation | 20,000/20,000 seeds (0–19,999) passed; 93 release tests including ignored mutation campaigns passed |
| Store crash consistency | 50,000 release proptest cases passed (162.51 s) |
| Miri | 2 aligned-buffer tests passed; does not execute kernel io_uring |
| Address/leak sanitizer | Full configured crate lib/tests passed |
| Thread sanitizer | Full configured crate lib/tests passed; QUICK was not set |

No `make deploy`, `make chaos`, shared-lab benchmark/fault test, hardware erase, or raw-device
command ran. The io_uring reproduction used only a newly created temporary regular file.
Adversarial reproductions are kept as `.txt` sources so ordinary builds do not acquire
intentionally failing tests. Run explicitly from this checkout:

```sh
python3 docs/audits/2026-11-12/repro/run.py go
python3 docs/audits/2026-11-12/repro/run.py rust
```

On the audited revision: **six Go tests fail and four Rust tests fail**. A nonzero test exit is
expected until their issues are fixed. The Rust runner uses the pinned 1.99.0 toolchain,
repository lockfile and cached dependencies (`--offline`); run the normal build first if those
are not installed. It reserves >4 GiB of virtual address space for F08 on 64-bit Linux, keeps
all borrowed buffers alive until the kernel is drained, and removes its temporary regular file.
The Python runners write scratch files outside the repository and reuse its Cargo target cache.
See [reproduction results](repro-results.txt) for concise observed failures.

## Remaining assurance limits and recommended order

The initial bounded audit completed for the scope above; subsequent issue-plan reviews continue
and can add findings, as F09 demonstrates. **Correctness is not established for every possible
state or the future roadmap**. Critical outstanding evidence includes paused/rolled-back clocks,
stale metadata restore with live old targets, production-scale failure storms, real mount/ublk
multi-queue ordering, genuine device power loss, and end-to-end encryption/snapshot/backup
features that do not yet exist. These require implementation and additional model/hardware
validation, not an assurance claim based on the current green suite.

Fix #85 and #82 first, then #81/#80 and the metadata/CSI races (#76–#79). Resolve/accept the
proposed ADRs and enforce #83 before expanding supported deployment environments. Continue
Phase 1's #15, Kubernetes packaging/cleanup and operator/observability work, then Phase 2's
automatic rebuild/resync/scrub with #72/#75 capacity and scheduling. No new runtime feature or
on-disk format is enabled by these documentation changes.

## F09 follow-up found during issue-plan review

Independent correctness/security reviewers of #80 identified Fenced uncertainty accumulation
which the geometry fix alone does not prevent. The retained [probe source](repro/fenced.rs.txt)
uses only ReplicaCore state and real frame encode/decode; it performs no disk/network I/O.
[Observed results](fenced-repro-results.txt):65,537 extents in one legal1GiB chunk encode to
524,317bytes but decoder refuses;131,086 extents in two such chunks panic encoding at
1,048,717bytes. This is tracked separately as #86; source runtime remains unchanged.

```sh
python3 docs/audits/2026-11-12/repro/fenced.py
```

Unlike the desired-contract tests above, this probe exits0 only when both current defects are
reproduced and caught. The expected encoder panic is caught in-process. After implementation,
replace those expectations with normal round-trip/coverage assertions in the regression suite.
It uses cached Cargo dependencies offline and temporary files; an optional CARGO_TARGET_DIR
can reuse a build cache. No new full-suite pass or fixed runtime is claimed by this follow-up.
