# #67 qualification contract — proposed review corrections

Proposed and unapproved, 2026-11-20. Supplements the [design](design.md); the historical
2026-11-13 accepted server/transport/privilege approach remains unchanged. All checks below
are future qualification requirements, not executed evidence or a chosen release/configuration.

## Host and client profile (R1-C1, R1-C2)

Before #83 lands, an authorized old-baseline VM run may test gateway behavior, explicitly
recording its source/kernel/profile and not certifying #83. After #83 lands, up/test first
requires its actual physical-boottime-v1 admission on storage nodes; current lab VMs refuse
before resource creation/publication. Successful two-workload-node qualification then uses
admitted physical hosts, one also running the node-pinned gateway. No force/VM override.
#84 evidence separately covers the actual supported physical/kernel profile and its minimum
kernel; older VM checks are not substituted. No clear dependency plan is changed.

The operator installs the host NFS client before up/test: Ubuntu `nfs-common` (or a documented
supported equivalent), `mount.nfs` in the actual kubelet host mount environment, and kernel
NFSv4.1 client support. Record package/helper/kernel/module versions and actual helper path.
Preflight rejects missing/nonexecutable helper, missing built-in or already-loaded client support,
unknown checks and unsupported mount options before creating gateway objects. It never installs
packages or loads modules silently; operator module setup precedes this preflight, and module
availability alone is not proof the client is active. Qualification performs an actual hard NFSv4.1 mount through
each host's loopback proxy and records the negotiated version/options. Fake checks cover absent
helper/client support and both physical/VM admission outcomes.

## Stable reply ordering (R1-C3)

For the pinned release, identify every VFS boundary used to make FILE_SYNC/DATA_SYNC WRITE
and COMMIT stable (fsync/fdatasync or an explicitly proved synchronous write path). A controlled
qualification build gates those exact calls and correlates request IDs, sync completion/result
and decoded NFS replies; the production pinned source/config is checked against that boundary.
For each stable WRITE class and COMMIT, hold the required sync and assert no stable-success
reply; release successfully and observe the permitted stable result. Inject sync failure and
assert no stable-success reply. The fixture must fail a negative candidate that replies before
the gate or ignores its error. Do not infer this ordering from process death, page-cache survival
or #84's downstream block tests. Retain SIGKILL, checksum, verifier/grace and reclaim tests.
Qualification instrumentation stays outside production; no new replica rule or node-loss test.

## Resource containment (SEC67-R1-02)

The release-specific qualification artifact must select finite numeric limits for each row,
name the actual supported configuration/control and record its effective read-back:

| Owner | Required finite budgets |
|---|---|
| Ganesha container | CPU quota, hard memory limit, pids/tasks, file descriptors, RPC connections and workers, outstanding RPC/reply-cache entries and send/receive buffers |
| Pod TLS tunnel | CPU, hard memory, pids/fds, accepted connections, concurrent handshakes, handshake/authorization deadline and per-connection buffered bytes |
| Each host node proxy and owned mount gate | Separate per-export service cgroup CPU/memory/tasks and fd limits, connection/handshake counts/deadline and per-connection buffered bytes; finite helper/probe concurrency and bounded invocation records |

Container requests/limits alone do not establish fd/pid limits: qualify the runtime/launcher
settings too. Host-proxy limits are per export, not the unrestricted kubelet/data-plane service.
Refuse exposure when any required limit is absent, unlimited, unsupported or not observable.
Use ordinary cgroup/runtime/service limits and supported Ganesha/tunnel settings, with bounded
logs; no claim that a configured memory ceiling is an exact process-RSS proof.

Overload refuses/closes new work without plaintext or unstable-as-stable fallback. Existing
hard NFS clients may pause/retry; bounded authorization/handshake deadlines still apply. Memory
containment may kill an owned server/proxy; restart uses the same persisted identity, normal
grace/reclaim and stable-write rules, and cannot overlap the predecessor. No resource-failure
path deletes recovery/base data or restarts unrelated node services. Qualify slow/failed TLS
handshakes and concurrent open/lock/RPC pressure, observe each budget, and verify owned-process
cleanup plus stable-data/reclaim recovery after containment kills. If any limit/restart behavior
fails, no export is exposed; exact version-specific values remain in the accepted qualification
approach, not invented here as measured or human-accepted performance/availability results.
