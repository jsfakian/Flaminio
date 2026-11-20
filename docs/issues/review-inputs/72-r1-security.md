# Issue #72 plan review — round 1 — design-security (plus plan security on intent and spec)

Scope: `docs/issues/72/` (intent, spec, design, capacity-contracts, adr-proposal) in
`/tmp/flaminio-plans-2026-11-12/72`, checked against the code of that worktree. Paths below are
relative to it unless absolute. Q1–Q3 are labelled open choices and are not findings in
themselves; the findings are mechanisms left unspecified inside them, claims the code
contradicts, cross-plan contradictions and abuse cases without a test.

### R1-S1 — High

**One node can hold a device class closed, and nothing says what ends the wait.**
The class head is held from step 2 until `ready` (design.md:62–75). Step 3 needs installed
grants and "authenticated boot/sequence/plan-hash acknowledgements", and "an unavailable device
blocks or aborts before decision" (design.md:66–67), while "no process owns this lock; no timer
releases it" (design.md:64). The design never says (a) which devices must acknowledge: every
device whose E changes (potentially the whole class, since the certificate is recomputed) or
only the plan's destinations; (b) what decides between "blocks" and "aborts", and when.
Consequences under the rules as written:
- A node that is dead, partitioned from metadata, hung, or simply never sends the next
  `SyncCapacity` stalls every create, grow, move, policy change and release in its class. The
  dead-node case is exactly when rebuild admission (S5, #45) must run: recovery is serialized
  behind the same head and has no priority, although S8 says rebuild/drain take precedence.
- A policy reduction below a device's present use "cannot acknowledge preparation"
  (design.md:100–101), so it holds the head too.
- The same question for telemetry: "reports older than 15 s block new admission"
  (design.md:36) and S1's "fails closed" do not say whether one stale device blocks itself or
  the class.
- Hold time of a healthy operation is not bounded either: step 5 folds P into one
  `assignments/<node>/<uuid>/<volume>/<first_epoch>/<chunk>` key per chunk replica under the
  head (design.md:44, 73–74), and a helper advances 8 pages of at most 119 keys every 5 s
  (design.md:81, contracts:114–115). A caller that dies after the decision of a large create
  (via CSI, on a tenant's request) leaves the class, recovery included, closed for minutes.
  Acknowledgements arrive only with the 5 s report, so each operation costs at least one to two
  report periods under a class-wide lock.

Fix: state the acknowledgement set (only devices whose limits tighten, and only where a
commitment depends on the tighter limit); make a stale or non-acknowledging device ineligible
for that plan (a per-device exclusion, not a class stop) and say that a certificate treats it as
a non-destination; define the abort trigger for an undecided operation (a liveness deadline is
fine here: abort of an undecided operation is safe by step 6, so this is not safety from a
timeout); give recovery admission a way past a normal operation (abort an undecided normal
operation, or bound the work done under the head and do the fold after releasing it). Tests:
a device that never acknowledges; a dead node while rebuild admission runs; a policy reduction
below present use; a create whose caller dies after the decision, measured against a recovery
admission.

### R1-S2 — High

**The new safety rule "minimum ISR and RF count distinct nodes, not refs" has no injected bug,
no named simulator check and no vectors.**
contracts:9–12 and design.md:26–29 allow two refs of one host in a replica set and an ISR during
a same-host disk move. Today the check counts entries: `uint32(len(isr)) < h.MinISR`
(`meta/domain/domain.go:238`), and the Rust model mirrors it. Counting refs would accept
ISR `{N:U1, N:U2}` at min ISR 2 and acknowledge writes that one host loss destroys. The design's
test table (design.md:162–169) names no test for it, the injected bugs are only "missing-charge"
and "ack-on-NoSpace" (design.md:171), and `make meta-vectors` is not in the command list
(design.md:155, 172) although the transition vectors change with `*_refs`. Only the ADR's
Confirmation mentions a "same-host correlated failure simulation". `AGENTS.md` requires a
simulator check and an injected bug per new safety rule.
Fix: add the rule to the spec's invariant tests; a simulator check with host-correlated loss
during a same-host move and an injected "count refs, not hosts" bug it must catch; Go and Rust
`CheckIsrChange` vectors for a two-refs-one-host ISR (shrink, move step, takeover);
`make meta-vectors`.

### R1-S3 — Medium

**Replica-side route enforcement is stated as a principle, not as a mechanism, and one creation
path is not covered.**
"Node→disk assignments are durable metadata, not caller-supplied routing" (contracts:30), yet
Hello v2 carries a caller-supplied `device_uuid` (contracts:127–128). Not specified:
- what the replica does when the UUID or a listed chunk has no assignment, or the purpose
  differs (refuse the session? which frame?);
- the second creation path: a request for a chunk that does not exist makes the replica create
  it mid-session (`Action::CreateChunk` → `create_moved_chunk`,
  `dataplane/crates/flaminio-node/src/replica/mod.rs:347–349, 267–294`). The design gates only
  "before Hello creates/fences replicas" (design.md:24). Ungated, a target with a valid token
  allocates slots and pages on any disk of any node without a reservation, which the invariant
  forbids;
- stale tokens: tokens do not expire (`issued_at` is parsed, never checked,
  `flaminio-net/src/token.rs:116–131`; `session.rs:138–148` compares node, volume and epoch
  only). A deposed target can open Hello at its old epoch to a disk that never held the volume,
  where no fence exists, so only the assignment check stops it;
- when the cached assignments are refreshed. A miss must fetch from metadata, so every Hello
  with an unknown route is a metadata round trip a peer can trigger; no negative cache or rate
  bound is given;
- that fencing now waits for metadata: "fetch assignments … before Hello creates/fences"
  (design.md:23–24). Today a newer-epoch Hello fences without metadata
  (`replica/mod.rs:258`). A replica that cannot reach metadata could no longer be fenced by a
  new target.
Fix: specify the gate once for both creation paths (assignment present for volume, first_epoch,
chunk and this UUID, in state reserved or active; else `Refused`, no store change); raise the
fence of chunks that already exist before and independently of the route lookup; bound and
negative-cache the lookup. Tests: Hello and mid-session create for an unassigned UUID or chunk,
an old-epoch token against a disk without the volume, a wrong purpose, a route-lookup failure
during a takeover's fence.

### R1-S4 — Medium

**`NoSpace` deposes a target, but the conditions under which a replica may send it and a
target may act on it are open.**
- "Start/reopen with allocation disabled until current grants and routes are fetched"
  (design.md:99–100) does not say what a replica answers meanwhile. If it answers `NoSpace`,
  one replica restart during a metadata outage stops every target with a replica there
  (design.md:109–113), and a new attach needs metadata too. `docs/ARCHITECTURE.md:518–519`
  promises that I/O continues in a metadata outage and that only the chunks that lost a replica
  block. Fix: until grants are loaded the replica refuses the session (as a replica that is
  down), never `NoSpace`; add the test.
- contracts:133–134 bind a reply to "their live request/epoch/session" but do not restrict
  the request kind. A `NoSpace` for a Read, Heartbeat, Settle or Digest, for an unknown or
  already answered `req`, or a second one, must be a protocol error that closes the session
  (today's handling of a failed replica), not a stop of the target. Otherwise any replica, in
  the ISR or not, deposes targets at will; a session close only costs it its ISR seat.
- Who makes "a new explicit attach" (design.md:111–112) and what bounds repetition is not
  said. With overcommit above 1, one tenant filling its volume makes other tenants' targets on
  that device stop; an automatic re-attach then hits the same rejection, and each round costs
  an epoch, fences and a reconciliation on every replica. State the actor and a bound (for
  example: no automatic re-attach while the device reports the pressure that stopped it).
- The design's S6 tests (design.md:167) and `dataplane/fuzz/` have no forged, unsolicited or
  duplicated `NoSpace`, and no Hello v2 or tag 72 fuzz input. Add them.
Under Q2, record also that a denied copy to a destination outside the ISR stops the target
(design.md:113) although no acknowledged write depends on it.

### R1-S5 — Medium

**"Exclude every old writer before activation" (S12, design.md:192–194, contracts:146–148) has
no enforcement or detection for an old metadata-service instance.**
ALPN `flaminio-repl/2` excludes old replication peers only. An old `flaminio-meta` still
running (a forgotten pod, a rollback) keeps creating and moving: its transactions compare only
the volume's own keys (`meta/service/service.go:414–417`, claim on `volumes/<id>` and
`tombstones/<id>`), so they commit after activation and add replicas no ledger entry charges.
The invariant then fails silently, and the principle is that safety comes from linearizable
metadata, not from an operator's procedure.
Fix: one of: new binaries only under a key prefix or etcd credentials the old ones do not
have; or a reconciler that compares chunk records with assignments and closes the class for
admission (with the volume named in `ListCapacity`) on any uncharged replica. Test: an old-style
create and `MoveReplica` written straight to the store after activation are refused or detected
before the next admission.

### R1-S6 — Medium

**`Volume` tags 20 and 21 collide with #50, and the contract claims the tags are
collision-free.**
contracts:24: `Volume: string device_class=20, data_locality=21; … fenced_refs=22;
capacity_generation=23`. #50's draft
(`/home/jsfakian/Documents/src/flaminio-50/docs/issues/50/contracts.md:37–38`):
`Volume encryption=20 (EncryptionDescriptor), key_source_storage_mode=21 (string)`. Both are
length-delimited, so a reader of one schema decodes the other's field without a wire-type
error: an encryption descriptor read as a device class, or the reverse. contracts:36 says
"proposed collision-free additions".
Fix: pick tags no draft uses (drafts so far: #13/#17/#18 9–10, #70 11–14, #50 20–21, #75 40)
and say so in contracts:36–38.

### R1-S7 — Medium

**The ref fields exceed #80's response bound with certainty, two repeated fields have no
count limit, and the plan leaves the outcome to later fixtures.**
#80 admits geometry under `B(N,R) = 2048 + N*(288 + 264*(R+1)) ≤ 4 MiB`
(`/tmp/flaminio-plans-2026-11-12/80/docs/issues/80/design.md:30–33`): two 128-byte names per
replica and two moving endpoints. contracts:14–17 keep those node fields "as informational
projections" and add `replica_refs`, `isr_refs`, `moving_from_ref`, `moving_to_ref` and
`released_refs` per chunk, about 170 bytes per ref at worst (node 128 + UUID 36 + framing):
at R=3 roughly 1,700 more bytes per chunk before `released_refs`, against 1,344 budgeted. A
volume admitted at #80's cap (N=3119 at R=3) then has a `GetVolume`/`Attach` response of about
9 MiB against a 4 MiB receive limit and cannot be attached. `released_refs` (ChunkMeta) and
`fenced_refs` (Volume) have no maximum count. contracts:39–41 and design.md:157–158 say "if
those additions exceed the bound"; #80 says existing volumes "must not be … made unreadable
during profile enablement" (#80 contracts.md:48–49).
Fix: decide now: drop the node projections once refs are authoritative (or stop sending both),
cap `released_refs` per chunk and `fenced_refs` per volume, give the new formula and N caps
jointly with #80, and add an activation rule for existing volumes above the new cap (activation
of the class is refused and reported, never a response that cannot be received).

### R1-S8 — Medium

**Registered totals can never be checked against the disk.**
design.md:37–38: "validate reported totals against registered UUID/layout". Neither
`OpenCapacityReport` nor `SyncCapacity` carries totals (contracts:60–70: only used figures),
and `RegisterDevice` takes `data_bytes`, `pool_pages`, `chunk_slots` typed by an administrator
(contracts:49–52). A wrong registration (bytes of the raw disk instead of usable data, a unit
slip) makes T too large and every budget derived from it over-admits, with no evidence in any
report. S1 requires "inconsistent inventory fails closed".
Fix: `OpenCapacityReport` carries the store's layout totals (`data_clusters`, cluster size,
`pool_pairs`, chunk slots, from `Stats`/the superblock); the service refuses the session and
marks the device ineligible on mismatch. Test: registration larger and smaller than the layout.

### R1-S9 — Low

**`SyncCapacity` has no server-side rate bound, and report age has no defined clock.**
Each report is a durable `reports/<uuid>` write (design.md:42–43); the only rule is a strictly
increasing sequence (contracts:68). A node identity can therefore drive etcd writes and MVCC
growth at any rate. "Metadata records receipt age" (design.md:38) does not say whose clock;
several service instances with skewed wall clocks would disagree on the 15 s rule.
Fix: refuse a report sooner than a minimum interval after the stored one
(`ResourceExhausted`), and express freshness the way liveness is today (`PutTTL`, as
`alive/<id>`: absence is staleness, no clocks compared). Test: a report flood.

### R1-S10 — Low

**`NoSpace` is specified in network byte order; the frame codec is little-endian.**
contracts:132: "body length 10 bytes, network byte order". Every frame integer, `req`
included, is little-endian (`dataplane/crates/flaminio-net/src/frame.rs:156–165`, 561). A
big-endian `req` matches no live request. Fix: say little-endian, as the other frames.

### R1-S11 — Low

**ALPN alone does not refuse an old peer.**
contracts:126–127: peers are "refused before Hello". No ALPN is configured today
(`flaminio-net/src/tls.rs:71–93`). A rustls server with protocols configured rejects only a
client that offers a non-matching list; a client that offers none (every v1 binary) completes
the handshake (rustls 0.23.46 `server/hs.rs:105–120`), and a client does not fail when the
server selects none. A v1 Hello would then be parsed as v2.
Fix: both sides check `alpn_protocol() == Some("flaminio-repl/2")` after the handshake and
refuse otherwise; test with a peer that offers no ALPN, in both directions.

### R1-S12 — Low

**New strings that become key segments, metric labels and CLI arguments have no validation
rule.** `device_class` is an etcd key segment (`classes/<class>`, design.md:42), an OpenMetrics
label and a CLI argument (contracts:138–141); contracts:7 gives rules only for node and UUID.
`boot`, `reserve_domain`, `purpose`, `state` and `applied_plan_hash` (exactly 32 bytes) have
none either; contracts:102 lists only page tokens, limits and UUIDs as `InvalidArgument`.
Fix: `device_class` passes `domain.ValidID` (`meta/domain/domain.go:87–90`); enumerations are
closed sets; `boot` and the hash have exact lengths; all checked before any read or write.

### R1-S13 — Low

**Caller-controlled paging inputs lack rules and tests.** `ListReplicaAssignments` lets a node
supply `after` and `revision` (contracts:71–73). Not stated: that `after` is confined to the
caller's own `assignments/<node>/<uuid>/` prefix (a crafted token must not page into another
node's routes), and what a revision above the store's current one returns (contracts:100–102
cover only compacted ones). Fix: state both (`InvalidArgument`), and add them to the auth
matrix of design.md:169.

### R1-S14 — Low

**Locality priority can be claimed by pod churn.** Priority is "rebuild then drain then
locality then balance" (design.md:121) under 8 active moves per class (design.md:119). A
workload that reschedules across nodes after each 600 s cooldown re-requests a move of every
chunk of its volumes and keeps the class's discretionary slots, so balancing never runs. No
per-volume share exists. Fix: bound locality's share of the class budget (or a per-volume
limit of active and queued locality moves), and add the churn-starves-balance schedule to the
S8/S9 tests.

## Nits

- contracts:129–130: "TCP connections may remain pooled per node pair". No pooling exists: a
  connection carries one session (`flaminio-net/src/session.rs:176`, "one session per
  connection"). Say "one connection per session, as today" or design the multiplexing.
- spec S3 "these tests are inclusive at the threshold", design.md:53–54 "comparisons at
  threshold refuse additional work", and the budgets use `<=` (design.md:50–51). State for each
  test whether a projected value equal to the threshold is admitted; the "watermark ±1" tests
  depend on it.
- contracts:87–88: "only a decided/ready operation authorizes a route" while design.md:74–75
  says publication and copying wait for `ready`. Pick one state.

Reviewed commit: 813d9a5cb1d210c427c7afe8fa73cc1f3669fd7a
