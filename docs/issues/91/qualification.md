# Issue #91: qualification and evidence

Proposed, unapproved appendix to [design](design.md). All checks below are **future** checks;
this draft records no runtime/API/storage pass. No shared lab, disk or deployment action is
authorized by writing/reviewing these documents. Original #63 S8 evidence remains required.

## Pinned API and independent evidence classes

Qualify against actual k3s **v1.36.5+k3s1**, embedded Kubernetes **v1.36.5**, matching
`scripts/lab.conf:114`. Capture `/version`, binary/image digests, webhook configurations,
admission plugin/feature settings and exact guard/dependency commits for every run.
Record both-plugin credential configuration and the authenticated peer URI observed from
each HA API-server instance; a caBundle-only/server-authenticated run is not qualified.
The [k3s release](https://github.com/k3s-io/k3s/releases/tag/v1.36.5%2Bk3s1) identifies the
embedded version. A different release is unqualified until the same suite runs there.

| Evidence | Environment | What a passing result proves |
|---|---|---|
| Unit/service/transport | Local Mem and real disposable etcd + mTLS gRPC | Bounded reader, auth, adapter and reducer contracts |
| Real API mechanism | Disposable pinned k3s cluster; API-only Pods and metadata fixtures | Admission matching, UID timing, CAS, lifecycle, Restricted admission and authorizer behavior |
| Historical storage exploration | Explicitly authorized pre-#83 VM stack, exact old commits recorded | Exploratory observations of that historical stack only |
| Composed RF2 storage acceptance | Separately authorized qualified physical hosts, pinned API and #83 profile | Original stop/return/drain/data proof for the composed implementation |
| Current VMs with #83 active | Existing lab VMs, no false physical attestation | Expected production-admission refusal; S8 storage acceptance remains blocked/uncollected |

No row substitutes for another. Fake API/PDB status cannot certify the real API row, and
passing API evictions cannot certify data durability. A skipped or unavailable physical run
is explicitly **uncollected/blocked**, never pass. Do not weaken #83, supply `--force`/an
environment bypass, forge physical provenance or omit the admission guard to green a test.

## Real API counterexamples

Implement future `make disruption-guard-api-test` using an explicitly disposable pinned
cluster and temporary namespace/credentials. This is an API qualification target, not a
shared-lab default or a data-plane launch. It uses stock kubectl and actual API server
admission/controller behavior, actual ConfigMap resourceVersion conflicts and observed Pod
UIDs; no synthetic PDB status or monkey-patched successful authorizer. Time-bounded waits
have explicit failed/blocked outcomes and save redacted object/response histories.

Required fixtures, mapped to [spec](spec.md):

- S1/S7: Node CREATE with actual server-generated UID; pause commit after admission, then
  resume; label update; existing-node bootstrap; failed create's Staging enrollment retained;
  deletion/recreation with same name while old controlled anchor remains and controller is absent.
  No UID affinity field or trusted name-only identity is used. Cold/empty-node ordinary drain
  must encounter its anchor after declared installation activation.
- S1/S6: actual pinned API servers use the exact both-plugin client certificate configuration
  in [security](security-contract.md). Successful authenticated Node enrollment, eviction UID
  mutation and eviction validation cover every HA instance. No-client/wrong/expired-client
  forged and replayed reviews allocate zero nameclaims/enrollments/anchors/slots; omit or
  mismatch one plugin's credential entry and show Fail-policy refusal. Exercise atomic trust/
  serving/client rotation, old connections closed, no anonymous startup/health fallback.
- S1/S2/S7: [identity-contract](identity-contract.md) fixtures for exact-name/no-alias join,
  durable no-history proof, nonstorage terminal/image refresh and drain/uncordon/release
  followed by storage maintenance; registration appearing during recovery, formerly-storage
  absence, same-name new UID/store replacement and retained old earned refs. Repeated
  kubelet-equivalent POSTs/AlreadyExists and overlapping delayed creates allocate no doomed
  extra interlock and remain ordinarily drainable.
- S1/S3/S4: three Nodes A/B/C with RF2/minISR1 fixture on A/B. Give C duplicate Ready
  lookalikes/rollout Pods, run simultaneous ordinary A/B drains and show at most one distinct
  node owns the durable slot. C readiness/surge must never add credit. Pending, Succeeded,
  Failed, unready, terminating and replacement anchors still hit fail-closed admission, even
  where the pinned eviction handler bypasses PDB checks.
- S3/S4/S7: kill all webhook endpoints before a request and after slot CAS; fail metadata
  reads, stop the API client, drop CAS/allow responses, cancel drain, overlap/restart
  controllers and resume paused old replies. New admission holds; ambiguous reservation
  persists. A previously committed operation may finish, as the explicit linearization rule
  says. Uncordon/recovery retires the exact old Pod UID before releasing; a delayed old
  eviction with injected UID precondition cannot delete its replacement.
- S2/S6: route actual strict service reads through mTLS reader, including sole and mixed
  malformed headers, errors/caps and exact/paged identity fixtures from [reader](reader-contract.md).
  No direct etcd-to-guard injection path is accepted as a service-completeness proof.
  Add actual malicious-gRPC predecode fixtures for repeated-empty and nested message
  expansion on all four response types and encoded boundary/+1. Supported compressed
  responses within ordinary encoded/decompressed receive and cardinality caps are accepted
  and decoded; unsupported encoding, malformed compression and decompressed boundary+1
  hold/recycle under the stock policy. Exercise
  concurrent transient/retained application accounting; credit high-water stays≤64MiB before
  generated object construction. Normal generated service responses alone do not prove it.
  Exercise the stock HTTP2 path at receive/header/trailer boundary/+1, four calls and
  application-credit exhaustion, ordinary pooling with alternating1025-byte/16KiB DATA,
  valid PADDED frames, queued/partial cancellation, codec errors and R+1 advertised length
  in one1025-byte DATA frame. No custom exact-pool, padding prefilter or whole-transport
  memory proof is required. Expected #90 full→paged discovery proceeds without recycle;
  unexpected statuses/hints and page failures hold and recycle.
  [Resource lifecycle](resource-lifecycle-contract.md) proves verified memory.max/swap0,
  Quiescing/exit75 and OOM supervision with durable slot state preserved. Four hostile
  attempts followed by a healthy peer must recover across supervised restarts when needed.
  Record application64MiB credits separately from stock cache reachability, cgroup events/
  peak and kernel temporary overage. Timeout/ConnEnd/GC alone establishes no cache purge,
  and same-process recovery is not promised.
- S4/S5/S6: global eviction/node match under all namespaces/labels, dry-run no slot write,
  expired/wrong serving TLS, malformed bodies, exact allowed/denied RBAC and Pod Security
  fixtures from [security](security-contract.md). Demonstrate direct deletion/disable-eviction
  only as documented privileged bypass; never count it as successful guarded drain.
  Slow TLS/header/body/idle/write and connection/worker saturation cases must demonstrate
  finite reclamation, no HTTP2 stream support, authenticated health policy and successful
  legitimate API requests after abusive connections close. Capture actual deadlines/ALPN.
  Include nonzero body/decode/initial-read time before a reader consumes its full **remaining**
  allowance: with H fixed, its deadline is min(readerStart+20s,H+23s), never a reset24/28s
  budget. Capture actual RPC/API contexts and dispatch timestamps; assert no new durable
  write/CAS/retry starts at/after H+23s and bounded response/unwind ends by H+24s. A CAS
  dispatched before the cutoff whose reply is lost may commit later: keep its unknown slot
  held, including across process replacement, until ordinary durable re-read/reconciliation.

Future files: `control/disruption/*_test.go`, real-API harness under
`scripts/tools/test-disruption-guard-api.sh`, deployment fixtures under `deploy/disruption/`.
The harness cleans only its exact installationID/object UIDs after taking down test clients;
it must not delete a production webhook or reset an ambiguous production slot.

## Storage preflight before any stop or cordon

Future authorized `make disruption-guard-test` first records actual dependency commits,
clock-check/startup profile output, host provenance, existing cordon state, Node UIDs,
store UUID/generation, guard installation/version/coverage, exact API version and metadata
identity mode. It requires dedicated RF2/minISR1 test data on A/B, verified starting contents,
full earned ISR and no unrelated workloads or storage obligations on the affected hosts.
The proposed target and `scripts/checks/disruption-guard.sh` must refuse before any
stop/cordon/drain when those prerequisites cannot be established.

With #83 active, every storage process must pass **physical-boottime-v1** under its exact
qualified clocksource/error/rate and operator deployment-attestation contract. Existing
fl-a/fl-b VMs are excluded: record their expected refusal before storage opens/registers/
listens and terminate the S8 run **blocked**, without stopping/cordoning another node.
An authorized historical pre-#83 VM run must be labeled exploratory with exact old commits;
it cannot attest to the composed implementation. No test script writes a false physical
contract, hides a hypervisor, weakens detector inputs or silently chooses an older binary.

The composed acceptance path is a later, separately authorized physical-host environment
with equivalent named roles A/B and the same pinned k3s/API release, dedicated test disks,
installed reviewed guard/#69/#77/#83/#90 composition and all profile prerequisites. Preparing
that environment is outside this guard slice; absent resources leave evidence uncollected.

## Original RF2 data sequence and result record

After preflight, the authorized operator:

1. Verifies the dedicated volume's known checksums/content; stops A's data plane only.
   Waits for observed metadata liveness loss, then starts ordinary eviction-enabled drain B.
   Drain must remain held and report that actual volume/chunk and unavailable surviving member.
2. Restarts A under the same qualified profile/actual opened store; waits for full earned ISR
   and fresh observation. The **same pending drain B** may complete without a Flaminio override.
3. Keeps B cordoned and initiates drain A. It must remain held. Returns B to schedulable,
   live/full-earned-ISR and actual anchor-ready state; after retirement/recovery bookkeeping,
   the pending drain A may complete. Include simultaneous-drain and metadata/controller-loss
   cases independently; no bypass/forced delete is an acceptable release.
4. Restores original process/cordon/workload state and verifies all test data after recovery.
   Save checksums, full exact refs/incarnations, Node/Pod UIDs, ordered admission/CAS outcomes,
   reason evidence and actual versions, with Secrets/tokens/private keys redacted.

No VM shutdown, raw-device format, destructive claim or unrelated workload eviction belongs
to this target. A held/timeout drain is a named failed/blocked observation unless that step
explicitly expects a hold. Cleanup uses exact owned UIDs, never global label-only deletion,
and preserves ambiguous admission accounting until the ordinary documented recovery path.
Record each row as pass/fail/blocked/uncollected with evidence path; failing cleanup is visible.
Update README, docs/lab.md and docs/followups.md only with actual implemented commands and
collected results. Docs-only link/whitespace checks do not count as any runtime evidence.
