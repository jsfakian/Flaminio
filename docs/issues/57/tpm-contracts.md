# Issue #57 — concrete TPM source and recovery contracts

Proposed review revision, 2026-11-19; unapproved. Historical approval is preserved separately.
[Design](design.md) · [specification](spec.md).

## Static identity and canary

The material adapter constructs #50's static KeySource: Kind=ID=`static`; KEKVersion is
`k1-` plus lowercase hex of the first8 bytes of HMAC-SHA256(KEK, `FLKEKCHK001`). It is19 ASCII
bytes, derived only from the32-byte KEK. No CLI selects an identity or version. Bundle and
escrow identity are assertions checked against the material, never aliases for a different key.
Use #50's1..64 ASCII `[A-Za-z0-9._-]` identity bounds. The cluster source pin remains immutable;
resealing/recovering the same KEK changes storage only. This increment adds no rotation or
multi-version key ring and does not lift #51–#53's encrypted snapshot/backup refusals. Preserve
all prior escrow/bundle artifacts; later encrypted retention requires its own reference rules.

The bundle stores `canary_binding`, `canary_envelope` and `canary_sha256`. Binding is
VolumeID=`tpm-canary-` plus32 lowercase hex digits from16 random bytes, FirstEpoch=1,
Profile=`xts-aes256-v1`. This is a local test identity, never a metadata volume. Draw a64-byte
canary DEK from crypto/rand, again if its32-byte halves match. Wrap through #50's exact92-byte
static envelope and authenticated Binding; persist SHA256 of the canary DEK, then clear it.
Verification derives the material identity, unwraps using the stored exact Binding, validates
64-byte unequal-halves plaintext and its digest, and clears the buffer. No plaintext canary DEK
or production DEK is persisted. Every holder/recovery uses this same complete contract.

`INSTANCE_UNAVAILABLE` remains gRPC Unavailable and advances to the next instance; ErrorInfo
is diagnostic. Other codes follow #50's failure table. There is no8-call in-flight bound in
#50's current increment; local wrap/unwrap retain its1s contexts and no TPM transactions.

## Versioned TPM recipe

Recipe `rsa-owner-pcr-v1` uses go-tpm v0.9.8 modern typed API. Recreate an owner-hierarchy
primary with the upstream RSASRKTemplate unchanged, empty primary userAuth and no sensitive
data. Record/compare its actual SHA256 public Name before Load. It is not a persistent handle;
never evict another object's handle or change hierarchy authorization. This first recipe requires
existing owner-hierarchy authorization to be empty, as asserted by owner_auth_policy=empty-existing
in the qualified profile. CreatePrimary uses a SHA256 HMAC session with empty auth, not TPM_RS_PW.
A nonempty hierarchy rejects this attempt before candidate publication and leaves authorization
unchanged. There is no hierarchy-auth input, persistence or unattended boot dependency.

Child: Type=KeyedHash, NameAlg=SHA256, FixedTPM/FixedParent/AdminWithPolicy/NoDA=true;
UserWithAuth/SensitiveDataOrigin/STClear/EncryptedDuplication/Restricted/Decrypt/SignEncrypt=false.
Scheme=Null; sensitive data is exactly32-byte KEK, child userAuth empty. Decode/verify the full
public attributes and policy, not merely a cached recipe label.

Enrollment uses SHA256 trial PolicyPCR with one SHA256 bank, sorted selected indices and
SHA256 of concatenated selected PCR values, then PolicyCommandCode(TPM_CC_Unseal), in that
order. Store PolicyGetDigest output as child authPolicy; no PolicyOR/AuthValue/Password path.
Runtime starts a real policy session and repeats those two commands with the bundle's exact
PCR digest/selection. Use parent-salted SHA256 sessions with32-byte nonces and AES128-CFB:
HMAC with EncryptIn authorizes Create and encrypts its first sensitive parameter; the real
policy authorization session has EncryptOut for Unseal's first response parameter. Trial
sessions never authorize Unseal. Verify response HMACs before using any output; best-effort
flush all transient child/parent/session handles, with timeout ownership as below.

## Protected TPM owner and deadline

Use one isolated `flaminio-meta key-source owner --attempt HEX32` child per attempt,
with the parent-generated16-byte attempt nonce as32 lowercase hex digits, fixed argv and
an inherited private AF_UNIX SOCK_SEQPACKET descriptor3. It accepts one operation, never a
listening socket or arbitrary device/command. Both parent and exec'd child run the common
TPM-source protected-memory preflight below before receiving source key material; memory locks do not survive
exec. The parent passes public bundle/profile data and protected FD payloads only after Ready.
IPC version1: each record is u32BE length (1..71680), counting type:u8 plus its exact payload.
Ready(type1) has no body. Request(type2) is op:u8(1unseal,2seal), attempt_nonce[16],
u32BE bundle_len||bundle_json, u32BE profile_len||profile_json, kek_len:u8||KEK.
Bundle≤65536, profile≤4096; unseal requires
bundle present/KEK length0, seal requires bundle length0/KEK length32. Result(type3) echoes
op/attempt_nonce and, for unseal, exactly32 KEK bytes; for seal it has u16BE parent_name_len=34,
parent_name, u16BE public_len||public≤8192, u16BE private_len||private≤8192 and policy_digest32.
Seal's owner compares its Create→Load→Unseal output with the supplied KEK before success.
Failure(type4) echoes op/nonce plus one-byte class (1config,2policy,3device,4memory,5ownerbusy).
There is no free-form error payload. After Ready, exactly one Request and one terminal response; no ancillary
FDs, trailing bytes or unknown types. Failure before Ready exits nonzero without secret input;
the parent classifies a missing Ready/early exit as local source unavailable. Parent checks response nonce/op and all lengths before use.
Secret buffers never reach stdout/stderr. Parent and child close unrelated inherited FDs.

Before Ready/opening TPM, the owner exclusively locks the fixed host file
`/run/flaminio-tpm/owner.lock` with nonblocking flock. Host tmpfiles creates the directory
root:flaminio-tpm0750 and regular files owner.lock/owner.state root:flaminio-tpm0660 once per
boot. Initial owner.state has length0; tmpfiles creates it only when absent and never
truncates an existing file. The first owner publishes the128-byte live record under flock.
Neither file is removed/truncated/replaced at unit stop. Validate no symlink, owner/mode,
regular type and link count1. Existing files may be opened read/write through ReadWritePaths;
no directory creation/rename authority is needed. A busy lock fails before reading TPM-source material. Validate the argv nonce before recording ownership;
the Request nonce must match it. The record is complete before Ready.

Under the lock, owner.state is empty only for initial use, else exactly128 bytes. Layout:
magic8 `FLTPMO01`; u32BE version1; u32BE state (1live,2closed); boot UUID16; u32BE PID;
u32BE reserved0; u64BE /proc/PID/stat starttime ticks; attempt_nonce16; u32BE device major;
u32BE device minor; reserved52 zero; u32BE CRC32C of the preceding124 bytes. Any malformed,
partial or foreign-boot record refuses without overwrite. A live record whose PID still has
that starttime blocks new ownership, even if its flock was released during kernel exit.
Only a closed record, or a live record whose original process is provably gone (PID absent or
reused with different starttime), permits a new attempt. Missing/inaccessible /proc evidence
is not proof of exit. Never signal a PID obtained only from a stale record.

The owner pwrite's its live record, fdatasyncs and read-backs it before opening TPM or reading
source-material payload. Partial/failed publication refuses TPM access and leaves state blocked. After
explicit successful device close it writes/syncs state=closed; otherwise live state remains.
This supplements flock across SIGKILL/FD-close ordering: a blocked exiting owner can release
flock before TPM close completes, but its live PID record still excludes replacements. All participants use the host PID namespace; private PID namespaces are unsupported. A
DynamicUser restart must treat permission-denied/hidden identity as unknown, never as absent.
New workers never erase that evidence. Operator maintenance can clear ambiguous state only after
proving no old process/TPM use; the normal protocol never clears it to force progress.

Startup has one5s monotonic caller budget including worker startup, commands, response and
successful owner exit. A result is usable only if complete, identity/canary-valid, the owner has
exited0 and acceptance occurs before the deadline. On timeout the parent permanently cancels
that attempt, closes result reception, clears received material and signals its pidfd-pinned
owner with SIGKILL. It does not wait unboundedly to serve ordinary metadata methods. Any late
KEK/result is discarded; there is no retry in that metadata process. Seal/recover use the same
owner but a30s caller budget for the complete multi-command operation; timeout publishes no
candidate and prints only owner-quarantined/source-unavailable. It may report the nonsecret
PID/starttime/boot and attempt ID needed for maintenance, never secret buffers.

Kernel read/write/close can remain blocked despite SIGKILL. Such an owner remains quarantined
with its locked memory, bounded input and lifetime lock until the kernel returns; cleanup is
not claimed complete at5s. A reaper retains only process bookkeeping, no copied KEK or TPM FD.
Restarts/new CLI invocations see the same lock and cannot create another TPM-owning worker or
accept its result. Never reset the TPM, replace the lock or force concurrent owners to recover.
Report this state distinctly; operator recovery may require host maintenance/reboot. Successful
unseal leaves no owner/device/session on the attach path; only #50's in-memory source remains.

## Host and unit profile

Only TPM mode changes the existing meta unit: keep DynamicUser=yes, no capabilities,
NoNewPrivileges, ProtectSystem/ProtectHome/PrivateTmp and AF_UNIX support. Add
mlockall/prctl/prlimit64/pidfd_open/pidfd_send_signal to the syscall allowlist. Use
PrivateDevices=no, DevicePolicy=closed and DeviceAllow=/dev/tpmrm0 rw, plus
SupplementaryGroups=flaminio-tpm. Only this profile exposes host device names; the cgroup still
denies other physical device access. Host udev makes only tpmrm0 root:flaminio-tpm0660;
validate its character-device identity before open. No broad disk/raw TPM group or root broker.
File-static mode retains PrivateDevices=yes. A missing/denied TPM is local source unavailable,
not an ExecStartPre failure that prevents unencrypted service.

Deliver the selected root-owned0600 immutable bundle with LoadCredential=sealed:ABSOLUTE_PATH;
ExecStart points --sealed-bundle at ${CREDENTIALS_DIRECTORY}/sealed. Add the nonsecret fallback
SetCredential=sealed:source-unavailable so missing/unreadable source data is not a fatal systemd
credential lookup; the parser refuses the sentinel as an unavailable source. No KEK/unlock key
is a systemd credential or environment value. The runtime directory copy contains sealed bytes.
Allow ReadWritePaths=/run/flaminio-tpm for the existing owned lock/state files; no
chmod/unlink or arbitrary path is part of the protocol.

TPM profile sets LimitCORE=0, LimitMEMLOCK=infinity, MemoryMax=512M, MemorySwapMax=0,
TasksMax=128 and GOMEMLIMIT=384MiB. The metadata parent and its sole owner share that cgroup;
no CAP_IPC_LOCK is needed with the inherited unlimited memlock rlimit. This is a bounded optional
TPM profile, not a compact-mode memory claim. Host qualification requires no swap devices, no
hibernation and disabled kernel crash capture/kdump; root administration is trusted to maintain that condition. The common
Go preflight sets RLIMIT_CORE=0, PR_SET_DUMPABLE=0, mlockall(MCL_CURRENT|MCL_FUTURE|MCL_ONFAULT), checks
rlimits/cgroup profile and disables heap/core/debug output before reading TPM-source KEK/escrow-unlock input or constructing its canary/wrapping cipher.
This preflight covers KEK, unlock key, transient canary DEK and their sensitive input streams;
baseline TLS identity/token-key loading remains governed by existing setup and is outside #57.
Any failure reads no source key material and leaves only encrypted key service unavailable;
unencrypted RPC service still starts. TPM-source seal/recover/escrow commands and
each exec'd owner run the same preflight; operator tools need the same explicit limits/profile.
MCL_ONFAULT avoids prefaulting reserved Go mappings; pages lock when first touched.
Preflight requires /proc/swaps to contain only its header, readable /sys/power/state without
`disk`, and readable /sys/kernel/kexec_crash_loaded equal0. Missing/unreadable/ambiguous evidence
refuses. Host boot policy disables hibernation (for example nohibernate where supported) and
kdump loaders; trusted root must not re-enable them. These checks bound supported hosts without
pretending an application prevents a malicious administrator from changing global kernel policy.
Go allocation/cgroup OOM may terminate a process; no claim of continued service after OOM.

TPM-source key inputs are dedicated readable pipes from a trusted protected-memory provisioner, not
files, argv, environment, TTY or ordinary shell-copy pipelines; inputs have exact length limits,
separate FDs and bounded read deadlines. The producer is part of the qualified admin procedure,
with dump/swap/hibernation restrictions too. No generic network provisioning service is added.
No plaintext KEK is sent through unqualified SSH/logging/copy processes. Best-effort clearing is
used, but compiler/GC zeroization is not guaranteed; all process memory stays locked while alive.

For a local root admin command, the documented launcher sets ulimit core0/memlock unlimited,
then uses systemd-run --scope with MemoryMax=512M/MemorySwapMax=0/TasksMax=128 and
GOMEMLIMIT=384MiB. FD0 is inherited as the dedicated input pipe; the launcher never reads it.
The command checks the resulting limits itself before input; an ordinary unqualified shell
invocation refuses. The provisioner, command and owner each enforce protected memory.

## Bundle, escrow and immutable publication

`sealed.json` schema_version1 has exactly: recipe_id, source_kind, source_id, kek_version,
profile_id, pcr_bank, pcrs, parent_name, public, private, policy_digest, canary_binding,
canary_envelope, canary_sha256. Source fields must equal the derived static identity; recipe
is rsa-owner-pcr-v1. Whole JSON≤64KiB; reject duplicate/unknown keys, trailing data and bad
encodings. Binary fields use canonical padded standard base64; public/private≤8KiB each;
parent_name34 bytes; policy/canary SHA256 digests32 bytes; canary static Sealed92 bytes.
PCRs are1..24 unique ascending indices0..23 with32-byte SHA256 values, bank=sha256.
Profile ID is1..64 ASCII identifier bytes. `canary_binding` has exactly volume_id, first_epoch, profile; `canary_envelope` exactly
version, kind, id, kek_version, profile, sealed, using #50's flat record encoding. Public/private
are complete marshaled TPM2B_PUBLIC/TPM2B_PRIVATE, decoded with no leftover bytes; parent_name
is the34-byte Name buffer without a TPM2B size prefix. Canary Binding/Envelope follows #50; no partial identity or reconstructed implicit Binding is accepted.

Escrow is exactly97 bytes: magic8 `FLTPMR01`, u16BE idlen=6, `static`, u16BE versionlen=19,
derived version19 bytes, nonce12, AES256-GCM ciphertext+tag48 over the32-byte KEK. AAD is
magic through the version bytes; no trailing bytes. Unlock key is32 independently random
bytes held off-node separately; it is never a human password or stored on the node. The
trusted off-node operator creates, read-backs, decrypts and verifies escrow before node
activation. Node seal/recover decrypts the supplied escrow in protected memory and compares
its KEK/derived identity before proceeding; this proves recoverability, while off-node custody
remains an explicit administrator responsibility, not a location claim inferred from a path.

Local CLI: `flaminio-meta key-source escrow --input-fd N --out-new PATH` runs off-node;
`seal --input-fd N --bundle-new PATH --profile PATH`; `recover --input-fd N
--previous-bundle PATH --bundle-new PATH --profile PATH`; `inspect --bundle PATH` reads no
source key material and prints only nonsecret source/profile/PCR/recipe fields. No source/version flag.
The sole source-material input FD is a dedicated pipe; record header is magic8 `FLTINP01`, op:u8
(1escrow,2seal,3recover), reserved[3]=0, u32BE length, exact payload and EOF. Escrow payload:
KEK32||unlock32. Seal: KEK32||escrow97||unlock32 (161 bytes). Recover: escrow97||unlock32
(129 bytes). No hierarchy-auth field or FD is accepted; extra/legacy auth bytes fail as trailing
data. Payload≤161 bytes;
read deadline5s before any TPM command. No other FD/terminal/file/environment source-material input.
Dedicated protected-memory producer writes this record without shell tracing or plaintext
intermediates. Exit0 means validated candidate durably published,1failure/uncertain,2badargs;
no command edits service configuration, restarts a service, deletes escrow or changes a pin.

Use a fresh root-owned0600 destination under an already trusted root-owned directory; reject
existing paths, symlink components, relative/dot/dotdot paths and an active candidate name.
Create an exclusive same-directory temp, write/fsync candidate, then renameat2(RENAME_NOREPLACE)
to the fresh path and fsync directory. Never overwrite an existing bundle, even from another
concurrent command. Failed/uncertain publication leaves active selection and prior bundles
untouched; report the candidate path as uncertain and do not select it automatically. A later
inspect/verify reads it as untrusted data before reuse; never infer durability from exit failure.
The prior bundle remains selected until the administrator explicitly configures the validated
new immutable path in the unit. Startup validates/unseals/canary-checks that selected candidate;
failure disables that holder's encrypted source, with no fallback to its prior bundle/file.
Previous valid path can be explicitly selected again; a changed PCR may require recovery.

Seal verifies escrow before building a fresh canary, then Create/Load/Unseal and canary check
before publication. Recover verifies the old canary from previous-bundle using escrow material,
creates a new TPM object for the qualified replacement profile, and verifies the same canary
before publishing. Repeated/concurrent operations cannot replace active bytes. A post-publication
directory-fsync failure may leave an unselected candidate but never destroys the old one.

## Isolated local swtpm integration

`make tpm-test` builds a Go test binary for pkg/keysource/tpmstatic and uses its `_test.go`
child entrypoint to execute the same owner protocol/recipe/state machine. An unexported owner
constructor receives transport, lock/state paths, expected file-owner UID and host-fact reader.
The production constructor hardcodes /dev/tpmrm0, /run/flaminio-tpm, UID0 and real host facts;
shipping binaries expose no test flag, environment override, socket or alternative constructor.
Only test-binary code injects a Unix transport through net.DialUnix and a test-only
transport.TPM Send adapter, private fixture files and synthetic host-profile facts. The adapter
serializes each complete command/response pair under one mutex and loops writes until the
entire command is sent; zero-progress/error writes fail and close the connection. It validates
the command header/declared length against actual bytes and a65536-byte cap before sending.
For each reply, io.ReadFull reads the10-byte TPM2 header: tag:u16BE, total_size:u32BE,
response_code:u32BE. Accept only TPM2 tags0x8001/0x8002 and10≤total_size≤65536, then allocate
the bounded complete response and ReadFull exactly total_size-10 body bytes. Truncation, invalid
header/length or I/O deadline closes/fails the connection; never reuse a partial stream or pass
raw single-read transport.FromReadWriteCloser through to go-tpm. Socket deadlines are at most5s
per Send and cannot extend the owner's overall caller deadline. Real
prctl/mlockall/core/cgroup checks still run for the protected-memory positive cases; negative
fixtures force/refuse locking before any source input. No test result qualifies physical boot.

The runner stages the root-owned0755 test binary under a fresh /run/flaminio-tpm-tests/RUN
with root-owned0755 staging directories
and launches a dedicated transient systemd test service with DynamicUser=yes, PrivateDevices=yes,
PrivateNetwork=yes, NoNewPrivileges=yes, no capabilities, ProtectSystem=strict and
InaccessiblePaths=-/run/flaminio-tpm. RuntimeDirectory=flaminio-tpm-test-RUN with RuntimeDirectoryMode=0700 provides the
service-owned0700 writable fixture tree; no host production state path is used. Use the same
LimitCORE=0/LimitMEMLOCK=infinity/MemoryMax=512M/MemorySwapMax=0/TasksMax=128 limits and
GOMEMLIMIT=384MiB. Fixture lock/state files are0600, owned by the test service UID; only the
test constructor substitutes that UID and tree. Parent/child repeat actual memory preflight.

Inside that service, run `swtpm socket --tpm2` with private0700 state and0600 Unix data/
control sockets, plus --tpmstate dir=STATE, --server type=unixio,path=DATA,mode=0600,
--ctrl type=unixio,path=CTRL,mode=0600 and --flags startup-clear. Pin/record swtpm/libtpms tool
versions. Connect only to DATA in the fixture tree; restart/wrong-TPM/PCR cases use distinct
owned state. A transport/state access recorder rejects any path outside that tree. Assert the
private /dev has no tpm device and the production state path is inaccessible; an accidental
production constructor must fail before TPM access. Host TPM and unrelated host cgroups are preserved; only the owned transient test cgroup is created/removed.

CI explicitly installs pinned tools and grants the runner transient-unit setup; local callers
without that prerequisite receive a failing diagnostic, not a skipped successful target. Required
flow/owner/preflight fixtures must execute. Cleanup stops only the owned test unit/emulator,
waits for its processes and removes its fixture/staged binary; no shared lab VM or hardware TPM.
The Unix socket recipe follows [swtpm0.9.0 options](https://raw.githubusercontent.com/stefanberger/swtpm/v0.9.0/man/man8/swtpm.pod).

## Planned regression fixtures and evidence

- Test stream adapter: one-byte/fragmented10-byte header and fragmented response body produce
  the same complete recipe result as an unfragmented response; short writes are completed.
  EOF in header/body, declared lengths below10/above65536, bad tag, zero-progress write and
  timeout fail/close without another command on that connection. Concurrent Send calls remain
  serialized; byte-stream fragmentation cannot leak stale reply bytes into the next command.

- Identity: material-derived version, fixed static kind/id, unknown128-byte/UTF8 identities
  refused, pin unchanged, no rotation; gRPC Unavailable advances and ResourceExhausted stops.
- Canary: cross-holder/recovery with stored Binding; changed id/epoch/profile/source/digest,
  short/equal-halves key and tampered envelope fail; no plaintext DEK or KEK in kept artifacts.
- Recipe: exact primary/public/policy vectors; PCR command order, wrong parent/TPM/PCR,
  permissive object bits, trial/password/empty-auth bypass and unencrypted session rejected.
  Empty existing owner auth boots without external input; nonempty owner auth/profile/input
  refuses before publication and leaves that hierarchy authorization unchanged.
- Owner: blocked read/write/close, timeout racing success, late bytes after deadline, failed
  exec/preflight, busy lock and restart with old owner alive; early flock release while live PID persists,
  torn state/foreign boot/unknown process and PID reuse; one TPM owner, no late KEK accepted.
- Unit/memory: rendered TPM-only device policy and sealed credential/sentinel; DynamicUser
  reads valid bundle and tpmrm0 but cannot open a raw disk; missing device/bundle, bad memlock,
  blocked syscalls and absent profile leave unencrypted RPCs working without reading TPM-source material; baseline TLS/token startup is unchanged.
  Assert kernel crash capture is off and every exec child repeats preflight.
- CLI/candidates: source preflight before every KEK/unlock/canary input, malformed/oversize/nonpipe/timeout input;
  crash/write/fsync/RENAME_NOREPLACE/directory-fsync cuts, concurrent same-name candidates,
  existing/active destination preserved, explicit selection and no implicit fallback/deletion.

Primary references checked2026-11-19: [go-tpm templates](https://raw.githubusercontent.com/google/go-tpm/v0.9.8/tpm2/templates.go),
[sessions](https://github.com/google/go-tpm/blob/v0.9.8/tpm2/sessions.go),
[sealing tests](https://raw.githubusercontent.com/google/go-tpm/v0.9.8/tpm2/test/sealing_test.go),
[device transport](https://raw.githubusercontent.com/google/go-tpm/v0.9.8/tpm2/transport/linuxtpm/linuxtpm.go),
[Linux6.6 TPM I/O](https://raw.githubusercontent.com/torvalds/linux/v6.6/drivers/char/tpm/tpm-dev-common.c),
[systemd execution](https://www.freedesktop.org/software/systemd/man/254/systemd.exec.html),
[device/resource controls](https://www.freedesktop.org/software/systemd/man/254/systemd.resource-control.html),
[mlock](https://man7.org/linux/man-pages/man2/mlock.2.html).
