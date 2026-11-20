# Issue #91: namespace, credentials and diagnostics

Proposed, unapproved appendix to [design](design.md) and [admission-contract](admission-contract.md).
The historical #63 `--namespace flaminio-system` command is provenance; it is not this command.

## Namespace and process boundary

Install namespace **flaminio-disruption-guard**, separate from privileged flaminio-system.
Installer sets Pod Security labels enforce/audit/warn=`restricted`, all three version labels
`v1.36`. It contains only guard objects: controller ServiceAccount `flaminio-disruption-guard`,
unbound/default no-rights ServiceAccount, the guard Deployment/Service, anchor Pods,
state/enrollment/status ConfigMaps and two Secrets below. No CSI/agent/admin/node/CA
private credentials, stronger ServiceAccounts or tenant workloads may be installed there.
Guard runtime cannot write Namespace labels, ServiceAccounts, Secrets, RBAC or admission
configuration. Cluster administrators installing other credentials/privileged bindings in this
namespace invalidate this boundary and must be documented as privileged bypass.

All containers run nonroot with RuntimeDefault seccomp, no privilege escalation, all Linux
capabilities dropped, read-only root filesystem and bounded ephemeral scratch. No hostPID,
hostIPC, hostNetwork, hostPath, device mounts, PVC/data volumes or privileged containers.
Anchors automount no service-account token and mount neither credential Secret. Their idle
image is pinned by digest; requests/limits are10m CPU/16MiB memory per anchor. Controller
requests100m CPU/128MiB, limits1 CPU/256MiB, with64MiB application reader credits and bounded
queues. Actual stock-library/cache allocations are bounded by the verified cgroup hard cap,
not by claiming complete visibility through those credits; see
[resource lifecycle](resource-lifecycle-contract.md). Swap must be disabled for that cgroup.
Installer owns controller Deployment updates; runtime only reconciles anchor Pods.

Creating a workload can select local ServiceAccounts and mount local Secrets without direct
Secret-read or impersonate rights. Denying those verbs alone is insufficient. Here every
usable local account is no stronger than the already compromised guard account, and the only
credentials reachable through workload creation have the guard's same authority. Pod Secret
references cannot name another namespace. Restricted admission closes privileged-host paths;
it does not inspect metadata-reader authority. No additional generic policy engine is needed
for this isolated namespace. [Kubernetes workload-creation escalation guidance](https://kubernetes.io/docs/concepts/security/rbac-good-practices/#workload-creation)
describes this trust boundary.

## Exact credentials and TLS

`flaminio-disruption-guard-reader-tls` contains only reader leaf certificate, private key and
public metadata CA bundle; URI is `spiffe://<trust-domain>/flaminio/reader/disruption-guard`
under #69. It contains no signing key, admin/node/io certificate or capability token.
Read-only Secret mounts use0440 files with nonroot UID/GID65532 and fsGroup65532 so the
controller can read them without a privileged ownership-changing init container.
Reader verifies configured CA, endpoint hostname/IP and the configured metadata-server
identity `spiffe://<trust-domain>/flaminio/node/<server-node>/meta` using PKI helpers; no
InsecureSkipVerify, insecure development mode, admin fallback or arbitrary server-selected
trust roots. Failed/missing/expired/rotated-invalid credentials hold admission. Valid rotation
reloads a complete matched cert/key/CA set; mixed files fail without retaining cached success.

`flaminio-disruption-guard-webhook-tls` contains only HTTPS server leaf/key for
`flaminio-disruption-guard.flaminio-disruption-guard.svc` and public serving CA chain.
Installer writes its CA to both webhook configurations' caBundle. This serving certificate
is not trusted as a metadata client credential; metadata Reader URI/caller parsing rejects it.
CA private keys remain with the installer. Runtime cannot renew itself through CSR approval.
The API server also presents a dedicated client certificate under the following exact
configuration. AdmissionReview fields authenticate nobody. NetworkPolicy restricts reachability
to9443/API/metadata endpoints but cannot replace TLS peer authentication or object validation.

Kubernetes client uses its projected token/CA and verifies the API server. Reload rotated
tokens; never log Authorization headers. Anchor pods have no token. Runtime has no exec/logs/
attach/portforward/token-request/impersonate privilege. Readiness cannot substitute for auth.

## API-server client authentication (R1-C1, SEC91-R1-01)

On **each** pinned k3s server, installer merges into `/etc/rancher/k3s/config.yaml`:

```yaml
kube-apiserver-arg:
  - admission-control-config-file=/etc/rancher/k3s/flaminio-admission.yaml
```

The root-owned0600 admission file is concrete configuration for both plugins:

```yaml
apiVersion: apiserver.config.k8s.io/v1
kind: AdmissionConfiguration
plugins:
  - name: MutatingAdmissionWebhook
    configuration:
      apiVersion: apiserver.config.k8s.io/v1
      kind: WebhookAdmissionConfiguration
      kubeConfigFile: /etc/rancher/k3s/flaminio-webhook-client.yaml
  - name: ValidatingAdmissionWebhook
    configuration:
      apiVersion: apiserver.config.k8s.io/v1
      kind: WebhookAdmissionConfiguration
      kubeConfigFile: /etc/rancher/k3s/flaminio-webhook-client.yaml
```

Its root-owned0600 client file is `apiVersion:v1`, `kind:Config`, with exactly one `users`
entry named **flaminio-disruption-guard.flaminio-disruption-guard.svc:443**. That entry's
`user.client-certificate` and `user.client-key` are absolute paths to this server's dedicated
root-owned0400 client leaf/key. No wildcard/default/context/admin/bearer/exec credentials.
Service443, not container9443, is the resolver key. The pinned
[resolver](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/util/webhook/authentication.go#L127-L205)
matches that exact host:port; missing matches otherwise become anonymous. Both admission
plugins must be enabled and neither may be listed under disable-admission-plugins. Pinned
[k3s argument forwarding](https://github.com/k3s-io/k3s/blob/v1.36.5%2Bk3s1/pkg/daemons/control/server.go#L191-L266)
passes this API-server argument; no upstream manifest or unsupported credential flag is assumed.

A dedicated **admission-client CA**, separate from metadata, Kubernetes API-client and serving
CAs, issues only these client leaves. Private CA key never enters the cluster namespace.
Leaf has DigitalSignature, exactly ClientAuth EKU, no ServerAuth/Any EKU, non-CA, and exactly
one URI `spiffe://<trust-domain>/flaminio/node/<control-plane-id>/admission-apiserver`.
Control-plane IDs use existing128-byte segment grammar and installer allowlists≤16 exact
URIs in a≤16KiB trust configuration. No CN/DNS/userInfo/forwarded-header/ServiceAccount token
substitution. The public client CA and allowlist are mounted with serving material; API-server
private client keys remain only on their respective control-plane hosts. Neither guard
namespace leaf can authenticate as an admission caller, metadata writer or Kubernetes admin.

Listener requires TLS1.3 `RequireAndVerifyClientCert`, verifies validity/chain to only that
client CA and exact EKU/URI allowlist in VerifyConnection **before HTTP/body parsing, API
calls, name claims, enrollment counts, anchors or slot writes**. Missing/wrong-role/expired
client fails the handshake, with zero actionable decode/allocation. Do not honor TLS early
data, client-supplied trust or X-Remote-* identities. Disable session tickets so each new
connection revalidates current trust. Each request rechecks its verified leaf's validity and
current trust generation; trust reload closes existing connections before admitting more work.
Actual-object reads remain validation of trusted API-server requests, not origin authentication.

Fail startup closed on missing/unreadable/mismatched leaf/key/CA/allowlist, empty client trust,
unknown URI or unsupported TLS settings. Installer restarts each k3s server after writing its
files; credential files/resolver configuration are not assumed hot-reloaded. Before Active,
every HA instance must demonstrate authenticated mutating eviction and validating Node/eviction
requests through the actual API path. Missing client config causes TLS failure and Fail-policy
rejection, never anonymous enrollment. Record observed peer URI per server without key material.

Rotation: install overlapping old+new public admission-client trust on guard, provision new
per-server leaves, atomically replace each host's matched file set and roll that k3s server;
verify both plugins for every HA instance, then remove old trust and close old connections.
Serving-CA rotation similarly overlaps webhook caBundle before replacing the serving leaf.
Trust replacement is atomic; invalid reload closes readiness/admission and cannot retain a
permissive fallback. An outage holds requests. Compromise of a dedicated API-server client
key is compromise of this trusted admission caller; network policy is only additional defense.

## Bounded HTTPS listener (SEC91-R1-06)

`admission_server.go` wraps the TCP listener **before** net/http/TLS spawning:32 accepted
connections maximum, no wait queue; excess accepted sockets close immediately. Every admitted
socket owns one permit until Close, including handshake, pre-header and idle states. Set
raw connection deadline before TLS; handshake≤2s and maximum4 simultaneous handshakes,
nonblocking rejection. Verify TLS peer before handing the authenticated connection to HTTP.
During handshake the raw connection wrapper permits≤64KiB total received bytes, then closes
without another read; this applies to encrypted TLS1.3 records too and requires no encrypted
Certificate parser. Pinned Go1.27.4 already caps one Certificate handshake message at256KiB
before reading it (`crypto/tls/{common,conn}.go`); account for that transient preallocation,
not an invented pre-x509 callback. Reserve16MiB per active handshake for TLS/x509 scratch;
chain length≤3 and combined DER≤16KiB are verified before HTTP use. Hostile-handshake
allocation fixtures enforce this conservative scratch ceiling for the pinned toolchain.
No unbounded accept/handshake goroutine or error-log queue. Keep≤64KiB/socket transport
storage plus bounded peer state and reserve an additional1MiB per accepted connection for
net/http prehandler header maps/strings/slices, including parser byte-limit read-ahead.
The raw16KiB header limit alone is not a decoded-memory bound; many short headers consume
this scratch before handler validation. At most32 connection goroutines and8 request workers,
with closed sockets releasing all permits even on parse/TLS/cancellation errors.

Use HTTP/1.1 only: `Server.Protocols` enables HTTP1 alone, TLS ALPN only `http/1.1`, empty
TLSNextProto map, no h2/h2c/upgrade handler. This matches the pinned Service webhook
[client](https://github.com/kubernetes/kubernetes/blob/v1.36.5/staging/src/k8s.io/apiserver/pkg/util/webhook/client.go#L157-L198),
which explicitly selects HTTP/1.1; no HTTP/2 streams or per-stream queues exist here.
Set ReadHeaderTimeout=2s, ReadTimeout=3s, WriteTimeout=29s, IdleTimeout=5s,
MaxHeaderBytes=16KiB and MaxHeaderValueCount=64. Reject Content-Encoding, Upgrade, trailers,
ambiguous length framing, and unsupported method/path/content-type before reading bodies.
Body≤1MiB via MaxBytesReader, one JSON value/depth32. A bounded JSON scanner projects only
the AdmissionReview/Node/eviction fields used by the admission contract, skips irrelevant
values without materializing them, and never unmarshals a whole generated Node/Pod first.
Reject duplicate actionable keys, >8192 tokens, >128 label/annotation pairs, >8 owner refs,
IDs>128 bytes, other strings>4096 and >1MiB projected charged data before construction.
Body plus projected state fit the2MiB worker allowance; unknown operation/resource refuses.

At request-header completion set one absolute24s deadline, covering **body read, decode,
validation, reader/API calls, reply and unwind**. With≤2s TLS and≤2s headers, this totals≤28s,
below webhook timeout30s. Body reading ends
within3s; use ResponseController.SetReadDeadline and request context together, and close the
connection/body on expiry so a slow sender cannot leave a blocked Read. Reserve≤1s for
bounded redacted response/cleanup; with header-completion H, stop actionable work at
A=H+23s and finish reply/unwind by D=H+24s. Body/decode and initial reads consume that same
budget. Reader deadline=min(readerStart+20s,A); each RPC=min(RPCStart+2s,readerDeadline),
and each Kubernetes read/write/CAS=min(operationStart+2s,A). Check the cutoff before every
durable dispatch/retry; never start one at/after A. A submitted write may still commit after
cancellation, so unknown outcomes remain held. Set per-request write deadline to D (29s server timeout is
only a fallback). Eight active handlers maximum, no worker queue, excess429/close without
body drain. Decode/body buffers are capped≤2MiB/worker, and response≤16KiB; JSON scanner
uses fixed depth/counter state, not an unbounded map. Cancellation closes active body reads,
cancels RPCs, releases decode/reservation permits, and retains possibly committed slots.
No background goroutine is left waiting to write a timed-out response. Shutdown closes
listener and idle sockets, cancels active requests, waits at most3s then closes remaining
sockets; no accounting reset. [Go server timeout/protocol controls](https://pkg.go.dev/net/http#Server)
have independent defaults, so each value is set explicitly.

Health probes use an exec command reading a bounded process-local readiness file/state;
there is no unauthenticated HTTP health listener/path or TLS auth exception. Network health
requests, if ever added, require the same peer authentication and transport budgets.

## Exact runtime RBAC

One ClusterRole bound only to the guard ServiceAccount:

| API group | Resource | Verbs | Purpose |
|---|---|---|---|
| `""` | nodes | get,list,watch | Immutable UID and schedulability/readiness observation |

One Role in **flaminio-disruption-guard** bound only to that account:

| API group | Resource | Verbs | Scope |
|---|---|---|---|
| `""` | configmaps | get,list,create,update | State/status/enrollment only; namespace is enforcement boundary |
| `""` | pods | get,list,create,delete,update | Anchor creation/verification, UID/RV finalizer update and exact old-UID retirement |
| events.k8s.io | events | create,patch | Bounded owned diagnostics; no core/v1 Events path |

No Pod patch or StatefulSet/Deployment/ReplicaSet/PDB/Lease permissions, no
evictions, no node write, no Secret access, no flaminio-system RoleBinding and no webhook/
RBAC/Namespace/ServiceAccount mutation. Delete requests always carry UID preconditions;
finalizer updates carry UID/resourceVersion and modify only the owned anchor finalizer.
No image/spec update refreshes an existing anchor. Every replacement follows the single-slot
lifecycle; no Ready count grants admission capacity.

Installer precreates `flaminio-disruption-guard-state` (≤16KiB) and
`flaminio-disruption-guard-status` (≤32KiB); runtime creates `enrollment-<nodeUID>` records
(≤4KiB each, ≤4096 allocated across the installation lifetime, including unresolved staging
and retained Deleted records; no automatic decrement/reuse), and the exact
`nameclaim-<sha256(name)>` records (≤1KiB each,≤4096 distinct names) defined by
[admission](admission-contract.md). No delete of admission/enrollment/nameclaim
ConfigMaps is granted; absent/conflicting state fails closed. Dynamic ConfigMap create and
Event create cannot be restricted by `resourceNames` in Kubernetes RBAC. Dynamic enrollment
update likewise needs their names at runtime: do not publish fictitious static name
enforcement. Namespace isolation is the granted upper bound; code accepts only exact owned
names/UIDs/installationID. The installer is a separate explicitly privileged deployment actor;
runtime never receives that actor's credentials. [RBAC named-resource restrictions](https://kubernetes.io/docs/reference/access-authn-authz/rbac/#referring-to-resources)
are not a substitute for this boundary.
Evictions outside this fixed namespace cannot target an anchor and use the admission
contract's immediate non-anchor path; no tenant Pod GET is required. Other namespaces still
hit the global fail-closed webhook configuration; a serving outage is not a selector bypass.

## Status, Events and failures

Each enrollment ConfigMap's JSON remains≤4KiB **including status**: schema/installationID,
Node name/UID and recorded anchor UID, state, last observation digest/time, reason code,
up to8 sorted volume/chunk reason entries (≤256 UTF-8 bytes each), and omitted_count.
Truncate only diagnostics, with explicit omission count; never truncate the eligibility
calculation. [Identity-contract](identity-contract.md) owns saved store/history evidence;
it has priority within4KiB, so reduce diagnostics and raise omitted_count to fit it. Singleton
status contains schema, installationID, active/unknown, generation,
slot owner UID/phase, counts and≤64 node summaries, plus omitted_nodes, total≤32KiB.
All fields are public operational metadata. `token` in durable slot state is an idempotency
identifier, not a capability; do not copy even it into public Events unnecessarily.

Codes include MissingCoverage, MetadataUnknown, MetadataIncomplete, UnsupportedIdentity,
WouldBreachMinISR, OtherDrainInProgress, ForeignResource, StatusUnavailable and
MechanismUnqualified. A held response names the bounded node UID and first blocking volume/
chunk when known; logs and enrollment status carry the same reason. An unknown read never
produces a fake blocking volume. Escape untrusted IDs/control characters before text output.

Use only `events.k8s.io/v1` Event: generated name `guard-<nodeUID>-<random>`, regarding exact
enrollment ConfigMap UID, reportingController=`flaminio.io/disruption-guard`, reportingInstance
the controller Pod UID, type Normal|Warning, bounded reason/action, note≤1024 UTF-8 bytes.
Create on a changed reason; patch that known Event only to advance series count/lastObserved.
Per-node at most1 new Event/minute; global at most10/minute, burst20, bounded4096-entry
rate/dedup table. Expired/NotFound Event is recreated only within that budget. No Events for
every healthy poll; no unbounded in-memory retry queue. API Event TTL handles retention.

Before any allow, write required current enrollment/status updates and then the slot CAS;
status failure denies the request and leaves any possibly committed slot held. Event failure
does not undo/reset a slot and never manufactures eligibility; emit one bounded stderr reason
and continue to hold if required status also failed. Recovery release requires successful
status/state CAS under the admission contract. Never clear unknown accounting to publish a
healthy status. Logs/Event/status contain no PEM, tokens, credentials, volume data or full
RPC/request dumps; maximum diagnostic line4096 bytes, bounded error classifications.

## Required authorization and abuse fixtures

- Real API authorizer as the guard SA: allowed owned ConfigMap create/update and Event
  create/patch; deny foreign-namespace updates, nodes patch, tenant Pod delete/evict, Secrets
  get/list, ServiceAccount/token creation, impersonation, RBAC/webhook edits, Deployment and
  Lease writes. Assert the broader same-namespace create boundary honestly.
- Attempt Pod creation in flaminio-system using its CSI SA/admin Secret: Forbidden
  by RBAC. In the guard namespace, a CSI SA is absent and a same-named CSI Secret cannot be
  resolved there; prove no token/admin Secret is mounted and no metadata admin call succeeds.
  Do not claim RBAC inspects Pod specs or that missing Secret mounts must reject Pod CREATE.
- Restricted admission rejects privileged, host namespace/hostPath/device/capability requests
  for actual created Pods, including replacement/terminal-anchor cases. Guard credentials
  cannot remove Namespace labels or add a stronger SA/credential. Verify controller and idle
  anchor manifests actually satisfy Restricted v1.36.
- Real mTLS reader permits exact reads/page only; reject admin/replica actions, wrong CA/SAN/
  trust domain, expired leaves, mixed credential rotations and webhook leaf as metadata client.
- `TestAdmissionOriginRealTLS`: no client, wrong CA/EKU/URI/control-plane ID, expired leaf,
  metadata/serving leaf and replayed direct/forged Node CREATE reviews fail before body/action
  decode and leave name claims/enrollments/anchors/slot/counters unchanged. Actual API-server
  clients exercise both plugins/all paths on every HA instance; exact duplicate authenticated
  requests use the admission contract's idempotent name/UID rules and add no allocation.
  A stolen trusted API-server private key is not modeled as an unauthenticated caller.
- `TestAdmissionTransportBounds`: hold raw TLS, slow headers/body and idle sockets at32/+1,
  handshake4/+1 and worker8/+1; exceed header count/bytes, JSON token/depth/body limits;
  offer h2/h2c/Upgrade, hostile certificate messages and blocked writes. Assert finite close,
  bounded buffers/goroutines, all permits returned after cancel/timeouts, and successful next
  authenticated API request. Record actual HTTP1 negotiation against the pinned API server.
  Missing/mismatched plugin credentials or any invalid/expired trust reload must fail closed;
  rotation closes old peers and never permits unauthenticated health probes.
- ConfigMap/Event Forbidden, quota/size failure, API timeout and lost status/slot update reply:
  deny or retain occupancy; no success-as-empty/default healthy. Redaction fixtures seed PEM,
  token and control-character sentinels and assert none reach logs/status/Event notes.
- Bound admission body, concurrent requests, worker queue, state/enrollment count, per-object
  bytes, diagnostics, Events and collector allocations at the stated boundary and+1.
  Metadata uses stock gRPC with supported receive/header limits and four application calls,
  as [reader](reader-contract.md) specifies. Its64MiB budget covers application credits and
  owned views, not every stock transport/cache reference. There is no custom response pool,
  padding/frame filter, static-window allocation proof, dependency fork or cleanup receipt.
  [Resource lifecycle](resource-lifecycle-contract.md) contains residual stock allocations with
  verified256MiB memory.max/swap0 and Quiescing→exit75 on metadata error/cancel/decode/budget
  failure. Supervision restarts a fresh process; durable CAS/enrollment state survives.
  No active root is force-freed and no process-death/GC/ConnEnd claim substitutes for the
  actual evidence. Recovery after four hostile calls may cross supervised restarts. This
  Proposed held-drain/restart cost has not been accepted by the user.
