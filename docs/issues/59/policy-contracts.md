# Issue #59: policy, retention and qualification contracts

Review-round-1 proposal, unapproved. [Spec](spec.md), [design](design.md),
[transport profile](transport-profile.md). Dependencies stay as reviewed; no source change is
made by this plan. The following sections replace underspecified mechanisms in the old draft.

## Explicit activation (S1, S4)

`flaminio-backup backup --grant FILE --transfer-policy wire-v1|payload-v1` plus existing #52
arguments enables the policy. Any `--upload-*` or `--update-transfer-policy` without that selector
is exit 2 before network/setup. No #59 flags selects legacy #52 behavior: no namespace or policy
state, a store failure exits 1 with grant/checkpoint kept for rerun. Policy retry applies only to
`backup`; other commands retain #52 behavior unless explicitly invoked by this worker's policy.
Payload-v1 labels its weaker byte/window admission guarantee in every status result; wire-v1
requires the exact supported transport profile. Neither silently upgrades/downgrades a profile.

Config fields are profile, rate, burst, sorted normalized local intervals, canonical zone name,
concurrency and the zone-table identity below. Omitted zone without windows canonicalizes to
UTC for identity; a supplied zone without windows still validates. Initial policy revision is 1. Resuming an enabled
checkpoint requires `--transfer-policy` explicitly matching its profile;
otherwise exit 2 `transfer_policy_required`, naming its safe profile/hash, never silently revert
to legacy mode. Before its first admission, an explicit update fsyncs old/new hashes and revision,
closes/drains transport, discards permits/credit and rebuilds the gate. It cannot change source,
destination, retention P, immutable repository or root identity. Reject policy changes after root
publication; reruns then resolve that one root and perform #52/#53 completion only.

## #53-aware admission after waits (S3, S4, S9)

This section applies to #53 immutable repositories; ordinary #52 repositories keep their
existing publication rules without invented D/E or retention obligations. Use #53's authority,
P, D, E=D+P, uncertainty u, COMPLIANCE mode and exact version IDs.
`put_allowance=2u` and at most one re-cover per invocation remain #53's values. Outage/window retries in a living
invocation never reset its re-cover allowance; a process rerun starts #53's rerun check again. Client wall time
and a transfer window never establish retention. Every wait (tokens, gate, backoff, namespace,
connection pool or credentials) invalidates the previous dispatch permit and root-ready status.

1. Reconcile checkpoint/root first under the policy. If a root already exists, authenticate the
   exact version and resolve its response; never manufacture a replacement root. A qualifying
   root proceeds only to #52 completed close. A late root T+u>D exits 1 `late_root_terminal`,
   prints its ArchiveRef and #53's `snapshot-read close GRANT-FILE --abort`/new-grant instruction.
   An ACK loss therefore cannot create a second root or turn a late candidate into a completion.
2. After a limiter/window/backoff wait, reserve one paired admission slot for an exact-descriptor
   time HEAD followed by its pending object/control request, so the HEAD itself does not recurse
   into another freshness wait. A closed gate invalidates the pair; before the first dispatch
   of the pending request it must have a fresh HEAD response, unchanged generation and no local
   admission wait remaining. Normal in-flight byte pacing is not a new request dispatch.
   Refresh qualified store time with that permitted HEAD. Carry store Date S and
   receive monotonic instant m; at the paired dispatch use upper store time
   `S_now=S+u+ceil((mono_now-m)/1s)`, with receive-to-dispatch<=u or discard the slot and
   refresh again. A discarded freshness slot closes a failed resume episode, so repeated local
   scheduling stalls do not hide unbounded requests in a successful episode. An error/stale profile
   is transient only where #53 permits, never proof of a floor. Re-read retention of each exact
   descriptor/page/block version before reuse, extend via #53 cover when short of E and check
   the response's version/mode/floor. A pause resumes this pass at its durable cursor, but every
   version whose floor may now be insufficient is covered again before root readiness. Within an
   uninterrupted invocation, a verified COMPLIANCE floor cannot shorten; revalidate its exact
   cached proof against E/S_now locally after a wait. A process rerun re-reads every version as
   #52 requires. A new E invalidates all cover proofs below that floor.
3. Before choosing/rechoosing D, compute the known graph's remaining cover request count (at
   most 3/version/pass), exact encoded request bodies and profile-specific local pacing cost
   `local_cost(profile,n,c)` below. Walk the pure bucket/window state in request order, consuming
   each reserved credit only once; find an open-window schedule for that work plus `3u`, without
   closed-window credit. For wire-v1 use its shaper cost; payload-v1 uses only its optional body
   rate and credit, and window-only payload has zero known pacing cost. If that local schedule
   cannot fit the descriptor's fixed commit budget, exit 1 `policy_commit_budget_incompatible`, with grant/checkpoint/pins kept and no root PUT.
   Print required known lower bound and configured budget, not a claimed network-time guarantee.
4. For final publication drain all other object requests and reserve an exclusive finishing
   slot for the time-refresh request and bounded root request. Root is <=1048656 encoded bytes.
   Reserve profile-specific local work and an open window spanning that known pacing cost.
   `local_cost(profile,n,c)` returns checked u128 nanoseconds: wire-v1 uses its existing
   `ceil((Q+wire_budget(n))*1e9/r)+1s` (c ignored); payload-v1 with body rate Rp>=1 uses
   `ceil(max(n-c,0)*1e9/Rp)`, with no r/Q/kernel allowance; payload-v1 without a rate returns0
   without division. Here n is exact encoded request-body bytes, c is available body credit
   exclusively reserved at the slot (0<=c<=n); final-root admission uses actual reserved c.
   A conservative c=0 preflight estimate is not unavoidable infeasibility when actual credit fits.
   After acquiring the slot refresh store time, locally recheck the complete exact-version
   cover-proof cursor (all floors >=E), gate generation/window, reserved body credit and root
   reservation. `root_local_emit_seconds=ceil(local_cost(profile,root_encoded_bytes,c)/1e9)`.
   Even a zero cost requires the fresh receive-to-dispatch<=u check, an open current window and
   unchanged generation immediately before dispatch; it grants no minimum WAN throughput or
   server-completion guarantee. Immediately before PUT
   require `S_now+max(2u,root_local_emit_seconds)<=D`; this includes #53's minimum pre-PUT
   allowance. No local limiter/slot wait may remain after that check. A close/revision or any
   intervening await before request dispatch invalidates it and returns to re-admission. Once
   PUT has begun, byte pacing/cancellation is an in-flight request, never a new root admission.
   Runtime scheduling/remote latency can still exceed the allowance; those are not retention
   proofs.
5. If step 4 fails, use #53's one new D/E and full re-cover, checkpointed before extensions.
   The second failure exits 1 `commit_deadline_exhausted` without root PUT. Do not loop through
   fresh deadlines indefinitely. A transmitted root is resolved before any retry; an unknown
   result is queried under the policy. Only T+u<=D and complete retention proofs permit close.

A transient outage can end automatically only when these checks remain satisfiable. A policy
change may make an unrooted checkpoint viable; an immutable repository budget change requires
#53's new repository/graph remedy. A published late root requires abort/new operation. No safety
rule, retention period or one-root restriction is weakened to obtain WAN liveness.

## Resume accounting (S6)

A **resume episode** starts on process rerun or after an interrupted object session and ends at
`ready_for_new_upload` (before the first new block/page/root PUT), terminal refusal, or the next
transient failure/window close. The latter closes a failed episode and increments a persisted
ordinal; the next episode starts fresh. Persist per-episode and aggregate request/byte counters;
there is no finite aggregate bound for repeated outages or unsuccessful episodes.

For a single cut the ambiguous block PUT payload is at most `K*4194384` bytes (K<=4), not 16 MiB
exactly. It excludes TLS/HTTP/IP overhead, page/root PUTs and compulsory GET readback; all those
are counted separately. At most K page/root PUTs can instead be ambiguous, each <=1048656 bytes;
the shared K outstanding requests bound is not additive across the two classes. Recheck complete
versions before reuse; an acknowledged checkpoint receipt never replaces #52's authentication.

Let N be distinct block versions, M page versions, Q root candidates and H descriptor candidates
in the union of the checkpoint/base/operation graphs inspected before ready. Q,H<=8; reject
more as `resume_version_limit` (exit 3). Let V=N+M+Q+H. N is bounded by source logical geometry
and the checked graph; M by nonempty disjoint pages and the <=1 MiB root's explicit page map.
Decode/stream one bounded object at a time; reject overlong/duplicate/range-invalid graphs before
allocation. Deduplicate exact (key,version) visits, never rely on an untrusted count field.

Resolve version listings with max-keys=8, at most 2 pages per unresolved key/authority; stop before
following another page or accepting a ninth candidate. For K ambiguous PUTs plus root/descriptor
resolution, L<=2*(K+2) list requests. One HEAD+one authenticated GET per visited version is <=2V
requests. At most two #53 cover passes cost <=6V; pair each of those <=8V+L actions with one
time HEAD, and allow two final-root/time-refresh requests: <=16V+2L+2 requests before new PUT
in a successful uninterrupted episode. No SDK retries are hidden inside this count; an
interrupted attempt ends the episode. Candidate/response size
limits are protocol refusals, not evictions of required versions or permission to publish.

Required verification GET inbound encoded bytes are at most
`N*4194384+(M+Q+H)*1048656`. Add L*1048576 for bounded list bodies and (7V+2)*65536 for bounded
non-GET control bodies, plus (8V+L+2)*65536 for paired time HEAD response bounds. Headers
are bounded separately by the transport profile and counted in both directions and requests. These conservative control limits apply even to errors; oversized
responses fail before more body allocation. Sum actual outbound/inbound IP bytes and payload,
request count, ambiguous retransmitted PUT bytes, verification GET bytes, page/root bytes and
cover bytes per episode. The wire cap covers outbound IP bytes only; inbound GETs are counted
and graph-bounded, not advertised as rate-limited. Completed object payload is never re-PUT just
to avoid verification cost. A large graph legitimately consumes a window before new data moves.

## Deterministic zone data (S1, S3, S8)

Use one checked-in `contracts/transfer-policy/tzdb/2025b.json` transition table derived from the
[IANA 2025b release](https://data.iana.org/time-zones/tzdb-2025b/NEWS). Generate once using the
same-release tzcode/zic and tzdata. Source archive SHA-256 pins are
`tzdata2025b.tar.gz=11810413345fc7805017e27ea9fa4885fd74cd61b2911711ad038f5d28d71474` and
`tzcode2025b.tar.gz=05f8fedb3525ee70d49c87d3fae78a8a0dbae4fe87aa565c65cda9948ae135ec`
(downloaded from IANA and hashed during drafting). Record generator version and generated
output SHA-256 in the checked-in manifest; that output is not yet implemented/generated.
Bundle the exact artifact into Rust and Go; neither calls
host zoneinfo, localtime, Go's host LoadLocation or an independently versioned Rust zone database.
The table contains canonical zone IDs/aliases, initial offset and sorted UTC-second transitions
for 1970-01-01T00:00:00Z through 2101-01-01T00:00:00Z, with explicit exclusive range end. Generator
expands POSIX future rules into transitions; consumers have no POSIX-rule parser. Outside the
range fail `timezone_range` before admission, never extrapolate host rules. Unknown/malformed
zone or transition refuses. Rust/Go use the same canonical JSON and shared golden UTC vectors.

Persist `{tzdb_release:"2025b",tzdb_table_sha256,policy_schema:1,policy_revision,policy_hash}`.
Hash is SHA-256 of canonical strict UTF-8 JSON (sorted keys/no duplicates/integers only), including
profile/rate/burst/concurrency, canonical zone ID and sorted merged minute intervals plus exact
release/table hash; defaults are expanded. A newer binary must retain the old artifact or refuse
`timezone_rules_mismatch` before network; same label with different bytes also refuses. Explicit
`--update-transfer-policy` acknowledges new rules, records old/new identities and revision, closes
the gate and resets credit/permits. It does not reinterpret checkpoint identity or accepted data.

Golden UTC vectors, all using Europe/Athens under 2025b (half-open endpoints):

| Local window | UTC instant | Local instant | Admit |
|---|---|---|---|
| 03:00-03:30 | 2026-03-29T00:59:59Z | 02:59:59 +02 | false |
| 03:00-03:30 | 2026-03-29T01:00:00Z | 04:00:00 +03 | false: local hour absent |
| 03:00-03:30 | 2026-10-25T00:15:00Z | 03:15:00 +03 | true |
| 03:00-03:30 | 2026-10-25T01:15:00Z | 03:15:00 +02 | true: repeated hour |
| 03:00-03:30 | 2026-10-25T01:30:00Z | 03:30:00 +02 | false |
| 22:00-06:00 | 2026-01-01T19:59:59Z | 21:59:59 +02 | false |
| 22:00-06:00 | 2026-01-01T20:00:00Z | 22:00:00 +02 | true |
| 22:00-06:00 | 2026-01-02T03:59:59Z | 05:59:59 +02 | true |
| 22:00-06:00 | 2026-01-02T04:00:00Z | 06:00:00 +02 | false |

Mismatch vector: checkpoint release2025b/hash A and binary release2025b/hash B (A!=B) refuses
before gate creation; release2025c/hash C without an explicit update does likewise. An explicit
update produces revision2, new hash, zero credit, and re-evaluates the current UTC instant.

## Planned regression fixtures

`tests/policy.rs`: `legacy_no_flags_store_failure_exit1`, `policy_outage_resumes`,
`policy_option_requires_activation`, `resume_needs_same_profile`, `tzdb_pinned_utc_vectors`,
`tzdb_mismatch_before_setup`, `explicit_revision_discards_permits` (S1/S3/S4/S8).
`tests/retention_policy.rs`: `close_during_cover_revalidates`, `close_before_root_no_stale_put`,
`large_root_low_rate_budget_refused`, `one_recover_only`, `lost_root_ack_same_root`,
`late_root_terminal_new_operation_required` (S3/S4/S9). Add payload-specific cases:
`payload_rate1_root_budget_incompatible` (n1024,c0,Rp1 gives1024s, beyond300s budget; no root PUT),
`payload_reserved_credit_root_cost0` (n1024,c1024,Rp1 gives0 pacing; still checks2u/window),
`payload_window_only_root_cost0_no_division` (absent rate, no r/Rp division),
`payload_root_window_closes_after_refresh` (both modes revoke admission before PUT), and
`payload_root_remote_backpressure_stalls` (admissible precredited root in both modes counts
remote nonwritability after actual local body wait ends; no-rate has no limiter wait; lost/late
root still resolves once).
Checked arithmetic vectors reject an explicit zero rate before cost evaluation. Fake store
Date/monotonic clocks drive each; none promises remote service from a local zero-cost result.
`tests/resume_budget.rs`: `large_manifest_graph_count`, `receipt_full_version_revalidation`,
`lost_put_ack_full_get_not_payload_retransmit`, `request_limit_before_new_upload`,
`episode_failure_resets_ordinal_keeps_aggregate`, `version_listing_limit_refuses` (S6).
All are future implementation tests; drafting ran none. Lab contracts follow below.

## Proposed lab contract (S2–S7, S9, S10)

No command here ran during drafting. `make wan-backup-test WAN_ACTION=preflight|run|cut|heal|status|cleanup
WAN_RUN=<32-lower-hex> WAN_ENV=physical|legacy-vm WAN_FIXTURE=/absolute/file.json` dispatches
`lab.sh wan-backup-test ACTION --run ID --environment ENV --fixture FILE`. Defaults are absent;
`run` requires a successful same-run preflight record. `cut/heal` only close/open the owned #59
object gate through its helper; they never stop the store or change wg0. `status` is read-only.
Invalid/duplicate/unknown arguments exit 2 before ssh; refusal/collision exit 1 before mutation.
The fixture is a regular caller-owned 0600 no-follow JSON file <=4096 bytes with exact keys
`schema:1`, `a`, `b`, `w`; each role has `ssh_host` (DNS label/IPv4, no option prefix), `ipv4`,
`node_id` (dependency id grammar), and `clock_contract` (fixed `/etc/flaminio/clock-contract.json`).
Hosts must be distinct, IPv4s must match the existing role/node addresses and TLS SANs; ssh uses
strict known-host checking and fixed installed guest commands, never a fixture-supplied command.
Role labels correspond to #52/#53's fl-a/fl-b/fl-w; physical addresses may be different. Reuse
existing guest setup through these selected roles; do not infer physical hosts from VM names.

Preflight records exact #51/#52/#53/#59 binaries, transport/SDK/kernel/profile/zone-table hashes,
role identities and store release. With #83 active, run `flaminio-io clock-check --clock-contract
/etc/flaminio/clock-contract.json --json` on a and b before creating volume, load, network or
store resources. Current lab VMs return **Blocked: #83 physical profile unsupported**. Never
write a false physical contract, disable probes or add `--force`. Before #83 enforcement lands,
a separately authorized `legacy-vm` run may use existing VMs; its result says exploratory, no
production/profile qualification. When enforcement is active that mode still refuses. Pure
policy/parser/fake-helper tests may run on VMs without starting a data plane. The physical mode
uses separately authorized disposable qualified a/b nodes and a disposable worker/store w.

1. Provision #52's store/worker prerequisites using its guest `backup-store.sh up` on w (2 GiB
   file-backed store, store port8333), then #53's `backup-lock-store.sh up` (separate 1 GiB store,
   TLS S3 forwarder `w.ipv4:8334`, inner S3 listener127.0.0.1:8334). Refuse existing test-owned
   paths rather than reuse. All WAN operations below use **8334 only**; 8333 is a prerequisite,
   never a second endpoint behind the gate. Owner is `flaminio-backup-store`, worker
   `flaminio-backup`; retain #52/#53 credential/PKI ownership and no secrets in argv/artifacts.
2. Via #53's owner example create a separate Object-Lock-enabled versioned bucket
   `flaminio-wan-<first12hex>`, uploader policy scoped to it, same retention probe evidence and
   canary lifecycle. Create fresh `wan-<run>.conf`, `.key`, `.evidence.json` under
   `/var/lib/flaminio-backup` as worker0600, no-follow/exclusive. Owner files remain under #53's
   store root. Select the current candidate profile only with explicit
   `--allow-unqualified-profile`; then every result must say `qualified:false` and
   `immutable_guarantee:false`. A WAN transfer pass never qualifies Object Lock or removes
   #53's off-cluster-store requirement. `repo init --immutable` uses min2m, effective max30m,
   commit budget5m, uncertainty2s and requested retain-for5m, all #53 lab values.
3. Create only owned `wan-<first12hex>` volume, exactly64 MiB, RF2, attached on a; refuse collision.
   Fill all64 MiB with deterministic incompressible data (fio write, bs64k, direct1, iodepth1,
   randseed7410, refill_buffers1, buffer_compress_percentage0), fsync and stop that writer.
   Record full device SHA-256, then `snapshot request V wan-<id> --wait 60` and require Committed.
   Start foreground fio random64KiB writes at t0+35s over the same64MiB (direct1, iodepth4,
   time_based1, runtime300) only after the cut is committed; its traffic remains on a/b paths.
   t0 is selected/recorded by preflight as step5 defines, before starting this timed load.
   This bounded fixture replaces the existing write-load.sh's fixed4GiB job.
4. Authorize #52's snapshot-read grant, install its file on w as worker0600 and delete the
   root-owned staging copy exactly as #52 does. Root helper creates only
   `/run/netns/fbw-<run>`, host veth `fw<first10hex>`, namespace `eth0` and object endpoint
   `w.ipv4:8334`; the host worker's metadata/reader connectors remain outside. Use the exact
   transport-profile broker/gate/routing settings. Record local egress dequeue timestamp/IP
   capture at the owned eth0/qdisc boundary via the independent privileged capture process;
   capture on w's forwarder input is downstream diagnostic evidence only. No capture of shared
   wg0/credentials; root0600 artifacts contain packet headers/length/timestamps, not TLS secrets.
5. Start backup as worker with wire-v1, R=1048576, configured B=65536, K=1, UTC zone and two
   half-open windows computed from a recorded UTC base t0 rounded up to the next minute:
   [t0,t0+60s), [t0+120s,t0+240s). Daily HH:MM endpoints must not straddle UTC midnight in this
   fixture; otherwise preflight chooses next-day00:10. Save exact UTC admissions/gate generations.
   After >=2 durable blocks close only its gate for10s (`cut`), then `heal`; require automatic
   revalidation/resume. Keep enough pending blocks by starting the backup at t0+40s; transfer
   exceeds20s, so the first window closes mid-object. Reopening at t0+120s must finish without
   manual rerun. Use `status`/independent store receipts to trigger the cut, never worker byte
   estimates alone. Run a separate no-window cap round, alternating the same foreground load
   alone and load+backup; publish raw elapsed/IOPS/latency/amplification, not a new perf target.
6. Require local-egress envelope <=R*dt+B_published for all packet-event intervals, observed-close
   gate/tail contract, zero new admission while closed and correct accounting for each resume
   episode. Count both directions/requests before new upload; compare actual ambiguous block PUT
   bytes <=K*4194384 separately from GETs. Check etcd/reader/replica traffic is not gated.
   `verify --archive REF --compare-source --grant NEW-GRANT` uses a separately authorized grant
   for that pinned snapshot and closes it. `restore --archive REF --output NEWFILE
   --confirm-output IDENTITY` creates an exclusive64MiB output on w; fsync and full SHA-256 must
   equal the snapshot's pre-load hash. No restore over a live block device. List must show only
   qualifying completion; the immutable candidate label remains unqualified.
7. Stop only the owned fio/worker units, resolve/close all grants via #52/#53 (abort an unrooted or
   late operation explicitly; complete a qualifying published root), drain readers, and remove
   owned netns/broker/veth/capture after checking no sockets remain. Delete output and owned
   volume only when grants/pins permit; #51 has no new snapshot-pruning API added here. Owner
   lists every test-bucket version/retention/hold and records latest retain-until with store Date
   in `.lab/wan/<run>/cleanup-after`. `cleanup` refuses deletion while any is protected, preserving
   store files/credentials and returning the recorded date. Rerun after expiry clears test holds,
   deletes exact owned versions/bucket, removes scoped identity/config/key/evidence, and only
   then invokes #53/#52 down on stores this run created, obeying their existing cleanup checks.
   No early file wipe, raw reformat or cleanup of pre-existing #53 buckets/data.

`scripts/tools/wan-backup-test.sh` fake fixtures cover every argument/id/path/ssh-quoting check,
#83 refusal before the first mutation, partial setup/foreign ownership/collision, cut/heal
ownership, capture boundary, failed grant close, retained-version cleanup refusal/restart and
only-owned deletion. Planned pure/fake tests use make lint/test; real campaigns require separate
explicit authorization and leave a complete manifest even after interrupted cleanup.
