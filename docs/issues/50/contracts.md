# Issue #50 encryption/key contracts (Accepted with the plan, 2026-11-13)

Revised after approval in plan review rounds 1 to 4 (decisions.md). Every multi-byte integer
below is big-endian; `u16be len(x) ‖ x` is a string's length in bytes, then its bytes; `‖` joins.

## KeySource for #50/#57/#60

`SourceIdentity {Kind, ID, KEKVersion string}`, each 1 to 64 ASCII identifier bytes
(`[A-Za-z0-9._-]`). The static source: `Kind = "static"`, `ID = "static"`, and
`KEKVersion = "k1-" ‖ lowercase hex of the first 8 bytes of HMAC-SHA256(KEK, "FLKEKCHK001")`
(19 bytes): derived from the KEK's bytes, never a flag, so two instances with different KEKs
never share an identity. `Binding {VolumeID string; FirstEpoch uint64; Profile string}`;
profile `"xts-aes256-v1"`, 1 to 32 ASCII bytes. DEK 64 bytes, KEK 32 bytes; other lengths
refuse.

Generators: the KEK, by the installer (the lab: 32 bytes of `/dev/urandom`, Lab in the design);
the DEK, by the metadata service from `crypto/rand`, drawn again while its two 32-byte halves
are equal (XTS needs two different keys); the HPKE recipient, by the data plane from the OS
generator (`getrandom`). A released DEK whose halves are equal fails the attach.

`KeySource` has `Identity() SourceIdentity`, `Wrap(context.Context, Binding, []byte)
(Envelope, error)` and `Unwrap(context.Context, Binding, Envelope) ([]byte, error)`.
Returned plaintext is one caller-owned buffer, cleared on every exit; raw buffers never Debug,
log or serialize. Provider calls honor their context. #57 may load exactly the same KEK and
source identity at boot from a TPM sealed object, then use these methods without TPM I/O.
`storage_mode = file-static | tpm-sealed-static` is operational status, not a SourceIdentity
alias or an envelope decryption fallback.

`Envelope {Version uint32; Source SourceIdentity; Profile string; Sealed []byte}`. Version 1,
static wrapping: AES-256-GCM, `Sealed = nonce (12 random bytes) ‖ ciphertext of the DEK
(64 bytes) ‖ tag (16 bytes)`, exactly 92 bytes. The AAD, exactly:

```text
"FLKEY001" ‖ u32be Version ‖ u16be len(volume) ‖ volume ‖ u64be FirstEpoch
‖ u16be len(profile) ‖ profile ‖ u16be len(Kind) ‖ Kind ‖ u16be len(ID) ‖ ID
‖ u16be len(KEKVersion) ‖ KEKVersion
```

A later version keeps the label and changes `Version`, which the AAD covers. Provider
conformance (#60) must authenticate this binding even if later backend APIs differ.

`KeyError {Kind, Source, Volume, Cause}` with safe formatting (no key bytes). Kinds and codes:

| Kind | When | gRPC code | The client |
|---|---|---|---|
| `INSTANCE_UNAVAILABLE` | no source; an unusable KEK file; a KEK that is not the cluster's (below); an envelope whose `Source` is not this instance's identity | `Unavailable` | tries the next instance |
| `SOURCE_UNAVAILABLE` | the source is away or slow; the 1 s call bound ran out | `ResourceExhausted` | stops; tries later |
| `DENIED` | the source refuses; a caller that is not the target | `PermissionDenied` | stops |
| `INVALID_ENVELOPE` | same identity, and authentication fails or a field is malformed | `FailedPrecondition` | stops |

Each also carries `google.rpc.ErrorInfo {domain: "flaminio.keys.v1", reason: Kind, metadata:
{source, volume}}`; the message names the class. Retrying is decided by the code alone, as the
clients do today (`flaminio-metaclient/src/lib.rs:145-158`, `control/csi/meta.go:63-80`, which
move on only for `Unavailable`, `DeadlineExceeded` and `Cancelled`). Unknown typed failures fail
closed. Each provider call has a 1 s context, below the Go clients' 10 s per instance
(`control/csi/meta.go:21`) and the data plane's per-instance timeout. No bound on calls in
flight in this step (Q12: with step 2).

## etcd records

- **Key record** `/flaminio/v1/volkeys/<volume id>/<first epoch as %020d>`, JSON, unknown fields
  refused:
  `{"schema":1,"profile":"xts-aes256-v1","first_epoch":N,"envelope":{"version":1,"kind":…,
  "id":…,"kek_version":…,"profile":…,"sealed":"<standard base64 of the 92 bytes>"}}`.
  Written only by the transaction that publishes the volume; deleted by the transaction that
  deletes its header. A leftover (a header that did not decode; an older instance's deletion,
  in an unsupported rollback) is deleted by listing `volkeys/<id>/`: by DeleteVolume after its
  commit and when the header is absent, conditional on its absence, and by a creation under its
  claim. Until then it stays in etcd.
- **The header's mark.** An encrypted volume's header (`volumes/<id>`) writes `id` as
  `{"name":"<id>","profile":"xts-aes256-v1"}` instead of a string, which no older instance
  decodes; no new JSON key, and a plain header is byte for byte today's. Every header write of
  this version keeps the mark; the claim header is unmarked until the publication (with #76,
  its finalization, by the creator or a helper), which writes the mark and the key record in one
  transaction. The mark and the key record must agree, else `Internal`, never plain and never
  this profile: an encrypted header without its key record at its first epoch, a plain header
  with one, a mark whose `name` is not the volume's id or whose `profile` is not
  `xts-aes256-v1` or not the record's.
- **Source pin** `/flaminio/v1/keysource`: `{"schema":1,"kind":…,"id":…,"kek_version":…}`.
  Read before the claim of an encrypted creation; absent, created (compare: absent), and a
  create that fails is read again. Another identity: `INSTANCE_UNAVAILABLE`, before the claim.
  Never deleted or changed in this step.

## Metadata/protobuf

Tags (#13 reserves `Volume` 20-21 for #50; #72 takes no `Volume` tag, #73 takes
`CreateVolumeRequest` 30, #75 takes 40 in all three messages, #79 takes `CreateVolumeRequest` 9):

```proto
// CreateVolumeRequest: bool encrypted = 20;
// Volume:              EncryptionDescriptor encryption = 20;  // always set by this version
//                      string key_source_storage_mode = 21;   // encrypted volumes only
// GetVolumeRequest:    string accepted_profile = 2;           // "xts-aes256-v1" from a data plane
message EncryptionDescriptor {
  string profile = 1;      // "none" for a plain volume, then nothing else is set
  uint64 first_epoch = 2;
  KeyEnvelope wrapped = 3;
}
message KeyEnvelope {
  uint32 version = 1; string kind = 2; string id = 3; string kek_version = 4;
  string profile = 5; bytes sealed = 6;  // the 92 bytes above
}
```

- **Field 20 is the version marker.** An instance with this change sets `Volume.encryption` in
  every `Volume` it returns. A client with this change takes an answer without it for an older
  instance's, which answers only for a plain volume (the mark): the data plane and the CSI node
  plugin fail the attach; the CSI controller and `flaminio-meta create` refuse it for an
  encrypted request and take it for a plain one (design).
- **The gate.** `GetVolume` answers `FailedPrecondition` `volume <id> is encrypted and this data
  plane cannot decrypt it` to an `io` caller whose node is the record's target, for an
  encrypted volume, when `accepted_profile` is not `"xts-aes256-v1"`. Every other caller, a
  replica-side read among them, gets the record.
- **Size.** For #80's bound: a maximal `Volume.encryption` (64-byte identity strings, 32-byte
  profiles) encodes in 380 bytes (`KeyEnvelope` ≤ 328, the descriptor ≤ 376, plus 2 tag and
  2 length bytes), `key_source_storage_mode` ≤ 35; at most 415 bytes per `Volume`; a plain
  volume's field 20 is 9 bytes. #80's worst-response fixtures (Create, Get, Attach) gain a
  maximal descriptor (design, Creation).
- No raw key, HPKE private material or plaintext appears in GetVolume, ListVolumes or Attach,
  or in the capability token. A descriptor in a request is never read.

New RPC `ReleaseVolumeKey`. `auth.go`: `allowed = id.Node != "" && id.Component == "io"`; the
handler refuses a call with no authenticated caller, then requires the caller's node to be the
record's target and `epoch` its epoch, before any key call, and again after the unwrap (target,
epoch and first epoch as checked, else `DENIED`; the header absent or `Deleting`, `NotFound`). #73's
method table gains the row `ReleaseVolumeKey` | the target's data plane | the target's data plane |
– (an insecure instance refuses it in both columns, #73's rule). Request `{string volume = 1; uint64
epoch = 2; bytes recipient_public = 3; string profile = 4;}`. Response `{bytes encapsulation = 1;
bytes sealed_key = 2; string profile = 3; uint64 first_epoch = 4;}`. `recipient_public` 32 bytes,
`encapsulation` 32, `sealed_key` 80 (the DEK and a 16-byte tag); other sizes or an unknown profile
are refused.

HPKE Base, [RFC 9180](https://www.rfc-editor.org/rfc/rfc9180): DHKEM(X25519, HKDF-SHA256)
`0x0020`, HKDF-SHA256 `0x0001`, AES-256-GCM `0x0002`, sequence 0 only. `info`, exactly:

```text
"FLRELEASE001" ‖ u16be len(volume) ‖ volume ‖ u64be first_epoch ‖ u16be len(profile) ‖ profile
‖ u64be epoch ‖ u16be len(node) ‖ node ‖ u16be 32 ‖ recipient_public
```

`aad = info`. The service takes volume and epoch from the request, node from the caller's
certificate, first epoch and profile from the key record. The data plane builds `info` from
its own values: volume, first epoch, epoch and node from its capability token, verified by
#15's `Input::Attach` (a prerequisite, as #16's S15 is), profile
and `recipient_public` from its request; the response's `profile` and `first_epoch` must equal
them, or the attach fails and nothing is opened. A fresh recipient per attach. mTLS
authenticates the server only to a client that checks the `meta` URI SAN (#16's S15, a
prerequisite); HPKE keeps the transport's buffers opaque. Use interoperable reviewed libraries,
not a hand-coded key agreement: Go's standard `crypto/hpke`; the Rust library is a candidate
until Q2's review.
Settled at approval, 2026-11-13: Q2: measured and reviewed before a library is pinned (see
decisions.md, Settled at approval).

CSI: StorageClass parameter `encryption`, exactly `"true"` or `"false"`, default false.
`flaminio-meta`: client flag `--encrypted`, read by `create` only (as `--trust-domain` is read by
`confirm` only). `flaminio-meta serve --kek-file PATH`: refused at start with `--insecure`.
Root socket: no new line or operand; `attach VOLUME TOKEN-BASE64 [MS]` (`control.rs:5`) is
unchanged, and the header's mark and the key record decide (design). No key enters the socket,
the environment, argv or the token. The Rust metaclient adds `Call::ReleaseVolumeKey` and its
answer, whose Debug prints lengths only. No release to an administrator or a node agent.

## Data and retained references

AES-256-XTS: data key `DEK[0..32]`, tweak key `DEK[32..64]`. Data unit 4096 bytes; tweak
`LE128(volume_offset / 4096)`, no epoch and no chunk-relative tweak. Decryption is skipped only
for a complete stored zero block; checksums cover stored ciphertext or zero bytes at replicas as
today. A new authenticated profile cannot reuse this name or layout silently. No encrypted
snapshots, clones or backups are admitted until their catalog pins this descriptor, its source
version and any required keys; ordinary live deletion then removes only its reference, never
retained keys. No rewrap, DEK rewrite or erase API is introduced by this slice.
