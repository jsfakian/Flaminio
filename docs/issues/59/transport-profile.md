# Issue #59: object-only transport profile

[design](design.md) · [policy and publication](policy-contracts.md) · [decisions](decisions.md)

**Proposed R1/R2 revisions; unapproved, not implemented or qualified.** This appendix supplies
mechanisms for R1-C1/C2/C3/C5 and the R2-C1/C2 corrections. It does not alter the historical
approval or accepted ADR files.
The authoritative rate boundary is the owned Linux egress; arrival captures beyond it are
path diagnostics. This is an explicit proposed adjustment of S2's remote-observer wording.

## Isolation and ownership

The #52 Rust worker stays in the host network namespace. Its metadata and snapshot-reader
clients use their existing host connectors, including while object transfer is paused. Only
its `ObjectStore` HTTP connector obtains sockets from a per-run AF_UNIX socket broker inside
an owned network namespace. Socket descriptors retain that namespace when passed back to the
worker with SCM_RIGHTS; TLS, signing and object credentials remain in the unprivileged worker.
No worker thread calls setns; no source reader or metadata client uses this broker.

#59 supplies its own bounded root setup/monitor entry point, `flaminio-backup-net`; #74's
ublk/disk helper supplies no networking operation. The setup parent creates and configures
the broker namespace, veth, endpoint-only routes, packet filter and shaper as described below.
Its #59-owned Go broker child runs as the dedicated `flaminio-backup-net` UID there. The worker
remains `flaminio-backup` with no CAP_NET_ADMIN, CAP_SYS_ADMIN or CAP_NET_RAW.
The root monitor retains only the ability to manage its owned network objects. No helper
receives object credentials, repository keys, certificate private keys or arbitrary commands.

**Setup contract.** Root invokes `flaminio-backup-net serve --run UUID --endpoint PROFILE
--worker-pid PID --policy-file FILE`. `PROFILE` selects a root-owned, non-writable regular
file below `/etc/flaminio/backup-net/`; it contains one HTTPS authority, fixed region, 1..8
numeric IPv4 endpoints sharing one TCP port, an administrator-reserved /30 and an existing
host route. No URL credentials, proxy, DNS search, arbitrary interface, executable, UID,
route prefix or shell fragment is accepted. Files are opened no-follow; the policy file is
<=64 KiB, strictly decoded, and must match the worker's persisted policy identity. UUIDs use
canonical lower-case hexadecimal, endpoint labels `[a-z0-9-]{1,32}`, and PIDs are positive
u32 values checked through a pidfd, not later trusted by number. At most four run namespaces
exist on one host. A root-owned lock/registry makes concurrent setup and removal exclusive.

Names derive from the run's 32 hexadecimal digits: namespace `/run/netns/fbw-<32hex>`,
host veth `fw<first10hex>`, namespace peer `eth0`. A colliding name fails; it never adopts or
replaces an interface. Links have MTU1500 and one TX/RX queue; the namespace has no default
route, only /32 endpoint routes via its veth gateway. Both sides have permanent neighbor
entries and ARP disabled. IPv6, forwarding inside the namespace and TCP keepalive are off.
The host route/forwarding policy must already permit this reserved source; setup does not
change global forwarding sysctls or shared wg0 qdiscs. A profile needing SNAT installs only
an owned nftables table with rules matching its exact namespace source and endpoint tuples.
It refuses a host firewall that rejects that path; it does not relax another owner's rules.

The Go monitor starts its own fixed broker subcommand with exec.Cmd.SysProcAttr set to syscall.SysProcAttr,
Cloneflags=CLONE_NEWNET, a fixed unprivileged Credential (UID/GID, empty supplementary groups),
no ambient capabilities and a preopened AF_UNIX SOCK_SEQPACKET control FD. These are fields of the fixed launch profile,
not caller-supplied Go code. The Go runtime thus
starts entirely in the child network namespace without worker setns or per-thread namespace
migration. The monitor holds its pidfd and bind-mounts that child namespace at the owned path,
configures it while the broker waits for READY, then verifies its credentials/capability state
before opening the gate. The broker sets no_new_privs and applies its syscall filter with
SECCOMP_FILTER_FLAG_TSYNC. It can create only AF_INET/SOCK_STREAM/TCP and use the
preopened control FD; a syscall filter denies exec, namespace, mount, raw socket and netlink
operations. Its socket-creation request is `OPEN(version=1,generation:u64,sequence:u64,attempt_id:u64)`
with an exact fixed-size binary encoding and no ancillary FDs. It selects only a configured
endpoint, creates a nonblocking CLOEXEC socket, starts connect and passes exactly one FD with
SCM_RIGHTS. It retains one duplicate solely for TCP_INFO sampling and CLOSE/shutdown; it
never reads/writes socket data. The worker polls connect completion/SO_ERROR.
Malformed/truncated messages or unexpected ancillary data close the control channel. The
monitor authenticates the connected worker's SO_PEERCRED UID and PID against its pidfd and
run generation; sequence numbers are strictly increasing. At most K<=4 sockets/OPENs exist;
there is no generic TCP proxy, inherited listener, destination supplied by a request, or
connection reuse across runs. A worker-side attempt guard counts a live response stream too.
[Linux's UNIX socket API defines descriptor passing and peer credentials.](https://man7.org/linux/man-pages/man7/unix.7.html)


**Safe Rust API route.** Use the workspace's already-locked rustix1.1.5 (`net`) safe
`recvmsg`/RecvAncillaryBuffer with RecvFlags::CMSG_CLOEXEC. Its
RecvAncillaryMessage::ScmRights iterator yields OwnedFd directly; require exactly one,
reject MSG_TRUNC/MSG_CTRUNC and drop all unexpected/extra descriptors. Convert using safe
`std::net::TcpStream::from(OwnedFd)`, then Tokio TcpStream::from_std after nonblocking validation.
Clone the std stream with try_clone (or rustix::io::dup on its borrowed FD); shutdown uses the safe socket API. TCP_INFO
uses the Go broker path below because no Rust raw-pointer getsockopt wrapper is introduced.
No new unsafe block, FromRawFd conversion or flaminio-uring exception is needed. The inspected
[official rustix1.1.5 source](https://static.crates.io/crates/rustix/rustix-1.1.5.crate)
(`src/net/send_recv/msg.rs`, MSRV1.65) exposes this exact OwnedFd route;
[Go x/sys v0.49.0](https://cs.opensource.google/go/x/sys/+/v0.49.0:unix/syscall_linux.go)
provides the typed TCP_INFO call. The Go helper's fixed child launch, credential/drop and
netlink setup use existing Go/Linux APIs; no arbitrary caller-supplied command is executable.

## Shaper, packet gate, accounting and failure

`wire-v1` supports Linux >=6.6, IPv4, `1024<=R<=1000000000` bytes/s and requested
`4096<=B<=1048576` bytes. Its exact kernel build, iproute2/nftables versions, kernel HZ,
profile configuration and feature readbacks are recorded in qualification. Lower rates are
valid only in explicitly selected `payload-v1`; requesting wire-v1 at 1 byte/s fails before
any store connection, `wire_rate_below_minimum` (usage exit2). Native SYN retransmission,
TLS peer deadlines and ACK pacing cannot be suspended by a userspace eligibility clock.
Even supported rates do not promise liveness against every backend's request deadlines.

The namespace `eth0` owns a root TBF (handle1:) with rate `r=floor(99*R/100)` bytes/s,
no peakrate, default bfifo child and `limit=Q=16384` bytes. No child replacement or hardware
offload is permitted. Both veth ends disable and read back TSO/GSO/GRO/LRO, tx-gso-partial,
UDP segmentation and hardware TC offload; fixed unsupported-off features are acceptable,
any enabled relevant feature refuses wire-v1. No packet can exceed L=1514 charged bytes
(1500 IP +14 Ethernet), and oversized/GSO packets fail the profile. Every TCP packet,
including SYN, ACK, retransmit, TLS and request bytes, takes this qdisc. There is no separate
unlimited handshake/control lane. ARP is absent; the claim excludes Ethernet, physical NIC,
underlay and WireGuard headers. Charging Ethernet bytes conservatively bounds IP bytes.

The helper uses rtnetlink TCA_TBF_BURST with
`b=floor((B-L)*r/R)` and the explicit byte limit Q. To remove rounding ambiguity it reproduces
the pinned kernel's `psched_ratecfg_precompute`/`psched_l2t_ns`, recording its multiplier,
shift and time bucket `T=cost(b)`. Before opening it checks each possible frame length
54..1514: `cost(frame)>=ceil((frame-14)*1000000000/R)`, and checks
`ceil(R*T/1000000000)+L<=B`; overflow or failure refuses the profile. This is a finite integer
calculation, not a future choice of qdisc. The 1% rate margin and L reserve allow conservative
integer rounding. The invariant follows from the TBF time-token bucket: in any interval dt,
summed IP lengths <=R*dt+B. Queued socket/TLS/bfifo bytes precede this boundary and must earn
tokens on dequeue; they do not enlarge its burst. TBF initializes with b credit, a bounded
initial wire burst; the separate application bucket starts at zero.
[Linux6.6 TBF supplies the dequeue/time-token mechanism and byte FIFO](https://raw.githubusercontent.com/torvalds/linux/v6.6/net/sched/sch_tbf.c);
[its rate conversion is defined in the scheduler API](https://raw.githubusercontent.com/torvalds/linux/v6.6/net/sched/sch_generic.c).

**Boundary.** Authoritative events are successful dequeue accounting on this owned TBF,
using the kernel `t_c` monotonic timestamp and parsed IPv4 total_length. Qualification's
independent root observer uses a BTF-matched fexit probe of `tbf_dequeue` to read that timestamp
and returned skb, filtering exact netns/ifindex/handle. It counts every returned packet once,
including a retransmission; its ring-loss counter must stay zero. A pcap observer independently
captures PACKET_OUTGOING at namespace eth0, after scheduling, and captures at the store.
Those latter observations corroborate traffic identity and downstream delivery; neither's
batching/scheduling timestamp substitutes for the authoritative dequeue timestamp. This is
an egress-scheduler envelope, not a physical-link or remote-arrival envelope. A downstream
queue can batch local departures; absent a separately qualified path-jitter bound J and
remote burst `B_remote=B+R*J`, no remote arbitrary-interval claim is made.

**Gate.** An owned namespace nft `inet` table has output/input default-drop chains, no
established-flow bypass and no packet-path set refresh. Output permits only TCP endpoint
tuples in a timed set; input permits their reverse TCP traffic and the narrowly related
PMTU feedback below only while the same set member is live. The root monitor renews members
every500ms, with a maximum lease2s and expiry no
later than the next monotonic window boundary projected from the shared UTC policy. It
rounds expiry down to the kernel tick and never opens a sub-tick interval. It computes the
window predicate itself from the same persisted rules; the worker cannot send arbitrary
OPEN_GATE. A worker CLOSE, window observation or invalid clock closes earlier. nft changes
use one netlink transaction; helper/worker EOF never renews. Kernel expiry checks stop new
packets entering the qdisc even if userspace dies; delayed garbage collection does not renew
membership. [nftables defines timed sets and element replacement](https://wiki.nftables.org/wiki-nftables/index.php/Element_timeouts).

**PMTU feedback (R2-C1).** Keep Linux TCP Path MTU Discovery enabled: the broker sets
IP_MTU_DISCOVER=IP_PMTUDISC_DO on every object socket. Inside this owned namespace only, the
helper sets/readbacks ip_no_pmtu_disc=3 (hardened TCP sequence-validated PMTU) and conntrack
checksum verification on. This does not change host/other namespace sysctls. After conntrack
lookup, the namespace input filter adds exactly one ICMP exception: ingress eth0, outer IPv4
destination equal to this namespace address, ICMP type3/code4 (fragmentation needed),
`ct state related`, `ct original protocol tcp`, original source equal to the namespace address,
and original destination IP/TCP port present in the same unexpired endpoint lease set.
For example, its final tuple predicate is
`ct original ip daddr . ct original proto-dst @open_endpoints`; there is no general RELATED
accept. Conntrack must parse a complete quoted outbound IPv4/TCP tuple; malformed/truncated
quotes and bad outer IPv4/ICMP checksums are rejected. Router sources need not equal the store IP.
Linux then looks up the quoted live TCP socket and checks its quoted sequence against the
outstanding send range before lowering PMTU/MSS and retransmitting. Thus a forged tuple or
out-of-window sequence cannot change another socket's PMTU; this is validation of ICMP, not
cryptographic router authentication. Echo, redirect, unrelated errors and all ICMP when the
endpoint lease is closed remain dropped. NAT profiles require the existing host forward path
to deliver related ICMP with its quote reverse-translated to the namespace's original tuple;
otherwise setup/qualification refuses that path. Every resulting TCP retransmission still
uses the owned TBF, contributing fully to outbound IP accounting; ICMP is inbound diagnostic
traffic and no outbound control exemption is added.
[Linux6.6 TCP performs the socket/sequence check and PMTU/MSS retransmission](https://raw.githubusercontent.com/torvalds/linux/v6.6/net/ipv4/tcp_ipv4.c);
[the namespace hardened PMTU setting](https://www.kernel.org/doc/html/v6.6/networking/ip-sysctl.html)
and [nft ICMP/conntrack tuple fields](https://www.netfilter.org/projects/nftables/manpage.html)
are the selected APIs. Qualification must include a routed MTU1280 hop, not only the
co-located store path; a path suppressing necessary ICMP has no promised minimum service.

At observed close the worker stops admission, revokes permits and aborts all attempt guards.
The monitor clears the timed set, replaces root TBF with `blackhole` (purging its old FIFO),
and acknowledges only after netlink completion; link-down/delete is the fallback on error.
The application waits for all driver/body guards to drain. After an observed close, even if
the monitor is descheduled/killed, the conservative outbound tail is
`ceil(R*(2s+1/HZ))+B+Q+L` IP bytes: at most one remaining lease of conforming new traffic plus
already queued FIFO/one driver packet. From acknowledged qdisc closure, no new scheduler
dequeue occurs; events already accounted remain downstream. No finite wall-time tail or
exact UTC-nanosecond cut is promised by a non-real-time host. Qualification reports observed
latency separately; a requested stricter contract refuses rather than relabeling a result.

Reopen creates a fresh TBF only after at least `ceil(B/r)` seconds since acknowledged old
qdisc deletion, with the timed set still empty; new application credit is zero. This spacing
also applies to helper restart/policy revision, so repeated close/reopen cannot mint repeated
wire bursts faster than the envelope. There is no credit proportional to the closed duration.
A new monitor generation cannot inherit old sockets or authorization. The waiting interval
is explicit local wait and still requires renewed publication/retention admission afterward.

**Resources and lifetime.** Per socket set/read back SO_SNDBUF8192 (Linux reports <=16384),
SO_RCVBUF32768 (<=65536), TCP_NOTSENT_LOWAT1024 and SO_KEEPALIVE0; TCP_INFO is sampled by the Go broker every
100ms. A lower kernel-adjusted value is accepted, a larger value refuses. HTTP write/read
buffers are each <=16384; TLS application buffers <=16384 each, receive record <=18432,
body frames <=1024 bytes, request/response headers <=16384 bytes and <=100 fields, handshake
messages/certificate chain <=65536 bytes. Header/handshake limit violations are terminal for
that endpoint profile. These are transport buffers in addition to #52's K encoded object
buffers, not a claim that the socket buffer equals wire bytes. Unexpected qdisc/offload/routing
changes or monitor-channel loss stop the run with `wire_profile_lost`, retaining pins/checkpoint.

The root monitor polls its worker pidfd/control EOF and broker pidfd. Any death closes the
gate, takes links down, kills the broker and removes only registry-recorded nft tables,
veth, namespace bind mount and private runtime directory. Partial setup unwinds in reverse,
never opening the gate; systemd `ExecStopPost` runs the same idempotent cleanup after SIGKILL.
The timed permit also expires without systemd. Namespace sockets may keep a deleted namespace
alive, so cleanup proves link removal, not merely removal of `/run/netns`; all worker sockets
are cancelled and the pidfd owner must exit before a run is recreated. Startup removes stale
objects only with registry ownership/inode/ifindex checks, otherwise refuses. It never changes
source grants/pins, shared interfaces, store data, host firewall defaults or retained archives.

## SDK and HTTP implementation

Use `aws-sdk-s3 = =1.95.0`, default-features=false, features `rt-tokio,http-1x`; install a custom
`RunHttpClient` through Config::builder().http_client, not the SDK default HTTPS client. That
published crate declares Rust1.85; the workspace requires1.88. Its published Cargo.lock is
the compatibility starting graph: exact production pins are credential-types1.2.3,
aws-runtime1.5.8, sigv4 1.3.3, smithy-async1.2.5, checksums0.63.4, eventstream0.60.9,
smithy-http0.62.1, json0.61.4, runtime1.8.3, runtime-api1.8.1, types1.3.2, xml0.60.10,
aws-types1.3.7 and smithy-observability0.1.3. Enable runtime-api `client,http-1x` and types
`http-body-1-x`; pin hyper1.6.0 (`client,http1` only), hyper-util0.1.14 (`tokio`),
http-body1.0.1, http-body-util0.1.3, rustls0.23.27 and tokio-rustls0.26.2. Preserve a reviewed
Cargo.lock including transitive MSRVs; a semver range is insufficient because later Smithy
releases may require newer Rust. Tokio1.45.1 in that release graph is compatible with1.88;
the implementation may unify the existing workspace Tokio only after its MSRV/API checks.
These are reproducible design pins, not a claim a new SDK build has already passed.
[Official1.95.0 package](https://static.crates.io/crates/aws-sdk-s3/aws-sdk-s3-1.95.0.crate)
(SHA256 `a316e3c4c38837084dfbf87c0fc6ea016b3dc3e1f867d9d7f5eddfe47e5cae37`)
contains the manifest and release Cargo.lock inspected for this revision.

`RunHttpClient` implements Smithy HttpClient and returns SharedHttpConnector wrapping
`RunConnector: HttpConnector`; `call(HttpRequest)->HttpConnectorFuture` owns the asynchronous
attempt. It rejects nonempty SDK connect/read settings, because this profile explicitly sets
TimeoutConfig::disabled(), and uses its phase clocks below. Configure
BehaviorVersion::v2025_01_17(), RetryConfig::disabled() (max_attempts=1), and
StalledStreamProtectionConfig::disabled(); no total operation/attempt timeout or SDK stream-rate
heuristic may race the policy. Retry/backoff belongs to the outer ObjectStore adapter. These
APIs are present in the inspected pinned sources, not inferred from a hypothetical interceptor.
[Official runtime-api1.8.1](https://static.crates.io/crates/aws-smithy-runtime-api/aws-smithy-runtime-api-1.8.1.crate)
defines HttpClient/HttpConnector, HTTP1 conversion and the behavior version;
[official types1.3.2](https://static.crates.io/crates/aws-smithy-types/aws-smithy-types-1.3.2.crate)
defines body conversions/retry/timeout disabling. AWS documents why ordinary
[timeouts/stalled-stream protection](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/timeouts.html)
need deliberate handling for a custom transport.

The adapter performs policy/window/retention waits before constructing each SDK operation and
signing it. Its operation-specific client clone embeds immutable AttemptContext (run, policy
revision, object kind, cancellation, retention ticket). Inside every connector call, convert
with `HttpRequest::try_into_http1x()` and invoke the #52 `before_attempt` final nonblocking
admission check before OPEN or any reuse. If it says wait/stale, return a typed LocalDeferred
without opening a socket; the adapter waits, rebuilds/re-signs the operation and checks again.
A request signed before an hours-long closed window is never sent after that wait. Unexpected
multiple calls/attempts fail the conformance fixture. Signing itself has no network side effect.

Each admitted attempt uses one broker FD, rustls TLS1.3 with #52's CA/name/SPIFFE and client
identity verification, then `hyper::client::conn::http1::handshake` on TokioIo. No HTTP2,
pipelining, proxy, Upgrade, automatic redirect, TCP keepalive or idle pooled connection exists.
Send `Connection: close`; maximum pool size equals active attempts K, idle pool size zero.
This deliberately pays handshake bytes for predictable cancellation. The per-attempt driver
JoinHandle is retained through response-body EOF and aborted/joined on cancellation. A guard
also owns a duplicate socket FD for shutdown(SHUT_RDWR); dropping the SDK future alone is not
considered cancellation. Window close closes every guard, including a response returned as a
stream and a connection whose request future was dropped. A guard's Drop cancels its token;
a bounded run-level drain joins tasks before declaring suspended. Any nonconforming lingering
socket tears down the namespace instead of returning it to a pool.

Wrap each request SdkBody as `PacedBody<SdkBody>: http_body::Body<Data=Bytes>`: split frames to
<=1024 bytes, await/debit the shared payload bucket before poll_frame returns each data frame,
forward trailers and exact size_hint unchanged, and cap trailer size4096. It changes no signed
bytes. Preserve the immutable encoded object for the worker's explicit retry, never make a
hidden whole-body copy. Use `SdkBody::from_body_1_x` for the bounded response-body wrapper,
which owns the guard and progress clock until EOF. Hyper HTTP1 has explicit
[max_buf_size/max_headers settings and a connection driver](https://docs.rs/hyper/1.6.0/hyper/client/conn/http1/struct.Builder.html).
TLS outgoing data uses `set_buffer_limit(Some(16384))`; its application buffer limit alone does
not bound handshake messages, hence the independent65536 handshake parser/count limit above.
[Rustls0.23.27 provides that buffer API](https://docs.rs/rustls/0.23.27/rustls/struct.ClientConnection.html).

Construct Config directly with static region/HTTPS endpoint, force_path_style(true),
disable_multi_region_access_points(true), disable_s3_express_session_auth(true),
use_arn_region(false), request_checksum_calculation(WhenRequired), and a direct static
Credentials provider. No aws-config/default environment chain, IMDS, STS, SSO, external
credential_process, automatic refresh, accelerate or access-point ARN is enabled. Only
nonexpiring administrator-provided S3 credentials are supported in this profile; expiry or
rotation requires an explicit configuration revision/restart. KMS/source/metadata calls use
their own host clients and are explicitly excluded from the object destination counter.
Each call validates scheme, authority, IP allowlist, path prefix and bounded encoded length;
3xx/region redirect is a diagnosed endpoint error, never followed. Endpoint resolution occurs
at setup from the administrator profile, with no connector DNS/rebinding fallback. All HTTP
methods, list pages, GET verification, HEAD/lock controls and failed attempts use this same
connector/shaper; introducing multipart later cannot select another connector.

## Phase clocks: progress, backpressure and shaping

Only an explicit `LocalWait` entered by the body budget or closed/reopen gate removes time
from a monotonic phase clock. The event contains reason, start/end and policy revision; it
cannot be entered because poll_write is Pending, the socket is nonwritable, remote window is
zero, no ACK arrives, or a peer is slow. Local source preparation occurs before admission;
an unexpected empty producer is a local-source failure, not a remote-wait exemption.

**wire-v1 clocks.** Kernel shaping gets a finite allowance, never an indefinitely stopped
clock. With FIFO bound Q, maximum TLS write batch W=18432, define `A=ceil((Q+W)*1000000000/r)+scheduler_allowance_ns`.
The fixed v1 scheduler_allowance_ns is1000000000 (1s), recorded in the root-owned
profile; qualification verifies this selected bound rather than choosing a mechanism. A
miss is `local_scheduler_unqualified`, aborting the attempt while preserving pins, not an
extra exemption. Reset A only on an actual phase progress event below. Remote backpressure
still consumes all eligible elapsed and eventually reaches the threshold. With no kernel
qualification the policy may still run payload-v1 but claims no wire-v1 service/timing result.

**payload-v1 clocks (R2-C2).** Set A=0: this profile has no TBF r, FIFO Q or wire scheduler
allowance, regardless of whether an optional payload cap Rp is present. The same phase events
and base30s/60s/120s bounds below apply. Its bucket refills/debits in encoded HTTP body bytes at
the actual Rp>=1 when configured; PacedBody emits <=1024-byte frames, allowing smaller frames
when its current credit is smaller, so a1byte/s cap does not wait for an artificial1024-byte
quantum. A body-permit LocalWait ends as soon as bytes are permitted or the request/window is
cancelled; it is never entered for nonwritability/remote zero-window backpressure. With no cap,
body permits are immediate and no pacing LocalWait exists. TCP/TLS/header progress and response
ACKs are outside this payload bucket; they use the base clocks, without division by Rp or a
wire qualification dependency. Window-only admission still checks/cancels at the observed
window boundary; it does not claim the external packet-gate tail. The pinned Linux connector
uses the same typed TCP_INFO/progress API: the worker launches the fixed Go `broker --host`
child as its own UID with a preopened UNIX control FD, the bounded nonsecret endpoint
configuration and no namespace/credential/capability change. No root namespace/shaper setup
is requested for this weaker profile. All bounded body/socket/driver guards and cancellation
rules remain. The profile selector is embedded in
the connector context so no wire-only qualification failure or cost calculation is borrowed.

| Phase | Progress that resets its idle clock | Idle bound after explicit local waits |
|---|---|---|
| TCP connect | SO_ERROR=0 and TCP_INFO ESTABLISHED completes phase |30s+A; socket writability alone never resets |
| TLS handshake | successfully processed peer handshake bytes/state advance, bounded by65536 bytes |60s+A; no total upload-age limit |
| Upload/drain | strict increase in TCP_INFO tcpi_bytes_acked after the application's TLS-data baseline |120s+A; socket writes/body polls/duplicate ACKs/probes never reset |
| Response headers | new HTTP header octets accepted by the strict bounded parser, total<=16384; complete headers ends phase |60s+A, starts after final request data is ACKed; early error headers are read concurrently |
| Response body/verification GET | positive authenticated TLS application bytes consumed by HTTP body, within declared response length |120s+A; a full local consumer queue is an explicit bounded local wait |

The Go broker uses the repository's existing golang.org/x/sys/unix v0.49.0
`GetsockoptTCPInfo(fd,IPPROTO_TCP,TCP_INFO)` on its duplicate every100ms and a sequenced SAMPLE
request at a phase transition. Its bounded <=128-byte METRICS response carries generation,
attempt ID, sample CLOCK_MONOTONIC ns, Bytes_acked, Notsent_bytes and State. The Rust worker
validates identity/sequence, nondecreasing counters/time and freshness<=500ms; stale/lost
metrics fail `wire_profile_lost`. Linux>=6.6 supplies the required fields. Broker CLOSE shuts
down/closes that duplicate and acknowledges before the attempt slot is reusable. tcpi_bytes_acked reports
TCP transport progress only, never durable object acceptance; #52's authenticated exact-version
GET/receipt remains required. `tcpi_notsent_bytes`/SIOCOUTQ are diagnostic queue counters,
not reasons to freeze the clock. [Linux6.6's TCP_INFO ABI names these fields](https://raw.githubusercontent.com/torvalds/linux/v6.6/include/uapi/linux/tcp.h).
A full local response-consumer queue enters explicit LocalWait for at most5s, then fails
`local_consumer_stalled`; it cannot provide an infinite exemption.
The response clock does not start at SDK invocation while a long body is intentionally paced.
For GET/list/HEAD it starts after the short request is ACKed. A headerless peer that stops
ACKing is an upload/drain stall; one that ACKs/consumes everything and never responds is a
header stall. Slow positive progress can keep a request alive for days, subject to backend
limits. Gate closure cancels the attempt instead of retaining a clock/socket into reopening.

For #53's publication admission, `local_cost(profile,n,c)` returns checked u128 nanoseconds,
where n is the encoded request-body byte count and c is currently available body credit
reserved exclusively for that operation (0<=c<=min(n,bucket_capacity) when a bucket exists;
c=0 with no cap); no credit is reserved across a window close, policy revision or pre-dispatch
wait.

- wire-v1: `ceil((Q+wire_budget(n))*1000000000/r)+scheduler_allowance_ns`, independent of c,
  with `wire_budget(n)=2*(n+16384)+65536` IP bytes for the isolated no-loss request/handshake.
- payload-v1 with Some(Rp>=1): `ceil(max(n-c,0)*1000000000/Rp)` using the actual body bucket.
  Headers, TCP/TLS and ACK overhead are excluded because this policy paces body bytes only.
- payload-v1 with no cap:0 known local pacing ns, including a window-only policy. It never
  divides by an absent rate or substitutes the wire profile's r/Q/scheduler allowance.

The drained root finishing slot uses actual reserved c, or explicitly conservative c=0 for
preflight estimates; it never labels a c=0 estimate unavoidable infeasibility when available
credit could fit. `root_local_emit_seconds=ceil(local_cost(profile,n,c)/1000000000)` feeds the
existing `max(2u,root_local_emit_seconds)` dispatch check. Even a zero local pacing cost still
requires a currently open window, unchanged policy generation and the bounded fresh Date-to-
dispatch check in [policy-contracts](policy-contracts.md). Closing before dispatch cancels the
slot and re-admits; closing during the body cancels/resolves that request as already specified.
These are profile-specific local-work estimates/reservations, not bounds on kernel scheduling,
network loss, inbound responses, server latency or successful completion. No cap or a rate
*upper* cap establishes a minimum network throughput. #53's late-root outcome survives overrun.

## Conformance fixtures and future qualification

Implementation paths are the #52 `flaminio-backup` transport modules (`smithy`, `linux`,
`progress`) and #59's Go `control/backupnet`/`control/cmd/flaminio-backup-net` helper; script entry points remain the design's named
WAN scripts. No runtime implementation, setup, shared VM change or network measurement ran
while writing this appendix. Build/API fixtures must use the exact Rust1.88 locked graph.
Qualification may record measurements later; it may not replace any selected mechanism.

| Finding/rule | Required failing counterexample / check |
|---|---|
| R2-C1/PMTU | separate routed store namespace beyond a1500->1280-MTU hop: full TLS records and4MiB PUT/GET progress after type3/code4 feedback, MSS shrinks and retransmitted IP bytes remain charged; repeat with owned SNAT quote translation; no shared VM mutation |
| R2-C1/security | forged/wrong quoted source/destination/ports, truncated quote, bad checksum, out-of-range TCP sequence and ICMP redirect leave live socket PMTU unchanged; a valid hint after endpoint lease expiry/close is dropped and does not reopen the gate |
| R2-C2/payload | Rp1,n1024,c0 yields1024s and a300s root budget refuses; c1024 gives0 pacing cost but still enforces2u/freshness/open window; no-cap window-only yields0 without dividing by R; close before dispatch invalidates either finishing slot |
| R2-C2/clocks | payload1byte/s LocalWait excludes only actual bucket wait; full send buffer/peer never reads reaches120s with A0; window-only no-cap never enters pacing wait, never needs wire qualification and retains header/response stall limits |
| C1/S2 | exhaustive frame/rate rounding vectors at R1024/1e9 and B4096/65536/1MiB, offload readback/GSO rejection, every IP control/retry counted, independent dequeue trace; payload-only shaper fails |
| C1/S3 | timed permit expires after monitor SIGKILL; broker/worker kill, full FIFO at close, reset burst reopen spacing, partial setup at every step, cleanup twice, live FD after netns-name deletion |
| C1/security | forged UID/PID/generation/sequence, stale pidfd, arbitrary endpoint/FD injection, symlink/profile/name collision, fifth run/fifth socket; no host reader/meta traffic in trace and it continues when gate closes |
| C2/S5 | peer accepts but never reads; fill send buffer/zero remote window, tcpi_bytes_acked plateaus and120s+A expires; infinite Pending never creates LocalWait |
| C2/S5 | peer consumes full body but never responds reaches60s+A; real slow ACK/body progress survives many total timeout periods; qdisc backlog cannot turn into an unlimited idle exclusion |
| C2/low rate | wire1byte/s refuses before SYN; fake1byte/s local-budget clock excludes only named waits; wire1024B/s handshake/header with full Q uses A rather than false30s timeout, backend deadline failure remains diagnosed |
| C3/S1–S6 | locked API compile, each ObjectStore method/control/list page invokes exactly one connector call per explicit attempt, wait then re-sign, expired credentials cannot fetch metadata/STS, redirect never opens another authority |
| C3/cancel | window close/drop future/response stream/driver error leaves zero live drivers/sockets and no idle pool; body bytes/hash/trailers unchanged under1024-byte frames, memory stays within stated bounds |
| C5/S2 | downstream delay/batching emits>R*dt+B remotely while local dequeues pass; report remote diagnostic only; lost observer events/incorrect kernel BTF invalidates a measurement |

A supported kernel/profile still needs actual qualification of local scheduling, backend TLS/
HTTP limits, tail observations and restore correctness before an operator requests its wire
claim. Unsupported platforms/profile changes fail wire-v1 explicitly; they do not fall back
silently. Authorized lab/disposable-physical workflow, store port8334, fixture sizes,
publication deadline and retention-aware cleanup are specified in [policy-contracts](policy-contracts.md).
