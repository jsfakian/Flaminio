# Issue #50 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`323977909e4fce1dcf3c22b97858dc0ac6658b304b780d90f30474654a18be1d`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

- Plan review round 1 (2026-11-18) fixes, made after approval: see below.
- Plan review round 2 (2026-11-18) fixes, made after approval: see below.
- Plan review round 3 (2026-11-18) fixes, made after approval: see below.
- Plan review round 4 (2026-11-18) fixes, made after approval: see below.

## Settled at approval

- **Q1. Who releases the key:** (a), the metadata service keeps the wrapped key and releases it
  to the target alone, straight to its data plane; §15.4's "only in target memory" is reworded.
  [spec.md](spec.md), Open questions (S2, S7).
- **Q2. The cipher's library:** measure the pure-Rust and FFI libraries on the lab's CPUs first;
  S13 fixes only the cipher. The design's candidates (RustCrypto `aes` 0.9.4 + `xts-mode` 0.6.0,
  `hpke` 0.15.1, Go CIRCL HPKE) are not pinned: that comparison, conformance and the
  secret-memory review pick the library before implementation. [spec.md](spec.md), Open
  questions; [design.md](design.md), Protected delivery and memory.
- **Q3. The first key source:** (a), a static KEK file. [spec.md](spec.md), Open questions (S5).
- **Q4. An unwritten block:** zeros (S15). [spec.md](spec.md), Open questions.
- **Q5. `--zero-copy` and an encrypted volume:** served copied (S16). [spec.md](spec.md), Open
  questions.
- **Q6. A data-plane restart while the key source is away:** accepted: the volume stays down
  until the source is back; a node's sealed cache is #57's later step. [spec.md](spec.md), Open
  questions (S9).
- **Q7. Refusing a wrong key:** the binding inside the wrapped key (S3) alone.
  [spec.md](spec.md), Open questions.
- **Q8. `unsafe` for locking memory:** one small reviewed module. [spec.md](spec.md), Open
  questions (S11).
- **Q9. Encryption by default:** off until a KMS source exists. [spec.md](spec.md), Open
  questions (S1).
- **Q10. #60's finer failures:** they come with #60 as kinds of S6's "try later" and "refuses";
  S6's four are the minimum. [spec.md](spec.md), Open questions.
- **Q11. Configuring only tested sources:** #60's conformance test is the one place a kind is
  added. [spec.md](spec.md), Open questions.
- **Q12. Many attaches at once:** a bound on calls in flight, with step 2. [spec.md](spec.md),
  Open questions.
- **The profile's guarantee** (restricted profile or a superseding integrity-bearing format):
  as the design proposes, `xts-aes256-v1` gives confidentiality only, with no claim of
  protection against a malicious replica, tampering or replay. [design.md](design.md), Policy
  boundary before implementation; [adr-proposal.md](adr-proposal.md), Decision proposed for
  review.
- **Protected key delivery:** as the design proposes, the release carries an RFC 9180 HPKE
  envelope inside mutual TLS, to the target's fresh per-attach key, as
  [contracts.md](contracts.md) fixes; the secret-lifetime audit stays a merge blocker.
  [design.md](design.md), Protected delivery and memory; [adr-proposal.md](adr-proposal.md),
  Decision proposed for review.

## Changes after approval — plan review round 1 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `9edd590`; two plan
reviewers, design-code focus R1-C1…C19 and design-security focus R1-S1…S13). Nothing here is
approved anew, no risk is accepted, and no test or lab result exists. Q1–Q12 and the two
settled design points are unchanged.

**Choices, per finding**

- **R1-C1 (High), R1-S1 (Critical): the wrapped key in a record of its own.** The envelope moves
  out of the header into `/flaminio/v1/volkeys/<id>/<first epoch>`, written by the publication
  transaction and deleted by the tombstone transaction; its presence is what makes a volume
  encrypted, and the header keeps today's JSON. An older instance, which decodes and rewrites
  headers, can no longer erase the key or turn the record plain. Since the header carries no
  descriptor, C1's "a header without the descriptor whose key record exists is refused" becomes
  "the key record decides", the same guarantee with one source. Readers read the header, the
  record and the header's revision again (or #77's coherent read). Keying by first epoch keeps
  an older instance's leftover record from matching a later incarnation. S1's part (2), an
  answer the new clients can recognize: `Volume.encryption` (20) is always set by this version,
  `{profile: "none"}` for a plain volume, so no extra tag is needed; an answer without it fails
  the attach and is refused by the CSI controller and `flaminio-meta`. S1's part (3) and C1's
  switch: the key source flag `serve --kek-file` is the switch, given in a second rollout, as
  #75's `--qos` (the reviewer's reference): a store record written by an operator asserts no more
  than the flag does, while the flag also makes a binary-only rollback fail closed. Rollback is
  unsupported while an encrypted volume exists. Tests: `TestOldHeaderRewriteKeepsKey`,
  `old_instance_answer_is_refused`, `TestKeyRecordDamaged`.
- **Risk named, not accepted (R1-C1, R1-S1):** an older metadata instance answering an older
  data plane, both against the rollout rule, serves an encrypted volume as plain. **Withdrawn in
  round 2 (R2-S1):** closed by the header's mark, which no older instance decodes (below).
- **R1-C2 (High): the answer is checked.** `encrypted` joins #76's canonical create identity and
  #80's comparison (taken from the key record once published), whichever lands second. The CSI
  controller and `flaminio-meta create` compare the answer's profile on `OK` and on
  `AlreadyExists`, as #75 does for its policy: on `OK` the CSI controller deletes the volume it
  just created; nothing is deleted on `AlreadyExists`; `flaminio-meta` never deletes. The wrong
  citation of "#76's creator ownership" is gone (#76's S3 has none). Spec S1 gains the sentence;
  a widening, not a narrowing.
- **R1-C3 (Medium), R1-S8 (Medium), R1-S9 (Medium): #73's two hand-offs.** The policy check
  reads `encrypted`, before any key call (S1, design; whichever of #50 and #73 lands second).
  Audit events: the simpler option, a follow-up recorded in `docs/followups.md` with the change
  (#73's decisions ask for exactly that), issue to be filed; the intent's out-of-scope line is
  corrected. Refusals are logged at most 10 lines a minute per caller, the next line counting
  those dropped (#73's limiter once it lands). S7's "each one is logged" holds: every refusal is
  logged or counted. `ReleaseVolumeKey` gets its row in #73's method table (contracts).
- **R1-C4 (Medium), R1-S2 (High): the record decides; no new verb.** `attach-encrypted` is
  dropped (the reviewers' first option, cut before adding). The decision is in
  `Engine::start_target`, which every path reaches: the socket's `attach … [MS]` (so #6's
  deadline stays), the startup flags, #79's `attach-init`, #83's handoff lines. No operand is
  added to #79's or #83's lines, so neither clear plan changes. An older plugin's line works on
  a new daemon.
- **R1-C5 (Medium), R1-S13 (Low): the gate refuses only the target.** `GetVolume` refuses an
  `io` caller that is the record's target and sends no profile; replica-side reads (#13's sweep
  and lookups) are answered, so an older replica-only node loses nothing (S17). The data plane
  sets `accepted_profile` in #13's `get_volume_call`, as #75 does its flag. Test:
  `TestGetVolumeGate`.
- **R1-C6 (Medium), R1-S6 (Medium), R1-S7 (Medium): the KEK's delivery.** S5's "given as the
  token key is" cannot hold beside S5's own "an instance that cannot read it still serves
  everything else" (a `LoadCredential=` of a missing path stops the unit) and intent 6 (the token
  key travels base64 in `ENV_VARS` on the ssh command line). It is replaced by "given to the
  metadata instances only, on no command line and in no environment": the intent is unchanged.
  Named: flag `--kek-file`, file `/etc/flaminio/pki/kek` (`pki` only, never `io-pki`),
  generator `/dev/urandom` with `umask 077` once, transfer over ssh's stdin. The service checks
  only type and length (`O_NOFOLLOW`, 32 bytes); `lab.sh` checks root 0600 at install. Of the
  reviewer's two unit options the simpler: `meta.sh up` writes the credential and the flag only
  when the file exists, and the lab step restarts with `meta.sh up`. `encryption-test.sh`
  covers the unit text and that no argv or `ENV_VARS` holds the KEK.
- **R1-C7 (Medium): revocation without new machinery.** The cipher is a field of the device's
  `Shared` and is zeroized when it drops, on every exit of `run_device`; the admission flag,
  borrow count and wait are gone. With #15, its per-device `Shared`.
- **R1-C8 (Medium): concrete tests.** The test table names file and case per rule; the cipher
  is `flaminio-node/src/crypt.rs`, so the engine tests (`flaminio-node/tests/engine.rs`) use it
  as the frontend does; `flaminio-node` gains `flaminio-uring` for the locked memory. S16 has
  the pure `device_mode` with its table test; the rest of S16 stays in the lab. The no-cycle
  rule needs no new Go code: `SOURCE_UNAVAILABLE` is `ResourceExhausted`, on which neither
  client moves on (`TestSourceUnavailableIsNotCycled`).
- **R1-C9 (Low):** an encrypted device is added without the zero-copy flags and served by
  `engine_io_task`; no staged decrypting variant.
- **R1-C10 (Low):** the lab plan names `make encryption-check` (`scripts/checks/encryption.sh`),
  `make vol-create ENCRYPT=1`, `make bench-reads`/`bench-writes`, and adds S19's `--zero-copy`
  step in direct mode (#74's S7); S19's bullet says so too.
- **R1-C11 (Low):** wrap right before publication and put the record in that transaction; the
  separate envelope write and its extra revision are gone.
- **R1-C12 (Low):** the cap of 8 calls in flight is dropped, as Q12 settled.
- **R1-C13 (Low):** forced verification is kept and written into S16 ("whatever `--no-verify`
  says"), the reviewer's first option: removing it would narrow S16's "verified".
- **R1-C14 (Low):** the size arithmetic is in contracts (at most 415 bytes per `Volume`), and
  #80's worst-response fixtures gain a maximal descriptor. Whether #80's 2048-byte header
  allowance still covers it is for those fixtures to show; if not, #80's allowance must grow
  before encrypted creation is admitted (reported for #80, a clear plan, not edited here).
- **R1-C15 (Low), R1-S5 (Medium): the KEK's identity is its bytes.** `ID = "static"`;
  `KEKVersion` is derived from the KEK (an HMAC check value); the cluster pins the first identity
  with a conditional create of `/flaminio/v1/keysource`. A different KEK is no usable source
  (`INSTANCE_UNAVAILABLE`, so the caller tries another instance), and an envelope naming another
  identity is `INSTANCE_UNAVAILABLE`, not `INVALID_ENVELOPE`. Test: `TestKeySourcePin`.
- **R1-C16 (Low), R1-S4 (Medium):** a key source is refused at start with `--insecure`, and
  `ReleaseVolumeKey` and an encrypted `CreateVolume` refuse a call with no authenticated
  caller. Tests: `TestServeRefusesInsecureWithKEK`, `TestReleaseUnauthenticated`.
- **R1-C17 (Low):** every encoding is byte for byte in contracts: AAD, `Sealed`, HPKE `info`;
  `Version` is in the AAD (also the security Nit).
- **R1-C18 (Low):** Go's standard `crypto/hpke` replaces CIRCL as the Go side; the Rust library
  stays a candidate under Q2.
- **R1-C19 (Low):** the "Excluded threats" row names the tamper and replay tests.
- **R1-S3 (Medium):** #16's S15 (clients accept only a `meta` URI SAN) is a prerequisite of
  this change, the reviewer's first option, rather than a second copy of #16's check.
- **R1-S10 (Medium):** the ADR's Consequences state the XTS version leak, the metadata in clear
  and the nodes holding the KEK; its claim is reworded to "a data disk alone, or a node that
  holds no KEK"; S5's notice adds the nodes that run a metadata instance.
- **R1-S11 (Low):** the data plane builds the HPKE `info` from its verified token and its own
  request; the response's profile and first epoch must equal them. Test:
  `release_for_another_first_epoch_is_refused`.
- **R1-S12 (Low):** the rule, not the record: S11 gains "an instance with a key source is not
  dumpable" (`PR_SET_DUMPABLE` 0, the lab unit's `LimitCORE=0`); the GC-copy exposure stays in
  the ADR's Consequences, under review.
- **Nits (both reports):** all easy, all fixed: the stale "nothing is settled" text (spec,
  design, ADR), "Q1–Q9" to "Q1–Q12", the numbers run into words, the CSPRNG named and a DEK
  with equal halves redrawn, `Version` in the AAD.

## Changes after approval — plan review round 2 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `f2857b5`; two plan
reviewers, design-code focus R2-C1…C6 and design-security focus R2-S1…S4). Nothing here is
approved anew, no risk is accepted, and no test or lab result exists. Q1–Q12 and the two
settled design points are unchanged.

**Choices, per finding**

- **R2-S1 (High), R2-C3 (Medium): the header's mark closes round 1's risk; two rollouts in
  order.** The reviewer's fix: an encrypted header writes `id` as an object
  (`{"name", "profile"}`), which every older version, typing `id` as a string, cannot decode.
  Today's code fails closed on such a header: every call but DeleteVolume fails, ListVolumes
  skips it, DeleteVolume still removes it. `id` was chosen as the field because every version
  since the header existed has it as a string; `first_epoch` would be ignored by versions before
  #30. The key record still holds the wrapped key. The round-1 statement that the header keeps
  today's JSON changes for encrypted headers only; plain headers stay byte for byte today's
  (S4). The decode error answers `Unavailable` (`errStatus`), so a client does try the next
  instance, unlike the reviewer's note; an older one fails the same way, a newer one serves the
  volume, so the conclusion holds. Round 1's named risk is withdrawn: it would have been data
  loss, as the reviewer says (an older node plugin formats a device with no signature,
  `control/csi/node.go:107–116`), and no older instance now serves an encrypted volume at all,
  whatever it is started with (the lab's rollback included). The new CSI node plugin refuses an
  `Attach` answer without field 20 before calling the data plane, as recommended. R2-C3's first
  rollout is stated in the design, S12 and the docs: every metadata instance before any data
  plane, node plugin, CSI controller or `flaminio-meta`; the key source in a second rollout;
  rollback in reverse. Under that order every answer a new client sees has field 20, so plain
  volumes keep working; and the CSI controller and `flaminio-meta` take an answer without field
  20 to a plain request (an older instance made what was asked). Tests:
  `TestOldDecoderRefusesEncryptedHeader` (replaces `TestOldHeaderRewriteKeepsKey`, whose rewrite
  can no longer happen), `TestNodeRefusesOldInstanceAttach`, `TestControllerEncryption` and
  `TestCreateEncryptedFlag` gain the plain case.
- **Rule narrowed (R2-C3): S1's "an answer that does not say which is refused"** now applies to
  an encrypted request only. As stated it refused a plain volume made exactly as asked whenever
  an older instance answered, against S4 and intent 2 (unencrypted volumes behave as today), and
  the first rollout's order is not enforced. Its purpose, an encrypted request never yielding a
  plain volume, is kept; an answer without field 20 is a plain volume's (the mark).
- **R2-C1 (Medium): a failed wrap undoes its claim.** The reviewer's first option, rather than
  making #76 a prerequisite: on a wrap failure the handler deletes the chunk records and the
  `Creating` header under the claim's compare. Every source check, the pin included, runs before
  the claim. A crash or etcd error mid-creation still leaves today's hidden header, as for any
  creation until #76. `TestCreateEncryptedCrashPoints` asserts the id is free after a failed
  wrap.
- **R2-C2 (Medium): the engine's attach, specified.** `start_target` is split: checks, field
  20's decision, then (encrypted) the release under `MetaWait::Attach { part: 2, attempt }`,
  then the start and `Output::Attached`. An answer for a dropped attach is never opened: its
  recipient key went with the attach. With #15 the release goes after the checks and before
  #15's run mark, fenced by #15's generation; #15's plan is unchanged. Tests:
  `release_answer_after_detach_starts_nothing`, `no_target_before_release_answer`.
- **R2-C4 (Low): no revision compares.** Round 1's pair read (header, record, the header's
  revision again) is gone: `readVolume` reads the key record only for a marked header, and on
  its absence re-reads the header for presence and first epoch (`NotFound` or `Internal`). The
  release's re-read compares target, epoch and first epoch; a change is `DENIED`, not the
  reviewer's `Aborted`: only a deposition changes them, and a retry would be refused `DENIED`
  anyway, so the engine needs no retry. Tests: `TestKeyRecordDamaged`,
  `TestReleaseTargetChangesDuringUnwrap`, each with an unrelated header write.
- **R2-C5 (Low): the pin before the claim.** The reviewer's first option: a create that fails is
  read again and compared; checked before the claim; a match is cached, as the pin never
  changes in this step. `TestKeySourcePin` gains the race.
- **R2-C6 (Low), R2-S3 (Low): leftover key records.** Both fixes, as one rule beside the chunk
  records': DeleteVolume lists and deletes `volkeys/<id>/` after its commit and in its
  header-absent branch, and a creation does under its claim. Contracts say what remains: an
  older instance's deletion (an unsupported rollback) leaves the record until the id is deleted
  or created again. Test: `TestDeleteRemovesKeyRecord` with an undecodable header, an older
  deletion, and both follow-ups.
- **R2-S2 (Medium): the lab never replaces a VM's KEK.** The install compares the VM's `kek`
  with the workstation's under `sudo` (`cmp -s`, through stdin) and stops on a difference,
  leaving it unchanged. `encryption-test.sh` gains the case.
- **R2-S4 (Low), code Nit 3: #15 a prerequisite.** #15's token verification is named beside
  #16's S15, the first option of both reports; the design adds what holds even so (a wrong token
  value only fails the attach).
- **Nits:** code Nit 1 (the one test that guards today's behaviour is named as such), code
  Nit 2 (the seal goes between `copy_of` and `Bytes::from_owner`), security Nit 1 (`sudo test
  -f`) and security Nit 2 (`PR_SET_DUMPABLE` 0 before the KEK is read, in the design and S11):
  all easy, all fixed. None left.

## Changes after approval — plan review round 3 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `b87df55`; two plan
reviewers, design-code focus R3-C1…C3 and design-security focus R3-S1…S3). Nothing here is
approved anew, no risk is accepted, and no test or lab result exists. Q1–Q12 and the two
settled design points are unchanged.

**Choices, per finding**

- **R3-C1 (Medium), R3-S2 (Medium): an older instance's listing, one fix.** Both reports are
  right: an older ListVolumes skips a header it cannot decode and answers `OK`
  (`service.go:851–854`), and `drain.Step` visits only listed ids (`control/drain/drain.go:50`),
  so a drain through it reports a node empty while it holds encrypted replicas. The root cause is
  an older instance running while an encrypted volume exists; the fix restores round 1's
  instruction, dropped in round 2: the only safe rollback deletes the encrypted volumes first,
  with this version (which also deletes their key records, and with #72 releases their capacity,
  which an older deletion of a header that does not parse does not). The design, S12 and the docs
  name the exception: while an older instance serves and an encrypted volume exists, its
  listings omit the volume, so no drain or node removal is safe.
- **Claim narrowed (R3-C1, R3-S2):** the design's "fails closed whatever the older instance is
  started with" now covers per-volume calls only; listings do not fail closed. S12's "unserved,
  never plain" still holds and gains "but also unlisted". Round 2's withdrawal of round 1's risk
  stands, since it concerned serving as plain, which the mark still prevents.
- **Not taken (R3-S2): a version marker on `ListVolumesResponse`.** Cut before adding. Under the
  two orders no older instance runs beside an encrypted volume: the key source comes only after
  every binary runs this version, and the rollback deletes the encrypted volumes first. When
  one does (a rollback against the rule, in reverse order), the drain binary is already older,
  and no marker protects an older drain. The marker would guard only a new drain against an
  older instance, outside both orders, at the cost of a proto field, two client checks and a
  test. **Risk named, not accepted:** a rollback against this rule (an older
  instance serving while an encrypted volume exists) lets a drain of any version retire a node
  that holds that volume's replicas, all of them at RF 1.
- **R3-S1 (Medium), R3-C2 (Low), code Nit 1: the mark and the record must agree.** The
  security reviewer's fix: `readVolume` also reads the key record for a plain header (one Get,
  not on the I/O path), and a record under a plain header is `Internal`; so is a mark whose
  `name` is not the volume's id or whose `profile` is not `xts-aes256-v1` or not the record's
  (code Nit 1, the same rule). A new-version header write that lost the mark now fails closed,
  never serves ciphertext as plain. Round 2's "a plain header is plain, and no record is read"
  (R2-C4) is replaced; no revision is compared still. Records are keyed by first epoch, so a
  leftover of an earlier incarnation never matches a plain header. The design says every header
  write of this version keeps the mark (Attach and DeleteVolume write back the header they
  decoded; #13's `fenced` and #76's finalization too, whichever lands second); "header JSON as
  today" is gone and "the record decides" (design, contracts) is now "the header's mark and the
  key record decide (they must agree)". Tests: `TestCreateEncryptedVolume` asserts the claim
  header unmarked and the published header's `id` the object; `TestHeaderWritesKeepMark` (new:
  Attach, the deletion mark, and #13's and #76's writers once landed);
  `TestOldDecoderRefusesEncryptedHeader` decodes the header the service wrote;
  `TestKeyRecordDamaged` gains the plain header with a record and the three mismatched marks.
- **R3-C3 (Low): the failed-wrap undo with #73 and #76.** Both clear plans are followed and not
  edited; whichever lands second wires it. With #73: the undo's transaction also deletes
  `owners/<id>`, lowers the usage the claim charged and appends #73's `delete.committed`, as #73's
  release of a creation's own headerless owner record does. With #76: the reviewer's first
  option, the simpler (no undo): #76 alone removes a creating header (its rollback, with its
  root), the wrap is at its finalization, and a failed wrap leaves the operation to a retry, a
  helper or a DeleteVolume. `TestCreateEncryptedCrashPoints` covers both.
- **Test statement narrowed (R3-C3):** the spec's "a wrap that fails leaves the id free" (round
  2) cannot hold beside #76 without bypassing #76's protocol, which owns a creating header; it
  now reads "(with #76, its operation to a retry, a helper or a deletion)". Its purpose, no id
  left blocked by a failed wrap, is kept.
- **R3-S3 (Low): the KEK install.** The reviewer's fix: before any write to any VM, CA files
  included, `pki issue` checks the workstation's `kek` is exactly 32 bytes and compares it with
  every metadata VM's the run reaches, so a refusal changes no VM; remotely a `mktemp` file in
  `/etc/flaminio/pki`, `wc -c` 32, `sync`, compare again, `mv -f`. `encryption-test.sh` gains a
  31-byte workstation KEK and a second VM refusing.
- **Nits:** code Nit 1, closed with R3-S1 above; security Nit 1 (the release's re-read refuses
  a header absent or `Deleting`, `readHeader`'s `NotFound`, in the design and contracts): easy,
  fixed. None left.

## Changes after approval — plan review round 4 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `e067381`; one plan
reviewer, design-code and design-security focuses together, R4-C1, R4-C2, R4-B1 and one Nit).
Nothing here is approved anew, no risk is accepted, and no test or lab result exists. Q1–Q12,
the two settled design points and every spec rule are unchanged; nothing is narrowed.

**Choices, per finding**

- **R4-C1 (Low): the #73 undo is #73's own release.** The reviewer's first option, the simpler
  (no new transaction, no new outcome): with #73 the undo still deletes the chunks and the
  `Creating` header under the claim's compare, as without it; the creation's own owner record,
  now headerless, is then released by #73's existing release (#73 design, DeleteVolume's
  no-header branch), with that release's compares (header absent, owner record, chain tail,
  quota), its retry in the method's loop and its event, `delete.committed` `header-missing`,
  which is right because the header is gone when it runs. Round 3's single transaction under
  the claim's compare alone, a read-modify-write of the usage, is gone. A full chain or spent
  retries leave the headerless owner record and its charge to #73's next release (the id's next
  creation, which answers `Aborted` as #73 says, or a DeleteVolume); the call answers the wrap's
  error. #73 is followed, not edited. `TestCreateEncryptedCrashPoints` gains a failed wrap
  racing a second creation in the same namespace (usage exact) and the full chain.
- **R4-C2 (Low): with #76, the finalization marks the header.** #76 keeps the creating header
  unmarked until visibility, so its finalization is this plan's publication: it writes the
  header marked and the key record in one transaction, whether the creator or a helper runs it.
  It moves from the writers that keep the mark to the publication (design, contracts).
  `TestHeaderWritesKeepMark`: after a finalization by the creator and by a helper, the `id` is
  the object and the record written. #76 is followed, not edited.
- **R4-B1 (Low): `pki issue` on an existing lab.** The reviewer's fix: `pki issue` runs
  `pki init` when the CA or the `kek` is missing (`init` makes each piece only if missing), so
  a state dir with CAs and no `kek` gets one at its next `pki issue`, `issue w` and `issue c`
  included; the length check and the compare-only pass run only on runs that reach a metadata
  VM, so `issue c` does neither. The refusal to overwrite a different KEK is kept: a fresh
  `kek` against a VM that holds another is refused by the compare-only pass and by the remote
  step. `encryption-test.sh` gains the case (CAs, no `kek`, `pki issue c` then `pki issue`).
- **Nit 1 (easy, fixed): `ln`, then compare.** The remote step links its synced `mktemp` file
  as `kek` with `ln`, which fails if `kek` exists, and then compares; `mv -f` is gone, so two
  first installs of different KEKs cannot both succeed. The `mktemp` file is removed on every
  path, the failed length check included. `encryption-test.sh` covers both. None left.
