# Issue #50: Encryption at rest with a KMS (Vault, KMIP) — specification

Step 1 of the issue ([intent](intent.md)). The choices are under
[Open questions](#open-questions), and a rule that follows a recommendation names its question
(Q1–Q12). Settled at approval, 2026-11-13: each question as recommended (see decisions.md,
Settled at approval). Revised after approval in plan review rounds 1 to 3 (decisions.md).

## Terms

- **Encrypted volume:** a volume whose record says so, from its creation.
- **Volume key:** the AES-256-XTS key of one encrypted volume. **Wrapped key:** its form in the
  volume's record, which only a key source opens.
- **Key source:** what wraps and unwraps volume keys under a key of its own (the KEK). **Static
  source:** the one this step builds, whose KEK is a file.
- **Release:** giving a volume key, unwrapped, to a target's data plane.
- **Block:** 4 KiB, the device's logical block and the cipher's data unit. **Block number:** a
  block's offset in the volume, in blocks.
- **Stored bytes:** what replicas receive, store, checksum and digest. **Zero block:** a stored
  block of zeros.

## The invariant

Below the target of an encrypted volume, every stored byte is ciphertext or part of a zero
block: on the wire, on every replica disk, in checksums, digests, repairs and resync. Plaintext
exists only in the application and in the target's process. The volume key exists unwrapped only
in that process and, for the length of a release, in the metadata service (Q1). Without the key
nothing is served: no plaintext is stored and no ciphertext is returned.

Two limits stay. A node that was once the target has held the key (rotation is step 4). A
replica node can alter or replay ciphertext unseen (step 7).

## Behaviour

### The volume and its key

**S1. Encrypted at creation.** `CreateVolume` takes it as a field, the CSI controller as the
StorageClass parameter `encryption` (`"true"` or `"false"`), `flaminio-meta create` as a flag.
The default is unencrypted (Q9). The record says which, every reading of it shows it, and
nothing changes it. A retry of a creation matches only a volume of the same choice, and an
answer that does not say which is refused for an encrypted request (it is a plain volume's).
#73's namespace policy on unencrypted volumes reads this choice, before any key call.

**S2. The key is made at creation** (Q1). The metadata service makes a random volume key, wraps
it with its key source and keeps the wrapped key in the volume's record. Without a usable key
source the creation is refused: an encrypted volume never exists without its wrapped key.

**S3. The wrapped key binds and describes itself** (Q7). It names its key source and the
version of the KEK that wrapped it. It authenticates the volume's id and first epoch by itself,
whatever the source offers. Unwrapped for another volume, for a later volume of the same name,
under another KEK or after any change to it, it is refused: it never yields another key. It is
no secret: whoever reads the record reads it, and a later backup can copy it.

**S4. Unencrypted volumes are unchanged:** their records, tokens, stored bytes and I/O path.
They never cause a key call.

### The key source

**S5. One source, the static one** (Q3). A metadata instance has at most one key source, of a
kind it is built with; any other kind is refused at start, and so is a source on an instance
without authentication. The static source's KEK is 256 random bits in a root-only file, the
same on every instance, given to the metadata instances only, on no command line and in no
environment. Its version is derived from its bytes, and the cluster records the first one used:
an instance whose KEK is not that one has no usable source. An instance that cannot read it
still serves everything else. The service says at start, and with every encrypted volume it
shows, that the source is the weaker kind: its KEK rests on the instances' disks, a node that
runs a metadata instance holds the KEK and every wrapped key, and whoever loses every copy loses
every encrypted volume.

**S6. Key calls and their failures.** The key source is called to wrap at creation and to
unwrap at a release, never else. Every call ends within a bound shorter than the time a caller
gives one instance. A failure is in one class, and the answer names it, the source and the
volume:
- **this instance has no usable source:** the caller asks another instance;
- **the source is away or slow:** try later; the other instances are not asked in turn;
- **the source refuses;**
- **the wrapped key is damaged, or not this volume's** (S3).

No failure changes a record. No error or log line holds a key.

### Release and attach

**S7. Release only to the target** (Q1). The metadata service releases a volume key only to the
data plane (`io`) of the node the record names target, for the record's current epoch. Every
other caller is refused before any key call: an administrator, an agent, another node, a
deposed epoch, a caller with no authenticated identity. A release changes no record. Each one
is logged with the caller, the volume, the epoch and the outcome; refusals at a bounded rate per
caller, the lines past it counted.

**S8. The key's path.** The key travels only in that call, over mutual TLS, to a client that
accepts only a metadata instance's identity (#16's S15). It never passes through the CSI
plugin, the control socket, a command line, the environment or the capability token, which is
unchanged.

**S9. Fail closed** (Q6). An attach of an encrypted volume completes only with its key held. If
the release fails, the attach fails within its limit, no device appears, and the error names
the class (S6). The same at a first attach, at a failover and after a data-plane restart. A
record that says encrypted and has no wrapped key fails the same way.

**S10. No key call on the I/O path.** The target holds the key while attached. Reads, writes,
ISR changes, reconciliation and resync make no key call: an attached volume keeps serving while
the key source or the metadata service is away.

**S11. The key in memory** (Q8). In the data plane it is locked, out of core dumps, never
printed, and zeroized when the volume detaches, when the target is deposed and when an attach
fails. If it cannot be locked the attach fails. In the metadata service a volume key lives for
one call, and an instance with a key source is not dumpable, from before it reads its KEK.

**S12. Mixed versions.** A data plane that cannot encrypt is never handed an encrypted volume
as a plain one: its attach is refused; a replica on such a node needs nothing. A metadata
instance that predates encryption serves no call on an encrypted volume but its deletion, and
cannot erase its wrapped key; a data plane or node plugin of this version does not take its
answer for a plain volume: the attach fails. Every metadata instance runs this version before
any data plane or client does. Encrypted creation needs a key source, given to the instances
once every instance and data plane runs this version; rolling back below it is unsupported
while an encrypted volume exists, and leaves that volume unserved, never plain, but also
unlisted, so no drain or node removal is safe while an older instance serves: a rollback deletes
the encrypted volumes first.

### The data path

**S13. Writes.** Every application write is encrypted before it leaves the target's frontend:
AES-256-XTS under the volume key, per block, the block number as tweak. Zeros written by the
application are encrypted like any data. Every replica receives the same stored bytes.

**S14. Reads.** Stored bytes are verified as today, then decrypted, whichever replica served
them. A checksum mismatch is handled as today.

**S15. Zero blocks read as zeros** (Q4). A zero block is not decrypted. So a block never
written reads as zeros, and its digest stays the zero block's CRC on every replica. A written
block whose ciphertext is all zeros would read as zeros: one chance in 2^32768, accepted.

**S16. Zero copy** (Q5). On a node run with `--zero-copy`, an encrypted volume is served as
without the flag: its local reads are copied, verified (whatever `--no-verify` says) and
decrypted. The node logs it once per attach. Unencrypted volumes keep zero copy.

**S17. Below the target, unchanged.** The protocol, the frames, the store, the on-disk format,
`flaminio-repl` and its simulator. Checksums, digests, `Mapped`, `Discard`, repairs, resync and
moves work on stored bytes. A replica never holds or asks for a key, and whatever reads
replicas reads stored bytes. A target on another node reads the same data. Deletion is as
today; the wrapped key goes with the record, which is not crypto-erase.

### The lab

**S18. The lab's key source and volumes.** `lab.sh` makes the static KEK once and gives it to
the metadata instances only. `vol create` can create an encrypted volume, and a StorageClass
with `encryption: "true"` is deployed beside the plain one.

**S19. The lab check and the cost.** One check, on an encrypted RF 2 volume:
- a known pattern written through the device is on no replica disk, and both replicas' digests
  agree;
- the reconcile and resync checks pass on it, and unwritten ranges read zeros;
- after a failover the other node reads the pattern back;
- with the KEK taken from every instance, a restarted data plane's attach fails with S6's
  class; with it back, the attach works and the pattern reads back;
- on a `--zero-copy` node (direct mode, #74's S7) the pattern reads back, and S16's line is
  logged.

Reads (verify mode) and writes are measured on an encrypted and an unencrypted volume by the
performance plan's rules, and recorded under `docs/benchmarks/`, with whether the guests have
the hosts' AES instructions. No threshold: the issue asks for the number.

**S20. The ADR and the docs.** An ADR, accepted before the design, records Q1, Q2, Q3, Q7 and
Q8, and what S3 and S6 promise later sources. ARCHITECTURE §12 (who computes checksums) and
§15.4 (where the key lives) are aligned with it; the README's StorageClass example and
`docs/lab.md` with S1 and S18.

## Tests

- **S1–S4, S7, S12 (metadata service, in memory):** creation with and without a source;
  nothing changes the flag; the record holds the wrapped key and never the key; each refused
  caller, before any key call, a caller with no identity among them; a release leaves every
  revision as it was; an unencrypted volume's record is byte for byte today's; an older
  instance cannot decode an encrypted volume's record and can still delete it; a wrap that fails
  leaves the id free (with #76, its operation to a retry, a helper or a deletion); the target's
  older data plane is refused, a replica-side read answered; #73's policy sees the choice.
- **S1, S12 (CSI and admin client):** an answer that drops the choice of an encrypted request,
  and a plain volume of the same geometry under an encrypted request, are refused; an older
  instance's answer to a plain request is taken; the node plugin refuses an older instance's
  attach answer.
- **S3, S5, S6 (unit, with a source that fails on command):** a wrapped key unwraps to its key;
  another id, another first epoch, another KEK and a flipped bit are refused; each class gives
  its answer; with two instances, a missing or different KEK moves the caller on and a slow
  source is called once; a hanging source ends at the bound; a source with authentication off is
  refused at start; an instance with a source is not dumpable; no output holds key bytes.
- **S13–S15 (unit, data plane):** published XTS-AES-256 vectors; tweaks across a chunk
  boundary; a zero block in, zeros out.
- **S9–S11, S13–S17 (engine, in-memory devices, RF 2):** a pattern is in no device; replicas
  hold equal bytes; unwritten blocks digest as zero blocks; a restart's reconciliation and a
  resync pass; a second engine with the key reads the same; without the key the attach fails
  and no I/O runs; an older instance's answer, and a release for another first epoch, fail the
  attach; the key prints as nothing. Injected bugs, each caught: the tweak taken from the
  chunk's block number, a zero block decrypted, a write sent unencrypted. The threats step 7
  covers are shown unprotected: a replaced or replayed block reads without an error.
- **S17 (simulator):** `make sim` passes unchanged; no replication rule is added.
- **S18, S19 (script tests, `make lint`):** the lab commands and the check with the VMs faked.
- **S16, S19 (lab):** the check and the measurement. The ublk frontend runs nowhere else.

## Open questions

Each was settled at approval as recommended (decisions.md); the rules follow it. Q1–Q3 are
recorded in the ADR proposal. None contradicts an accepted ADR.

- **Q1. Who releases the key.** (a) The metadata service keeps the wrapped key and releases to
  the target alone, straight to its data plane. (b) Each data plane calls the key source.
  *Recommend (a):* only the service knows the target; under (b) any node may unwrap any key.
  Cost: the service sees the key, so §15.4's "only in target memory" is reworded. #57 and #60
  follow (a) and are redrawn under (b).
  Settled at approval, 2026-11-13: (a) (see decisions.md, Settled at approval).
- **Q2. The cipher's library.** Pure Rust (`aes`, `xts-mode`) or FFI (aws-lc, OpenSSL).
  *Recommend:* measure both on the lab's CPUs first; S13 fixes only the cipher.
  Settled at approval, 2026-11-13: measure both first; S13 fixes only the cipher (see decisions.md, Settled at approval).
- **Q3. The first key source.** (a) Static KEK file. (b) Vault Transit now, with a
  Vault-compatible server in the lab. *Recommend (a).*
  Settled at approval, 2026-11-13: (a), a static KEK file (see decisions.md, Settled at approval).
- **Q4. An unwritten block.** Zeros (S15), or noise as dm-crypt gives. *Recommend zeros:*
  today's checked behaviour; digests and repairs stay as they are.
  Settled at approval, 2026-11-13: zeros (see decisions.md, Settled at approval).
- **Q5. `--zero-copy` and an encrypted volume.** Serve it copied (S16), or refuse the attach.
  *Recommend copied.*
  Settled at approval, 2026-11-13: served copied (see decisions.md, Settled at approval).
- **Q6. A data-plane restart while the key source is away** leaves the volume down until it is
  back. *Recommend accepting it;* a node's sealed cache is #57's later step.
  Settled at approval, 2026-11-13: accepted (see decisions.md, Settled at approval).
- **Q7. Refusing a wrong key.** The binding inside the wrapped key (S3), or also a check value
  the data plane verifies. *Recommend S3 alone:* the service that unwraps is the one that
  releases.
  Settled at approval, 2026-11-13: S3 alone (see decisions.md, Settled at approval).
- **Q8. `unsafe` for locking memory.** One small reviewed module, or a crate. *Recommend the
  module.*
  Settled at approval, 2026-11-13: the module (see decisions.md, Settled at approval).
- **Q9. Encryption by default** (§2, principle 5). *Recommend off* until a KMS source exists.
  Settled at approval, 2026-11-13: off (see decisions.md, Settled at approval).
- **Q10. #60's finer failures:** rate-limited, KEK disabled, pending deletion. *Recommend* they
  come with #60 as kinds of S6's "try later" and "refuses"; S6's four are the minimum.
  Settled at approval, 2026-11-13: with #60, as kinds of S6's answers (see decisions.md, Settled at approval).
- **Q11. Configuring only tested sources** (#60). S5 closes the set of kinds. *Recommend* #60's
  conformance test be the one place a kind is added.
  Settled at approval, 2026-11-13: #60's conformance test (see decisions.md, Settled at approval).
- **Q12. Many attaches at once** against a slow or rate-limited source (#60). S6 bounds one
  call. *Recommend* a bound on calls in flight with step 2: the static source is local.
  Settled at approval, 2026-11-13: a bound on calls in flight, with step 2 (see decisions.md, Settled at approval).
