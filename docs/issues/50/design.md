# Issue #50: Confidentiality-only encrypted-volume foundation — design

Approved 2026-11-13; not implemented. [Spec](spec.md), [intent](intent.md),
[exact contracts](contracts.md), [proposed ADR](adr-proposal.md).
**#50 stays open** for Vault/KMIP, rotation, crypto-erasure and authenticated integrity.
Revised after approval in plan review rounds 1 to 4 ([decisions](decisions.md)).

## Policy boundary before implementation

The existing spec explicitly excludes malicious tampering/replay protection. Audit proposed
ADR 0009 requires authenticated identity/address/version binding for any profile claiming
malicious-replica protection, and retained-key lifetime across snapshots/clones/backups.
This draft proposes **xts-aes256-v1 confidentiality only**, never that stronger guarantee.
A compromised replica can alter/replay ciphertext or inject zero sentinels; CRC is not a MAC.
Do not advertise this profile as satisfying the architecture's compromised-replica threat.
Before implementation, explicitly settle this restricted profile versus a superseding
integrity-bearing format/spec. No authenticated format bits or read-time freshness are invented.
Settled at approval, 2026-11-13: the restricted confidentiality-only profile (see decisions.md, Settled at approval).

The facts it starts from:
- No encryption/key field or provider exists. CSI parseParams rejects encryption
  (`control/csi/controller.go:157`); Header/Volume contain geometry/replication only.
- ublk copies writes into shared aligned buffers; local/remote read pieces converge at frontend
  completion (`flaminio-io/src/ublk.rs:1244–1274`). Zero-copy local reads can bypass process
  memory (`:1464,1508`) and therefore must be disabled per encrypted device.
- Stores compute their own CRCs from received bytes; Frame::Write has no end-to-end checksum.
  Sparse, zero and partial-cluster holes return stored zero blocks. Repl remains opaque bytes.
- FirstEpoch is the create-claim revision, known after the claim's transaction and before the
  publication's (`meta/service/service.go:415`, `:465–468`).
- The header is JSON decoded leniently (`service.go:275–291`) and written back whole by Attach
  and DeleteVolume (`:723–725`, `:541`): an instance without this change drops a field it does
  not know. A header that does not decode fails every call on its volume but DeleteVolume,
  which still removes it (`:534–539`), and ListVolumes skips it (`:853`); the decode error
  answers `Unavailable` (`errStatus`, `:151`), so a client tries the next instance.
- GetVolume is broadly readable; Release needs a separate io/current-target authorization. With
  authentication off (`TrustDomain` empty, only with `--insecure`) `Authorize` passes every call
  with no identity (`auth.go:42–44`).
- Both metadata clients move to the next instance on `Unavailable`, `DeadlineExceeded` and
  `Cancelled` (`flaminio-metaclient/src/lib.rs:145–158`, `control/csi/meta.go:63–80`), and check
  only the CA and the address (`lib.rs:85–92`, `pkg/pki/pki.go:300–310`), until #16's S15.
- Every attach path reaches the engine's `Input::Attach`: the socket's `attach` line
  (`control.rs:84`) and the startup `--attach`/`--token` (`ublk.rs:731–770`); #79's and #83's
  lines reach `Engine::start_target` too.
- The metadata service is a `DynamicUser` unit that gets its keys as systemd credentials
  (`scripts/guest/meta.sh:20–36`); a `LoadCredential=` whose path is missing fails its start.

## Creation, identity and old-client refusal (S1–S4, S12)

**The header's mark and the key record.** An encrypted volume's header writes `id` as an
object, `{"name": "<id>", "profile": "xts-aes256-v1"}`, where a plain header has today's
string; every other field is as today, and a plain header is byte for byte today's. Every
version before this one types `id` as a string, so it cannot decode an encrypted header
(Facts): it fails every call on the volume but its deletion, leaves it out of ListVolumes
(Rollout, below), and can neither serve the volume as plain nor rewrite its header. This version
decodes `id` in either form (`domain.Header`'s own JSON codec, a `Profile` beside `ID`), and
every header write it makes keeps the mark: Attach and DeleteVolume write back the header they
decoded, and #13's `fenced` too (whichever lands second keeps it). The claim header stays
unmarked until the publication marks it; with #76, the publication is #76's finalization, which
marks the header it publishes in the transaction that writes the key record, whether the creator
or a helper runs it (whichever of #50 and #76 lands second wires it). The wrapped key is in a
record of its own, `/flaminio/v1/volkeys/<id>/<first epoch>` (contracts.md, etcd records),
written with the header's publication. The mark and the record must agree, and
`readVolume` checks both (one Get, not on the I/O path): a plain header with a record at its
first epoch is `Internal`; a mark whose `name` is not the volume's id, or whose `profile` is not
`xts-aes256-v1` or not the record's, is `Internal`, never plain and never this profile. An
encrypted header without its record is read again: gone, deleting or at another first epoch is
`NotFound` (deleted since), else `Internal`. A record that does not parse is `Internal`. Never
plain. No revision is compared, so an unrelated header write (another Attach, #13's `fenced`)
changes nothing.

**Inputs.** `CreateVolumeRequest.encrypted`; the StorageClass parameter `encryption`
(`ParamEncryption` in `parseParams`, `controller.go:135–165`), exactly `"true"`/`"false"`,
default false; `flaminio-meta --encrypted create …`. A profile or descriptor sent in a request is
never read.

**Creation** (`createVolume`, `service.go:371–470`). An encrypted request first needs an
authenticated caller (`callerOf`) and a usable source on this instance whose identity is the
cluster's pin (below; else `INSTANCE_UNAVAILABLE`), then #73's namespace policy, which reads
`encrypted` (whichever of #50 and #73 lands second wires it): all before placement, the claim
and any key call. Under its claim, beside the leftover chunk records, it deletes any leftover
key record of the id (Deletion, below). Right before publication: 64 bytes from `crypto/rand`
(halves equal: drawn again), wrapped with `Binding{id, claim revision, profile}`, then cleared.
The publication transaction (`service.go:465–468`) writes the header, marked, and the key
record: one compare, two writes. A wrap that fails (any class of S6) undoes the claim before
it answers: `deleteChunks` under the claim's compare, then the `Creating` header deleted under
the same compare (a deletion that marked it meanwhile owns it); the id is free, and a retry
wraps a new key. Of #50 and #73, and of #50 and #76, whichever lands second wires the undo.
With #73 the undo adds no transaction of its own: the header and chunks go as above, under the
claim's compare, and then the creation's own owner record, now headerless, is released through
#73's existing release (#73 design, "DeleteVolume", its no-header branch): one transaction
comparing the header absent, the owner record, the chain's tail and the quota at the revisions
it reads, deleting `owners/<id>`, lowering the usage the claim charged and appending
`delete.committed` `header-missing` (no `deleting_op`, so admitted as any other event, in the
chain #73 gives that release). A failed compare, such as a concurrent creation in the same
namespace moving the quota, reads again and retries in the method's loop (`retries`); a full
chain, or retries spent, leaves the headerless owner record and its charge until #73's next
release of it succeeds, by this id's next creation (the creator's own, before its quota check,
which then answers Aborted) or a DeleteVolume. Either way the call answers the wrap's error.
With #76, which alone removes a creating header (its rollback, with its root), there is no
undo: the wrap is at #76's finalization (below), and a failed one leaves the operation to a
retry, a helper or a DeleteVolume, as any failed finalization step does. A crash or an etcd
error between the claim and the publication leaves today's hidden `Creating` header and no
record, as for any creation until #76 resumes or removes it. One crash point and no extra
revision.

- **With #76.** `encrypted` joins #76's canonical create identity (`docs/issues/76/spec.md`,
  Terms), persisted with the operation; its finalization transaction is this plan's publication:
  it writes the header marked and the key record, and a helper wraps its own key there (the
  conditional finalization lets one win). Once published, #80's `CheckCreateVolume` compares
  `encrypted` as "every future non-capacity identity field" (`docs/issues/80/contracts.md`),
  taken from the key record. Whichever lands second wires it.
- **#80's bound.** The `Volume` fields add at most 415 bytes (contracts.md, Size); #80's
  worst-response fixtures gain a maximal descriptor. If the exact size then exceeds #80's
  `B`, #80's header allowance must grow before encrypted creation is admitted, a change to
  #80's plan made by whichever of #50 and #80 lands second.
- **Deletion.** DeleteVolume's tombstone transaction (`service.go:565–573`) deletes the key
  record its header names with the header: one write more. Within etcd's 120 with #13's and
  #73's additions. After its commit, beside its chunk records, and in its header-absent branch
  (`:526–529`), it lists `volkeys/<id>/` and deletes what is there, in batches conditional on the
  header being absent, as `deleteChunks` does; a creation does the same under its claim. This
  removes a record whose header did not decode, and one an older instance's deletion left (it
  cannot name the record: Rollout below) once the id is deleted or created again; until then
  that wrapped key of a deleted volume stays in etcd.

**Answers.** This version sets `Volume.encryption` (field 20) in every `Volume` it returns:
`{profile: "none"}` for a plain volume; profile, first epoch and wrapped key for an encrypted
one, with `key_source_storage_mode`. An answer without field 20 came from an older instance,
which answers only for a plain volume (the mark).

**The clients check the answer (S1, S12).**
- **CSI controller.** On `OK` the answer's `encryption.profile` must be the request's (`none` or
  `xts-aes256-v1`); an answer without field 20 to a plain request is accepted, as an older
  instance made the plain volume asked for. Otherwise (an older instance dropped `encrypted`,
  `controller.go:231`) it deletes the volume this call created (`c.Meta.DeleteVolume`, as
  `controller.go:265`) and answers `FailedPrecondition`, as #75 does for its policy. On
  `AlreadyExists` the profile joins the compared properties (`controller.go:239–247`), an answer
  without field 20 comparing as `none`: a plain volume of the same geometry under an encrypted
  request is `AlreadyExists` "other properties", nothing deleted.
- **`flaminio-meta create`.** The same comparison; on a mismatch it deletes nothing and exits 1
  naming the volume.
- **CSI node plugin.** An `Attach` answer without field 20 (`control/csi/node.go:70–74`) is
  refused, `FailedPrecondition` `an older metadata instance answered for VOLUME`, before the
  data plane is called, so the token reaches no data plane on that answer.
- **The data plane.** Every `GetVolumeRequest` it sends carries `accepted_profile =
  "xts-aes256-v1"`: in #13's one constructor `get_volume_call` (`docs/issues/13/design.md`, One
  `GetVolume` constructor), or in today's two requests (`engine.rs:668`, `:1782`), whichever
  lands second. At attach, field 20 absent: `AttachFailed` `an older metadata instance answered
  for VOLUME`; `none`: today's path; `xts-aes256-v1` with `first_epoch` equal to the token's:
  the release (below); anything else: `AttachFailed`. The refresh ignores the field.

**The service's gate (S12).** `GetVolume` refuses (`FailedPrecondition`) an `io` caller whose
node is the record's target and whose request lacks the profile, for an encrypted volume. Attach
moves the target before the data plane reads the volume (`control/csi/node.go:70–74`), so an
older data plane's attach fails there. Every other caller gets the record: administrators,
agents, and replica-side reads by nodes that are not the target (#13's sweep and lookups), which
an older data plane ignores, as replicas need nothing (S17).

**One decision for every attach path.** The header's mark and the key record decide (they must
agree), never the caller: the check above is in the engine's attach (Where the code goes,
below), which the socket's `attach VOLUME TOKEN-BASE64 [MS]`, the startup `--attach`/`--token`,
#79's `attach-init` and #83's handoff lines all reach. No line, verb or operand is added, so `[MS]`
and #6's deadline stay, and an older plugin's line works on a new daemon.

**Rollout and rollback (S12).** Two rollouts, in this order; nothing enforces it, as for #75's
`--qos`. First the binaries: every metadata instance before any data plane, CSI node plugin,
CSI controller or `flaminio-meta`, since those refuse an answer without field 20 (a plain
creation's excepted), while a new instance answers older clients as today for a plain volume.
Second, once all of them run this version, the key source: `serve --kek-file`, the switch. A
rollback goes in the reverse order. Below this version it is unsupported while an encrypted
volume exists: the only safe rollback deletes the encrypted volumes first, with this version
(which deletes their key records). On every call on an encrypted volume an older instance fails
closed, whatever it is started with: it cannot decode the mark, so it serves none and only
deletes one; its errors are `Unavailable`, so a client tries the next instance, which serves
the volume if it runs this version. Its listings do not fail closed: its ListVolumes leaves out
every encrypted volume and answers `OK` (`service.go:851–854`). `drain.Step` visits only the
listed ids (`control/drain/drain.go:50–63`, used by `flaminio-meta drain`, `main.go:433–440`),
so a drain through an older instance reports a node empty while it still holds encrypted
replicas; and an older instance's deletion leaves the key record (Deletion) and, with #72,
releases no capacity (a header that does not parse). So while an older instance serves and an
encrypted volume exists, no drain or node removal is safe. An older binary also exits on the
unknown `--kek-file` (`flag.ExitOnError`, `meta/cmd/flaminio-meta/main.go:84`). The flag's usage
and the docs say so. In every mix: an older instance serves no encrypted volume and cannot
rewrite its header or erase its key record; a new data plane, node plugin, CSI controller and
`flaminio-meta` refuse an older instance's answer (a plain creation's excepted); a new instance
refuses an older data plane (the gate). Round 1's named risk, an older instance serving an
encrypted volume as plain to an older data plane (whose node plugin would format the device,
`control/csi/node.go:107–116`), is closed by the mark (decisions.md). **Named, not accepted:** a
rollback against this rule, an older instance serving while an encrypted volume exists, lets a
drain of any version retire a node that holds that volume's replicas (all of them at RF 1).

## Source, wrapping, release and failures (S2, S3, S5–S10)

Go `pkg/keysource`: the KeySource, identity, envelope and error contract of contracts.md.
**The static source.** `flaminio-meta serve --kek-file PATH`, refused at start with
`--insecure` (no authentication: S5, S7) and for any other kind. It opens PATH with
`O_NOFOLLOW` and needs a regular file of exactly 32 bytes, read once at startup; anything else
leaves the instance without a usable source, logged once, every other method served. Owner and
mode are the installer's to check (the lab: Lab below), since the service reads a systemd
credential copy, not root's file. A missing or unusable source refuses only encrypted creation
and release (`INSTANCE_UNAVAILABLE`). Status: `storage_mode=file-static`, the weaker kind; the
start line and `key_source_storage_mode` say so, and the start line adds that a node running a
metadata instance holds the KEK and every wrapped key. No line ever holds KEK or DEK bytes.
#57 may supply a boot-unsealed KEK with `storage_mode=tpm-sealed-static` and the same identity;
this is explicit material loading, not decrypt aliasing.

**The cluster's KEK.** The identity derives from the KEK (contracts.md). Before the claim of an
encrypted creation an instance reads `/flaminio/v1/keysource`: absent, it creates it with its
identity (compare: absent), and a create that fails (another instance's won) reads it again;
present and different, it has no usable source (`INSTANCE_UNAVAILABLE`), claims nothing and
wraps nothing. The pin never changes in this step, so a match is cached for the instance's
life. An unwrap whose envelope names another identity is `INSTANCE_UNAVAILABLE` too, so the
caller tries an instance that holds that KEK; a replaced KEK fails loudly at creation instead of
orphaning the volumes made before it.

**Wrapping** is AES-256-GCM with a random 12-byte nonce and the AAD of contracts.md. A wrong
binding, KEK or flipped bit refuses; no unauthenticated fallback. Future Vault/KMIP implement
the same binding/error interface only after #60's conformance. Calls get a 1 s context; providers
must honor cancellation and not leave hung goroutines holding DEKs. The static source does no
file I/O per call. No bound on calls in flight in this step (Q12). Codes as contracts.md:
`INSTANCE_UNAVAILABLE` is `Unavailable` (the next instance is tried); `SOURCE_UNAVAILABLE` is
`ResourceExhausted`, on which neither client moves on, so the other instances are not asked in
turn; `DENIED` and `INVALID_ENVELOPE` stop.

**ReleaseVolumeKey** (`auth.go` rule in contracts.md). The handler refuses a call with no
authenticated caller (`callerOf`, as `ChangeIsr` does not, `service.go:801–804`), then reads the
header and the key record: caller's node not the target, or `epoch` not the record's, is
`DENIED` before any key call; a plain volume is `FailedPrecondition`. It unwraps, then reads the
header again: absent or `Deleting` (`readHeader`'s `NotFound`), or its target, epoch or first
epoch not the ones checked (`DENIED`, as a new call would get), it clears the DEK and refuses;
any other header write (#13's `fenced`) changes nothing. A successful release linearizes at that
check; a later target change cannot recall a key already held. It seals to the recipient
(`crypto/hpke`, contracts.md) and clears the DEK. No record changes. Each release is logged with
caller, volume, epoch and outcome; refusals at most 10 lines a minute per caller identity, the next
line counting those dropped (#73's limiter once it lands). The audit events of #73's chain are a
follow-up (below).

## Protected delivery and memory (S7–S11)

Ordinary protobuf/tonic clear-DEK responses create copies outside locked key memory. The release
therefore carries an opaque RFC 9180 HPKE envelope inside mutual TLS, to the target's fresh
per-attach recipient key; contracts.md fixes the suite and the binding. Transport buffers hold
ciphertext; the data plane opens directly into locked memory. It builds `info` from its verified
token and its own request, never from the response, whose `profile` and `first_epoch` must
match, or the attach fails (S9). **Prerequisites,** landing before this change: #16's S15, both
clients accepting only a `meta` URI SAN, since otherwise an `io` key at a metadata address could
answer the release with a key of its choosing, or `GetVolume` with no descriptor
(`docs/issues/74/design.md`, Risks); and #15's token verification in `Input::Attach`
(`docs/issues/15/design.md`, S11), since today the engine decodes the token unverified
(`decode_unverified`, `engine.rs:1610–1616`). A wrong token value fails the attach even so: the
service takes first epoch and profile from the key record and node from the certificate, so an
`info` that differs opens nothing.
Settled at approval, 2026-11-13: HPKE inside mutual TLS, as proposed (see decisions.md, Settled at approval).

**Where the code goes.** `flaminio-uring/src/secret.rs`, under its reviewed unsafe boundary:
dedicated mmap pages, mlock, `MADV_DONTDUMP`, checked failure cleanup, volatile zeroization
before munlock/unmap. `flaminio-node/src/crypt.rs`: `VolumeCipher` (XTS over a locked key),
`seal(offset, &mut [u8])`, `open(offset, &mut [u8])` (skipping complete zero blocks), the
per-attach recipient and the release's open. `flaminio-node` gains `flaminio-uring` and the
cipher crates as dependencies.

**The engine's attach.** `start_target` is synchronous, and its caller pushes `Output::Attached`
and drains at once (`engine.rs:1491–1506`, `:1596`), so it is split. When `GetVolume` and
`ListNodes` are both in, today's checks run (target, geometry, token: `engine.rs:1596–1621`),
then field 20's decision (above); only then is the target built and started.
- **`none`:** started as today.
- **`xts-aes256-v1`:** the attach stays in `attaching`, holding a fresh recipient key in locked
  memory, and sends `Call::ReleaseVolumeKey` through its `MetaPort` under a third part of
  today's wait, `MetaWait::Attach { volume, part: 2, attempt }`, fenced by `attempt` as the other
  two (#6). An answer for an attach dropped since (`Input::Detach` removes it, `engine.rs:1858`,
  and its recipient key with it) is never opened and starts nothing. A refusal ends the attach
  (`AttachFailed`, naming the class). An answer that opens builds the `VolumeCipher`; then the
  target is started and `Output::Attached` pushed. No target exists before the answer.
- **With #15,** whose engine holds the checked target unstarted until `Input::Marked`
  (`docs/issues/15/design.md`, S11): the release comes after the checks and before #15's
  `Output::MarkEpoch`, its wait fenced by the generation #15 gives the attach; the mark is
  written only for an attach that holds its key, and a `Detach` of that generation drops the
  held target and the cipher together.

The engine holds the cipher until the frontend takes it with `Engine::take_cipher(volume)` after
`Output::Attached` (`Output` is `Clone`, so it never carries the key); an attach that fails or is
dropped drops it. The data plane sets `RLIMIT_CORE` 0 and `PR_SET_DUMPABLE` 0 before a release.
No Debug, Clone or serialization of raw key; output types print a redacted identity only. Cipher
and HPKE expanded secrets and initialization temporaries must be covered, not just the 64-byte
DEK; Box placement after construction does not prove stack copies erased. A secret-lifetime
audit and the library choice are merge blockers, not promises this draft verified. **The
metadata service** clears DEK buffers on every exit of a wrap or release; with a key source it
sets `PR_SET_DUMPABLE` 0 at start, before it reads the KEK file (`golang.org/x/sys/unix`), and
the lab unit has `LimitCORE=0` (S11). Go GC copies still need the review (ADR, Consequences).

**Revocation.** The cipher is a field of the device's `Shared` (`ublk.rs:506`). `run_target`
returns only when every queue thread has ended, then `Arc::try_unwrap(shared)` succeeds
(`ublk.rs:1050–1057`); a deposition stops the device (`:619–629`) and reaches the same point;
`attach_and_serve` then detaches (`:888–895`). Dropping `Shared` drops the cipher, which zeroizes
its locked memory, on every exit of `run_device`, an early error included. No admission flag,
borrow count or wait. With #15, the cipher is a field of that device's own `Shared`, dropped
when its generation ends. An encrypted device is not added before the cipher is held; a lock
failure is an attach failure.

Proposed library candidates: RustCrypto `aes` 0.9.4 + `xts-mode` 0.6.0 for XTS and `hpke`
0.15.1 for Rust; Go's standard `crypto/hpke` (`go.mod`: go 1.26, toolchain 1.27.3). Upstream
[aes](https://docs.rs/aes/latest/aes/) describes runtime hardware dispatch;
[xts-mode](https://docs.rs/xts-mode/latest/xts_mode/) explicitly lacks an independent audit.
The Rust candidates are not approved or pinned here: conformance, secret-memory review and
comparison with an FFI backend settle Q2 before implementation.
Settled at approval, 2026-11-13: Q2 as recommended: measure first; these stay candidates until then (see decisions.md, Settled at approval).

## Data boundary, sparse zeros and zero-copy (S13–S17)

The frontend's tag task (`engine_io_task`, `flaminio-io/src/ublk.rs`) seals each 4 KiB block of
the write's aligned copy before `Io::Write` reaches replication: between `AlignedBuf::copy_of`
and `Bytes::from_owner`, which are one call today (`shared_copy_of`, `ublk.rs:1263`;
`flaminio-uring/src/aligned.rs:56`), since the `Bytes` it returns is immutable. Tweak is the
little-endian 128-bit volume-logical block number, never chunk-relative, an epoch or a physical
offset. XTS uses two independent 256-bit AES keys (the
64-byte DEK). Application zeros encrypt normally. Copies, repairs and reconciliation move stored
bytes inside the engine and never pass the frontend. Prevent double encryption with typed
Plain/Stored client buffers; encrypted entries reject untyped/plain bypasses.

Reads verify stored CRCs first, as today, then are opened once at completion, after the fast
copied read (`fast_read_copy`, `ublk.rs:1629`), the slow local read and remote pieces
(`ublk.rs:1244–1274`, `:1400`) have filled the buffer, mixed cross-chunk reads included. Any
complete stored 4 KiB zero block skips XTS and returns zeros; partial-cluster holes, `Discard`
and repair zero blocks keep their digest semantics. Written all-zero ciphertext has the spec's
accepted negligible risk; this sentinel is not integrity protection. On failure ciphertext is
never completed as success.

**Zero copy (S16).** A pure `device_mode(encrypted, zero_copy, no_verify) -> DeviceMode {
zero_copy, verify, log_fallback }` in `ublk.rs`: an encrypted device is added without the
zero-copy flags (`add_device`, `ublk.rs:906–910`), served by `engine_io_task` (`serve_with`,
`:1098`), and verified whatever `--no-verify` says; it logs the fallback once per attach when
the node runs `--zero-copy`. No staged decrypting variant of `engine_io_task_zc`. Plain devices
keep the flags. No change to Frame, the store format, maps or CRCs, Digest/Mapped/Discard or
repl; deletion is ordinary deletion, not erasure. No key deletion API. Future #51/#53 references
must pin the original wrapped material and source version before live-header deletion; do not
ship retained encrypted snapshots or backups without that catalog/GC contract. A newer
authenticated profile needs an explicit incompatible format/identity transition, not a
reinterpretation of this profile.

## Files and rule-to-test mapping

Each test fails without the change it names, but `TestSourceUnavailableIsNotCycled`, which
guards today's client behaviour that S6 relies on; the mapping it needs is `TestKeyErrorCodes`'s.

| Rules | Tests (file: case) |
|---|---|
| S1, S2, S4 | `meta/service/encryption_test.go`: `TestCreateEncryptedVolume` (key record at its key, envelope bound to id and first epoch, the claim header unmarked and the published header's `id` the object `{"name","profile"}`, field 20 answered), `TestHeaderWritesKeepMark` (after an Attach, the deletion mark and, with #13, `fenced`, whichever lands second: the header's `id` still the object; with #76, after a finalization by the creator and one by a helper: the published header's `id` the object and the key record written), `TestCreatePlainVolumeUnchanged` (records byte for byte today's, no source call, `{profile: none}`), `TestCreateEncryptedWithoutSource` (`Unavailable`, nothing written), `TestCreateEncryptedCrashPoints` (failure at the claim, a chunk batch and the publication: never a visible volume without its record; a wrap that fails: the claim undone, the id free, a retry creates the volume with a new key; with #73, whichever lands second: the owner record deleted, the charge released, `delete.committed` `header-missing` appended; a failed wrap racing a second creation in the same namespace: the release retries and the usage stays exact; the chain full: the owner record and its charge left, once the chain has room, a retry releases them and answers `Aborted`, the next creates the volume; with #76: no undo, the operation left, and a retry or a helper publishes the volume with a new key), `TestDeleteRemovesKeyRecord` (with the header; a header that does not decode; an older instance's deletion, header gone and record left, then DeleteVolume again, and separately a re-creation: no record of the id left but the new one's). `control/csi/csi_test.go: TestControllerEncryption` (`"true"`, `"false"`, `"yes"` refused; a fake service ignoring `encrypted` gets the volume deleted and `FailedPrecondition`; the same fake answering a plain request without field 20: created; a plain volume of the same geometry under an encrypted request is `AlreadyExists`, nothing deleted). `control/csi/csi_test.go: TestNodeRefusesOldInstanceAttach` (an `Attach` answer without field 20: `FailedPrecondition`, the fake data plane not called). `meta/cmd/flaminio-meta/main_test.go: TestCreateEncryptedFlag` (an answer without field 20: exit 1, no delete, for `--encrypted`; success for a plain create). |
| S1 (#73) | `meta/service/encryption_test.go: TestEncryptedCreateFeedsTenantPolicy`, with #73: under `allow_unencrypted=false` an encrypted creation passes, a plain one is refused, with no source call. |
| S3, S5, S6 | `pkg/keysource/static_test.go`: `TestStaticRoundTrip`, `TestStaticRefusesWrongBinding` (id, first epoch, profile, version, each bit of `Sealed`), `TestStaticKEKVersionDerived`, `TestStaticFileChecks` (symlink, 31 and 33 bytes, a directory: no source), `TestDEKHalvesDiffer`, `TestKeyErrorRedacted`, `TestSourceHonorsDeadline` (a hanging fake ends at 1 s, `SOURCE_UNAVAILABLE`). `meta/service/encryption_test.go`: `TestKeySourcePin` (two instances with different KEKs: the second refuses to wrap with `Unavailable`; a release through it is `Unavailable` and through the first succeeds; a replaced KEK refuses creation; two instances with different KEKs reading the pin absent at once: one creates it, the other reads it again and refuses before its claim, nothing written), `TestKeyErrorCodes` (each class's code and `ErrorInfo`). `control/csi/meta_test.go: TestSourceUnavailableIsNotCycled` (two fake instances, the first answers `ResourceExhausted`: the second gets no call). `meta/cmd/flaminio-meta/main_test.go`: `TestServeRefusesInsecureWithKEK`, `TestServeWithKEKIsNotDumpable` (`PR_GET_DUMPABLE` in the started server's process is 0). |
| S7, S8 | `meta/service/encryption_test.go`: `TestReleaseVolumeKey` (administrator, agent, another node's `io`, a deposed epoch: refused before any source call, counted by the fake; the target's `io`: opened with the test's recipient to the DEK; every revision unchanged), `TestReleaseTargetChangesDuringUnwrap` (a hook moves the target after the unwrap: `DENIED`, nothing sealed; a hook rewrites the header after the unwrap with target, epoch and first epoch unchanged: released), `TestReleaseUnauthenticated` (`TrustDomain` empty: the release and an encrypted creation refused), `TestReleaseRefusalLogBounded` (100 refusals from one caller: at most 10 lines and a count). `pkg/keysource/testdata/release-vectors.json`, read by Go `TestReleaseVectors` and Rust `crypt.rs: release_vectors`. #16's S15 tests (an `io` server certificate at a metadata address refused) and #15's token verification tests are prerequisites. |
| S9–S12 | `flaminio-node/tests/engine.rs` (fake `MetaPort` sealing with the Rust HPKE library): `encrypted_attach_without_key_fails` (each S6 class names itself in `AttachFailed`; no target, no I/O), `old_instance_answer_is_refused` (no field 20), `release_for_another_first_epoch_is_refused`, `cipher_dropped_on_detach_deposition_and_failure` (a drop hook), `no_key_call_after_attach` (the release refused from then on: I/O, an ISR change and a resync go on), `release_answer_after_detach_starts_nothing` (a `Detach` while the release is out: its answer is not opened, no target, no `Attached`), `no_target_before_release_answer`. `meta/service/encryption_test.go`: `TestOldDecoderRefusesEncryptedHeader` (a copy of today's `Header` fails to decode the header the service wrote for an encrypted creation, and decodes the one it wrote for a plain creation as today; DeleteVolume's branch for a header that does not decode, kept from today, removes it), `TestKeyRecordDamaged` (a record that does not parse, an encrypted header without its record, a plain header with a record at its first epoch, a mark whose `name` is not the id or whose `profile` is not `xts-aes256-v1` or not the record's: `Internal`, never plain; a header write that changes neither target nor first epoch, between the header's read and the record's: answered), `TestGetVolumeGate` (target `io` without the profile refused; a replica-side `io` caller, an administrator and a plain volume answered). `flaminio-uring/src/secret.rs`: `debug_is_redacted`, `zeroized_before_unmap`, `lock_failure_is_an_error`. |
| S13–S15 | `flaminio-node/src/crypt.rs`: `xts_ieee1619_vectors`, `tweak_is_the_volume_block_number` (block 0, across a chunk boundary, the highest block), `zero_block_opens_to_zeros`, `partial_zero_block_is_decrypted`. |
| S13–S17 | `flaminio-node/tests/engine.rs`, RF 2, memory devices, writes sealed and reads opened by `crypt.rs` as the frontend does: `encrypted_pattern_is_on_no_device`, `encrypted_replicas_hold_equal_bytes_and_zero_digests`, `encrypted_reconcile_and_resync_pass`, `second_engine_reads_the_same_plaintext` (a fresh epoch on the other node). Injected bugs, each caught by these: `injected_chunk_relative_tweak_is_caught`, `injected_zero_block_decryption_is_caught`, `injected_plaintext_write_is_caught`. |
| Excluded threats (ADR) | `flaminio-node/tests/engine.rs`: `tampered_ciphertext_is_not_detected` (a stored block replaced and its CRC recomputed reads back as other bytes, no error), `replayed_block_reads_old_plaintext`, `injected_zero_block_reads_zeros`: they show what the profile does not protect, and fail if a read ever reports protection. |
| S16 | `flaminio-io/src/ublk.rs: device_mode_table` (the eight combinations: zero copy only for plain with the flag; encrypted always verified; the fallback logged for encrypted with the flag). The rest in the lab. |
| S17 | `make sim` and the format and transport tests pass unchanged; no replication safety rule is added, so no simulator check or injected bug is owed. |
| S18, S19 | `scripts/tools/encryption-test.sh` (in `make lint`, VMs faked): `pki init` makes `kek` once (32 bytes, 0600; never again); the install reaches fl-a, fl-b and fl-w only, over ssh's stdin, and no recorded argv or `ENV_VARS` holds the KEK's bytes, base64 or hex; a second, different workstation KEK is refused and the VM's `kek` unchanged; a 31-byte workstation `kek` is refused before any VM file is written, CA files included; a second VM whose `kek` differs is refused before any VM is changed; the remote step writes a `mktemp` file in the directory, checks 32 bytes and syncs it before the `ln`, and removes it on every path (a short file, a link that fails, a difference, success); a different `kek` appearing on the VM after the compare-only pass: refused, `kek` unchanged; a state dir with CAs and no `kek`: `pki issue c` and then `pki issue` both succeed, the `kek` made once and installed on the metadata VMs only; `meta.sh up` writes `LoadCredential=kek` and `--kek-file` only when `sudo test -f` finds the file, and `LimitCORE=0`; `vol-create ENCRYPT=1` passes `--encrypted`; the check's steps. |
| S20 | Contracts, ADR and docs conformance; `make proto contracts`. |

Settled at approval, 2026-11-13: the ADR, profile and library decisions (see decisions.md, Settled at approval).

Touched: `pkg/keysource`; `meta/domain`, `meta/service` (`service.go`, `auth.go`, the key
record), `api/proto` and `api/gen`, `meta/cmd/flaminio-meta`; `control/csi` (parameters, the
answer checks); `flaminio-metaclient` (`pb.rs`, `lib.rs`); `flaminio-uring/src/secret.rs`;
`flaminio-node` (`crypt.rs`, the engine's attach, `Cargo.toml`); `flaminio-io/src/ublk.rs`; lab
scripts, the Makefile's help and the docs. Implementation checks: `make lint test race hygiene
contracts sim`, and the sanitizers on the new unsafe module; no test ran while drafting.

**Follow-ups** (`docs/followups.md`, added with the change, as #73 asks): the audit events of key
wrap and release through #73's chain, owned by #50's later steps (issue to be filed).

## Lab, docs, risks and ADRs

**The KEK** (S18). `lab.sh pki init` makes `$LOCAL_STATE_DIR/pki/kek` once, `umask 077`,
32 bytes of `/dev/urandom`, never again while it exists. `lab.sh pki issue` runs `pki init`
when `flaminio/ca.crt` or `kek` is missing (today only for the CA, `scripts/lab.sh:574`; `init`
makes each piece only if missing), so a lab whose CAs predate this change gets its `kek` from
its next `pki issue`, `issue w` and `issue c` included. It installs the `kek` on the metadata
VMs only (fl-a, fl-b, fl-w), over ssh's stdin (`vssh VM "sudo sh -c '…'" <kek`), never
`put_file`, whose base64 rides in `ENV_VARS` on the command line (`scripts/lab.sh:556–559`).
A run that reaches a metadata VM, before it writes anything to any VM, CA files included,
checks that the workstation's `kek` is exactly 32 bytes, then compares it with the `kek` of
each metadata VM the run reaches (`cmp -s` under `sudo`, through stdin; a VM without one
matches); a run that reaches none (`issue c`) does neither. A short file stops it naming the
file; a difference stops it naming the VM and saying the workstation's KEK is not the
cluster's; either way no VM is changed. Then, VM by VM, the remote step, as root under `umask
077` in `/etc/flaminio/pki`, writes a `mktemp` file there, checks `wc -c` is 32, `sync`s it,
and links it as `kek` with `ln`, which fails if `kek` exists; then it compares the file with
`kek`: different, it fails, `kek` unchanged. On every path, success or failure, it removes the
`mktemp` file. A workstation that lost its state dir, or another one, cannot replace the
cluster's KEK, nor can two first installs of different KEKs both succeed. Then `stat` must say
root 0600. In `pki` only, never #12's `io-pki` (#16).
`meta.sh up` writes `LoadCredential=kek:/etc/flaminio/pki/kek` and `--kek-file
${CREDENTIALS_DIRECTORY}/kek` only when `sudo test -f /etc/flaminio/pki/kek` succeeds (the directory
is root 0700, `scripts/guest/pki.sh:10`, and `meta.sh` runs as the ssh user), so an instance without
it starts and has no source; it adds `LimitCORE=0`.

**The check** (S19): `make encryption-check`, `scripts/lab.sh encryption-check`, running
`scripts/checks/encryption.sh`. On an encrypted RF 2 volume (`make vol-create ENCRYPT=1`) and a
plain one, and the encrypted StorageClass deployed beside the plain one:
1. a unique known pattern written through the device is on no replica disk (a raw scan of each
   data disk), and both replicas' digests agree;
2. `scripts/checks/reconcile.sh` and `resync.sh` pass on it, and unwritten ranges read zeros;
3. after a failover the other node reads the pattern back;
4. the KEK moved aside on every metadata VM and `meta.sh up` run again (removing the file alone
   does not revoke a KEK a live instance read): a restarted data plane's attach fails with
   `INSTANCE_UNAVAILABLE`; the exact file back and `meta.sh up`: the attach works at a fresh
   epoch and the pattern reads back. Never a replacement KEK;
5. a node in direct mode (no `--privsep`, the only mode where `--zero-copy` works, #74's S7)
   run with `--zero-copy`: the pattern reads back, and S16's line is logged.
Cleanup removes only its own volumes; the KEK stays, root-only; no crypto-erase.

**The cost.** `make bench-reads MODE=verify VOL=…` on the encrypted and the plain volume, and
`make bench-writes` with `ENCRYPT=1` against it without, under the performance plan's
repetitions; latency, throughput and CPU recorded under `docs/benchmarks/` with whether the
guests show the `aes` CPU flag. No threshold, no claimed result.

Docs align checksum ownership, sparse zeros, the weaker static source, HPKE and locked memory,
the two rollouts' order, the rollback that deletes the encrypted volumes first (an older
instance lists none, so no drain is safe), and the lack of malicious-replica/replay
protection. The protected-delivery and memory audit is an explicit gate. The local proposal
changes no accepted ADR. Go keysource/metadata and Rust adapters can proceed against exact
contracts after policy/library acceptance; security review and real frontend conformance must
precede encrypted production admission.
Settled at approval, 2026-11-13: Q1–Q12 as recommended; the issue-local ADR is accepted with the plan (see decisions.md, Settled at approval).
