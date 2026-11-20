# Issue #91 — bounded resource lifecycle and process reclamation

Proposed, unapproved response to actual SEC91-R4-01/02. This appendix changes no guard
admission, enrollment, replica, metadata-authority or wire-protocol rule. No dependency patch
or runtime result is claimed. [Reader](reader-contract.md) owns collector/schema budgets;
[security](security-contract.md) owns actual container/resource configuration.

## Stock-library lifetime and budget boundary

Pinned `google.golang.org/grpc v1.85.0` checksum is
`h1:BPBCfuVRQ1qvBhKDOq9uvQllMvO59rKsYhXpp7Dd8vw=`; protobuf remains1.36.12 and x/net0.61.0.
Stock gRPC has process-global control-item/buffer caches. `itemList.dequeue` returns a node
to itemNodePool without clearing its item/next chain; incomingSettings can retain settings,
cleanupStream.onWrite captures transport/stream, and ≤1024-byte SliceBuffer.Free is a no-op.
ConnEnd/call return/GC observation therefore does not prove every old payload reference gone.

A1025-byte DATA frame can contain a5-byte gRPC header announcing R+1. The reader consumes
that header, leaves the remainder in recvBufferReader.last, and parser.recvMsg rejects the
size before reading it. Stock close does not deterministically Free every abandoned root.
A response BufferPool cannot fix separate control-item caches or this ownership path.

Use unchanged stock gRPC and its supported receive/header limits with bounded CodecV2.
There is no custom exactPool, global pool replacement, PADDED-frame interception, bespoke
HTTP2 parser, internal import, dependency fork or invented terminal-drain API. Ordinary
supported stock framing, including legal padded responses, remains compatible. Remove the
transport-capacity/cleanup machinery which existed only to support the withdrawn whole-gRPC
64MiB proof; it is not required by storage-drain admission.

The64MiB bound is **application-controlled reader accounting**: scan/construction plans,
decoded/retained candidates and explicitly reserved application state. It is not a claim
that unchanged gRPC's raw receive queues, pooled backings, bookkeeping, TLS, runtime heap or
all reachable reference chains total≤64MiB. Ordinary application-credit return follows the
bounded codec/call's actual unwind and release of application references, never a fabricated
library cleanup receipt. Whole-process containment and error reclamation follow below.

## Enforced containment

The controller container requests128Mi and limits256Mi, restartPolicy Always through its
Deployment. Require actual cgroup-v2 `memory.max` numeric and≤268435456 and
`memory.swap.max=0`; a stricter imposed limit is accepted and may reduce working memory.
Verify the current process's cgroup using bounded `/proc/self/cgroup` and mount information
at startup, before Active and every1s thereafter. Missing/unsupported/unlimited/invalidated
controls refuse startup or enter Quiescing. Runtime reads but never writes these controls.
GOMEMLIMIT and Go counters are not substitutes. The deployment runs the guard directly,
without a shell wrapper or transport helper. Only bounded kubelet exec health checks may run
transiently in the same container cgroup; no unconstrained helper escapes its limit.

Stock caches remain covered by the configured cgroup limit across successful calls. Linux
memory.max is a hard-limit/reclaim/OOM mechanism with documented temporary overage semantics;
this is not a mathematical instantaneous256MiB RSS/heap bound. Record actual controls,
memory.current/peak/events and refusal/OOM behavior separately from application64MiB counters.
Host/kubelet resources outside that cgroup are not claimed inside the application budget.

## Fail closed and recycle

Any metadata RPC receive/transport/decode error, size refusal, cancellation/deadline or
application-budget failure initiates process recycle. This deliberately includes ordinary
server error statuses; there is no attempt to infer whether an error abandoned a hidden root.
The sole expected-status exception is the reader's exact #90 full→paged discovery branch:
GetVolume FailedPrecondition with the defined paged-view API hint, no protobuf response
payload/codec invocation, followed by ReadIdentityVolumePage. That is normal protocol flow,
not a collector fault. A different FailedPrecondition, any response payload/codec invocation
on that branch, missing/ambiguous hint or page error follows hold/recycle; never retry a
malformed or unsupported view as legacy. Other unsupported/unknown input holds admission. Retained stock caches can
also reach the cgroup limit and cause OOM termination. No error reuses a suspect transport or
opens an uncharged replacement to make collection progress in that process.

1. Latch process state Quiescing once; bounded local exec-readiness state becomes failed.
   New review handlers return allow=false429 and no new collection/enrollment allocation
   begins. Existing handlers check before durable admission CAS/allow. Already committed
   in-flight admission may finish under the existing linearization rule. No durable slot is
   cleared and no additional grant is created by recycling.
2. Cancel metadata work and close sockets/ClientConns under ordinary bounded deadlines.
   Stop new admissions. Retain genuinely active application references/credits; never force
   Free/Put, zero a library root or infer quiescence solely from cancellation/ConnEnd.
3. Allow request/call goroutines at most2s to unwind, then exit75. One process-local watchdog
   starts at Quiescing and exits at that deadline even if library cleanup blocks. This kills
   the process instead of reclaiming buffers concurrently with RecvMsg or a codec.
   Exec-liveness also returns nonzero; timeout1s/period1s/failureThreshold2 and termination
   grace5s permit kubelet's terminate/kill fallback. No unauthenticated HTTP health path or
   mTLS exception is introduced. An OOM kill may happen sooner and remains fail-closed.
4. Kubernetes restarts the terminated container. The replacement has a fresh process/Go heap
   and no executable old callback. Observe actual previous container/process termination;
   readiness alone is not death evidence. Startup verifies memory controls/TLS, reloads all
   durable enrollment/nameclaim/slot state, reconstructs coverage and collects fresh strict
   metadata before allow. Process death stops local callbacks, not a Kubernetes CAS/update
   already submitted to a remote API server; that request may still commit after local exit.
   Re-read durable state and reconcile UID/resourceVersion/token preconditions before further
   action. Never reset accounting from a missing reply or assumed cancellation. Lost replies,
   occupied slots and unknown outcomes remain conservatively held.

Late internal Get/Put in the old process may still run before termination; no claim is made
that it completed cleanup, and its memory remains in that old cgroup. After process death
it cannot access the replacement's owners. A deadline while a decoder/read remains active
never authorizes in-process credit reuse. Persistent state is unaffected by heap reclamation.

Restart/backoff and metadata unavailability are visible held/unavailable admission, not a
successful drain or storage proof. Repeated error statuses may cause repeated restarts; this
is an explicit **Proposed, unapproved availability cost**. S1/S4 still deny admission while
the controller is absent. No bounded restart-to-availability time is promised: recovery needs
a functioning supervisor, API/credentials and healthy metadata. With those restored, a fresh
process can complete a healthy call without inheriting abandoned roots from earlier failures.

## Rejected dependency expansion

A maintained cleanup patch would need item-node clearing plus deterministic terminal drain
of reader.last/queues/framer after all reader/codec/producer ownership quiesces. That receipt
does not exist in v1.85.0. It is unnecessary for this resource contract and is not proposed
for implementation. Future uninterrupted same-process recovery or a whole-library64MiB
requirement would need an explicit new decision; public pool options cannot supply it.

## Future files and qualification

`control/disruption/{reader,lifecycle}.go`, command startup and `deploy/disruption/` own the
Quiescing latch, bounded single exit watchdog, cgroup check/monitor, exec health and restart
supervision. No gRPC/vendor/go.mod/go.sum change is required. Future checks use
`make lint test race hygiene contracts`; none ran as part of this docs-only revision.

Real stock-transport/process fixtures must cover cached settings/cleanupStream references
across rotated successful calls, ≤1024 DATA, exact1025/R+1 refusal, queued/partial cancellation,
codec rejection, paused RecvMsg/producer/codec and late internal Get/Put. Ordinary padded
responses from a stock metadata peer succeed. Assert application64MiB separately from the
actual cgroup controls/events, no force-Free or fictional cache purge, Quiescing denial and
unchanged durable slot. Four malformed attempts across supervised restarts, then healthy
metadata, must recover through a fresh process. Test blocked cleanup's exit/kill fallback,
invalidated memory controls and OOM restart. Preserve original held/admitted drain and data
qualification; these resource fixtures do not replace them.

[Linux cgroup-v2 controls](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory) define
memory.max reclaim/OOM/temporary-overage and separate swap accounting.
[Pinned Kubernetes1.36 lifecycle](https://v1-36.docs.kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#container-restarts)
distinguishes container restart/backoff from process-local cleanup. These sources support the
Proposed containment behavior; no corresponding runtime qualification is claimed.
