# Issue #90 — FE0/LID strict local target and CSI contract

Proposed, unapproved; normative [design](design.md) appendix fixing SEC90-R2-01.
This is #90's whoever-lands-second amendment to the read-only technical contracts #15
`3592050cc4da3cacf4c9a759692907534fbcccb3` (device lifetime), #79
`52f7a966fa29cbea967d9252880636a328bd0b26` (normal intent/use admission) and #83
`f9a0a519adf21d4ee2c9048f44139489446c94e7` (fresh-process handoff). Those contracts remain
owners of device holding, operation retry, initialization and process freshness; this appendix
adds the missing exact legacy incarnation identity, not another target lifecycle.

## Current-process normal handoff

A strict target accepts exactly one identity form: positive FirstEpoch with no LID and FLCTv2,
or enrolled `(FirstEpoch=0,LID)` with nonzero UUID16, FLCTv3 and profile3/4 from
[wire-contracts](wire-contracts.md). For FE0 the engine verifies signed LID equals current
metadata header LID, its operation intent and cached target identity; FirstEpoch0 without that
explicit profile is still refused. Registry entries, target handles and device lifetimes cache
the verified LID; no daemon argument or current pathname supplies it.

Use **unchanged FLCH134** bytes: version1, purpose1 normal, current healthy-process nonce32,
SHA256(exact raw FLCT)32 and metadata Ed25519 signature64. The FLCTv3 signature and exact-token
hash bind LID along with volume/node/epoch; no new FLCH version or unsigned LID field is needed.
Metadata signs only after actual new-epoch normal Attach commits with existing own-agent/admin
authorization, current target and #79 use predicates. `AttachRequest.target_nonce=6` and normal
`AttachResponse.target_handoff=3` keep their owners. Nonce is32 nonzero CSPRNG bytes from this
daemon's attach-challenge; nonce+unless_target is InvalidArgument before mutation. Node/io
cannot mint Attach or borrow agent credentials. Engine verifies both signatures, purpose1,
current nonce/token digest, exact current incarnation/target/epoch and per-process ran_epochs
before target allocation/Hello. A signature alone never permits a same-epoch new lifetime.

For a paged volume use [budgets.md](budgets.md)'s AttachIdentityVolume with target_nonce12 and
returned target_handoff5; its existing header/gate revisions3/4 remain unchanged. It performs
the same new-epoch CAS and signing, not a read-only handoff mint. Complete coherent pages pinned
to that committed summary must be assembled before daemon target admission. A page conflict
invalidates the candidate/handoff for startup: discard it and obtain another fresh Attach epoch.
Ordinary and paged FE0 use the same strict engine entry and normal purpose1 path. No new
per-I/O metadata round trip or lock follows from these checks.

## Exact versioned device identity

FE0 callers must use these additive commands; positive lifetimes may also use them:

```text
device-info-v2 VOLUME
  -> ok none
  -> ok VOLUME starting BOOT_UUID INSTANCE GEN FIRST_EPOCH LID
  -> ok VOLUME running|stopping BOOT_UUID INSTANCE GEN FIRST_EPOCH LID DEVICE MAJ:MIN DISKSEQ
detach-device-v2 VOLUME BOOT_UUID INSTANCE GEN FIRST_EPOCH LID MAJ:MIN DISKSEQ
  -> ok detached | ok absent | error: VOLUME: identity: stale attachment
attachment-status-v2 VOLUME OP
  -> ok none
  -> ok starting OP EPOCH
  -> ok bound OP EPOCH VOLUME running|stopping BOOT_UUID INSTANCE GEN FIRST_EPOCH LID DEVICE MAJ:MIN DISKSEQ
```

`LID` is exactly `-` when FIRST_EPOCH is positive; when FIRST_EPOCH=0 it is the exact nonzero
canonical lowercase UUID36. All other numeric lifetime fields and EPOCH remain nonzero checked
u64. FIRST_EPOCH is canonical decimal u64 (0 only in the LID form). VOLUME uses the shared
ASCII1..128 metadata ID grammar; OP and BOOT_UUID use their owners' canonical UUID grammar.
DEVICE, MAJ:MIN, DISKSEQ and kernel checks remain exactly #15/#74's held-ublk rules. New command/
reply lines<=1024 bytes including newline; exact arity, ASCII, no trailing fields, duplicate
fields, suffix parsing, overflow or alternate zero spelling. Unknown command/version/profile,
missing/malformed LID or FE0 on a v1 device command refuses before effects. In particular old
`device-info`/`detach-device` cannot silently omit LID for an FE0 entry. No fallback is permitted.

The v2 lifetime tuple is `(volume,first_epoch,LID-or-none,boot_uuid,instance,generation,rdev,diskseq)`.
Path remains diagnostic. The daemon reads it atomically from one registry entry; detach compares
**every** field before routing that captured generation, as #15 requires. CSI obtains expected
incarnation from verified metadata/intent, checks its own held fd/sysfs/rdev/diskseq, then repeats
device-info-v2 and compares the complete tuple. An extra/wrong/stale LID is an identity failure,
never permission to adopt or detach another lifetime. Existing changed-state/error codes and
held-fd/no-reuse rules remain; attachment-status-v2 additionally compares exact operation and
fsynced authority_epoch before binding. No target nonce/token is emitted by device-info.

## Normal CSI intent, restart and initialization refusal

The #79 normal noninitializing path retains its order: durable unbound intent before any attach,
read daemon challenge and fsync nonce, own-agent OnlyIfTarget Attach with actual epoch advance,
fsync returned epoch, then `attach-operation VOLUME OP REPLICA_TOKEN HANDOFF`. That command's
arity is unchanged; LID comes from FLCTv3, never a new unverified argument. FE0 operation recovery
uses attachment-status-v2 and the tuple above. Never persist either credential.

Local intent schema2 adds `identity_profile` (`positive-v1` or `legacy-lid-v1`) and
`legacy_incarnation` (JSON null for positive, UUID36 for FE0), retaining **all** #79 path/mount/
operation/authority_epoch/target_nonce/purpose fields. FE0 schema2 requires first_epoch0,
legacy-lid-v1, exact LID and purpose=normal; positive requires first_epoch>0, positive-v1 and
null LID. Tuple is null only for reserved_unbound, otherwise its LID and every #15 field must
match the intent/verified target. Strict bounded JSON<=64KiB rejects duplicate/unknown fields,
wrong types and unknown schema/profile before probe/mount/detach. Existing schema1 remains
usable only for its previously supported positive lifetimes; FE0 schema1 is not upgraded by
inferring identity from disk/path. Fsync the full schema2 intent and bound tuple atomically in
the existing #79 order; missing/corrupt intent cannot authorize adoption or cleanup effects.

Lost metadata Attach reply means no daemon attach was sent: retain intent, reread the exact
incarnation and obtain a genuinely fresh epoch/FLCH. Lost daemon reply queries the exact OP,
current challenge, returned authority_epoch and full v2 tuple; same-process matching live entry
may be rebound without another target. Absent entry may retry only an unconsumed current-process
handoff as #79/#83 allow. Changed daemon nonce requires new normal Attach epoch, even when this
node holds no local replica fence; old FE0 token/handoff/epoch cannot restart it. Changed LID or
operation refuses and preserves intent/bytes. Lost detach reply retries the same complete v2
tuple, never a plain detach; stale/new lifetime remains untouched.

Enrollment remains Unknown/nonfresh under #79. Prepare normal Raw or an already recognized
filesystem may seal the permanent use mode and mount it; an ambiguous/blank filesystem signature
fails without mkfs. FE0 cannot enter FreshFs/Reserved/Started, issue FLCI/purpose2 FLCH, attach-init
or start a fresh filesystem initialization. Existing positive initialization behavior is unchanged.
When #15/#79/#83 strict profiles are composed, activation verifies these local/CSI adapters
are installed before enabling that combination; whoever lands second supplies them.

## Concrete future acceptance tests

- Go metadata + Rust strict daemon + actual CSI normal path: proven FE0 Raw and existing ext4,
  ordinary and snapshot profiles, normal full Attach and paged AttachIdentityVolume. New epoch,
  FLCTv3+purpose1 FLCH, exact LID device tuple/intent and mount/read preserve acknowledged current
  bytes and committed Before views; no init ticket, mkfs or DeleteEarlierIncarnation occurs.
- Normal and paged service tests on Mem/real etcd: nonce12 exact32/nonzero, nonce+unless_target,
  auth/current-target and use gates, handoff only after committed epoch, lost Attach reply then
  newer-epoch retry, page conflict before target start, max summary plus FLCH134 within the same
  32KiB fixed allowance. Go/Rust golden vectors prove FLCH hash changes for a different signed LID.
- Real control/CSI fixtures vary LID, FirstEpoch/profile, purpose, process nonce, token digest,
  OP, epoch and every old tuple field; reject before allocation/Hello/probe/mount/detach on a
  mismatch. Test v1 FE0 refusal,1024/+1 lines,64KiB/+1 intent, unknown versions/fields/duplicate
  JSON, lost bound-intent fsync, same-daemon plugin restart/lost daemon reply and exact live-entry
  recovery, daemon restart with no local replica fence, stale handoff and fresh-epoch success.
  Delayed old-LID detach/reply cannot affect a new lifetime or positive reincarnation.
- Reuse #83 FreshProcessEpoch and its caught old-process/same-epoch handoff mutants with FLCTv3
  and paged attach, plus #79 NoPublicInitAdmission: LID does not bypass nonce, ran_epochs or the
  legacy fresh-filesystem refusal. `make proto test race contracts sim` runs the changed codecs/
  adapters and model witnesses; no runtime/lab test is claimed by this docs-only revision.
