# Issue #50 — report

## Review rounds

### Plan review round 1 — 9edd590

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code focus (R1-C1…C19); one fresh plan-reviewer,
design-security focus (R1-S1…S13).

- **R1-S1 — Critical:** an older metadata binary destroys the wrapped key and answers an
  encrypted volume as plain. Evidence: the envelope lived only in the header JSON, decoded
  leniently and written back whole (`meta/service/service.go:275-291`, `:541`, `:723-725`);
  `volumePB` (`:337`); design.md:47 had one sentence, no mechanism, no test, no rollback rule.
- **R1-C1 — High:** the same hazard, from the design-code side. Evidence: as R1-S1; S5 lets an
  instance run without the KEK; #75 guards the same hazard (`docs/issues/75/design.md`, the
  switch).
- **R1-C2 — High:** a creation can answer success with a plain volume for an encrypted request.
  Evidence: `control/csi/controller.go:231` discards the answer, `:239-247` compares four
  properties; #76's identity and #80's comparison lacked `encrypted`; design cited a "#76
  creator ownership" #76 does not have.
- **R1-S2 — High:** whether a target encrypts followed the socket verb; the plain line, the
  startup flags, #79's `attach-init` and #83's handoff lines were not composed. Evidence:
  contracts.md:60-61; `control.rs:84`; `node.rs:42-43`; #13 design.md:312-315; #79 and #83
  contracts.
- **R1-C3 — Medium:** #73's two hand-offs (policy input, audit) not taken. Evidence: #73
  spec S2 and :133, decisions :110-115, batch ledger :222; intent.md:120-121.
- **R1-C4 — Medium:** attach paths unspecified and the new verb redundant; the verb lost `[MS]`.
  Evidence: `ublk.rs:731-770`, `scripts/guest/node.sh:60-63`, contracts.md:60, `control.rs:5`.
- **R1-C5 — Medium:** the `GetVolume` gate would refuse replica-side reads. Evidence:
  design.md:47-49, contracts.md:43; #13's `get_volume_call` (design.md:312-315).
- **R1-C6 — Medium:** the KEK's delivery contradicts the lab's `LoadCredential` unit; flag and
  path unnamed. Evidence: `scripts/guest/meta.sh:20-36`; #16 design.md:153, :212-214, :409.
- **R1-C7 — Medium:** revocation rested on a generation mechanism the code lacks. Evidence:
  design.md:97-100; `ublk.rs:619-629`, `:888-895`, `:1050-1057`.
- **R1-C8 — Medium:** test plan named areas, not tests; a fake ublk queue that does not exist.
  Evidence: design.md:136-147; `flaminio-node/Cargo.toml:13-17`; `ublk.rs:1207`, `:1360`;
  `control/csi/meta.go:63-86`.
- **R1-S3 — Medium:** the release relied on a server check the clients do not make. Evidence:
  `flaminio-metaclient/src/lib.rs:85-92`, `pkg/pki/pki.go:300-310`, `scripts/lab.sh:584`; #74
  design.md:333-338.
- **R1-S4 — Medium:** with authentication off, keys would be released to anyone. Evidence:
  `meta/service/auth.go:42-44`, `service.go:801-804`; #73 contracts.md:74-76.
- **R1-S5 — Medium:** the KEK's identity was not tied to its bytes. Evidence: contracts.md:5-6;
  design.md:69-70; `lib.rs:158-161`.
- **R1-S6 — Medium:** "given as the token key is" put the KEK on a command line. Evidence:
  `scripts/lab.sh:556-559`, `:579`; `scripts/lib.sh:157`; `scripts/guest/pki.sh:27`.
- **R1-S7 — Medium:** the file checks and the missing-KEK case did not fit a `DynamicUser`
  unit. Evidence: `scripts/guest/meta.sh:21-36`; design.md:57; S5; S19.
- **R1-S8 — Medium:** #73's audit hand-off not taken; refusal logs unbounded. Evidence: #73
  spec.md:132-133, S7; decisions.md:112-115; design.md:76; intent.md:120-121.
- **R1-S9 — Medium:** #73's unencrypted policy not fed. Evidence: #73 spec.md:72-74; batch
  ledger :222.
- **R1-S10 — Medium:** the ADR's statement of what the profile hides was incomplete. Evidence:
  adr-proposal.md:16, :40; `docs/format/on-disk-format-v1.md:363-364`.
- **R1-C9 — Low:** S16's mechanism not chosen. Evidence: design.md:126; `ublk.rs:906-910`,
  `:1098`.
- **R1-C10 — Low:** the lab plan not runnable; S19's zero-copy step missing. Evidence:
  design.md:158-168; #74 spec S7.
- **R1-C11 — Low:** the separate envelope write is not needed. Evidence: design.md:39-43;
  `service.go:415`, `:465-468`.
- **R1-C12 — Low:** the cap of 8 key calls contradicted Q12. Evidence: decisions.md:39-40;
  design.md:67; contracts.md:31.
- **R1-C13 — Low:** forced verification had no spec rule. Evidence: design.md:126-127; S14;
  `ublk.rs:996-999`, `:1678`.
- **R1-C14 — Low:** "within #80's admission budget" asserted, not shown. Evidence:
  contracts.md:45, :21; #80 design.md:30 and contracts (Layered schema profiles).
- **R1-C15 — Low:** the static source's `ID` and `KEKVersion` had no stated origin. Evidence:
  contracts.md:5-6; design.md:56-57.
- **R1-C16 — Low:** release with authentication off. Evidence: `auth.go:42-44`;
  `service.go:801-804`; design.md:82.
- **R1-C17 — Low:** contracts not byte-exact (`packed_nonce_ciphertext`, AAD prefixes, HPKE
  `info` widths, `Version` outside the AAD). Evidence: contracts.md:19-20, :40-41, :52-53.
- **R1-C18 — Low:** CIRCL proposed where Go's standard library serves. Evidence:
  design.md:103-104, decisions.md:18-19; `go.mod:3-5`.
- **R1-C19 — Low:** the ADR's tamper and replay tests were in no test row. Evidence:
  adr-proposal.md:52; design.md:136-147.
- **R1-S11 — Low:** the recipient's inputs to the HPKE `info` were not named. Evidence:
  contracts.md:52-53.
- **R1-S12 — Low:** no rule for key material in the metadata service's memory. Evidence:
  `scripts/guest/meta.sh`; design.md:95.
- **R1-S13 — Low:** the `GetVolume` refusal reached replica-only nodes. Evidence:
  design.md:47-49; #13 design.md:300-302, :312-315.
- **Nits (code 1-3, security 1-3):** stale "nothing is settled" text; "Q1–Q9"; numbers run into
  words; the generator unnamed and equal DEK halves allowed; `Version` outside the AAD.

Changes/resolution:
- **R1-S1, R1-C1:** the wrapped key moves to its own etcd record,
  `/flaminio/v1/volkeys/<id>/<first epoch>`, written by the publication transaction and deleted
  by the tombstone transaction; it alone makes a volume encrypted, and the header keeps today's
  JSON, so an older instance cannot erase it. This version always sets `Volume.encryption` (20),
  `{profile: "none"}` for plain, and new clients refuse an answer without it. The key source
  flag `serve --kek-file` is the switch, given in a second rollout (as #75's `--qos`); rollback
  is unsupported while an encrypted volume exists. Spec S12 widened. Tests
  `TestOldHeaderRewriteKeepsKey`, `old_instance_answer_is_refused`, `TestKeyRecordDamaged`.
  Risk named, not accepted: an older instance answering an older data plane, both against the
  rollout rule, serves an encrypted volume as plain.
- **R1-C2:** `encrypted` joins #76's identity and #80's comparison (whichever lands second);
  the CSI controller and `flaminio-meta create` check the answer's profile on `OK` and
  `AlreadyExists` (the CSI controller deletes only a volume it just created). Spec S1 gains the
  sentence. Tests `TestControllerEncryption`, `TestCreateEncryptedFlag`.
- **R1-S2, R1-C4:** `attach-encrypted` dropped; the record decides in `Engine::start_target`,
  which every attach path reaches; no line or operand changes, `[MS]` kept.
- **R1-C3, R1-S8, R1-S9:** #73's policy reads `encrypted` before any key call; audit events are
  a follow-up in `docs/followups.md` (issue to be filed); refusal logs bounded per caller and
  counted; `ReleaseVolumeKey`'s row for #73's table; intent's out-of-scope line corrected.
- **R1-C5, R1-S13:** the gate refuses only the `io` caller that is the record's target; the data
  plane sets `accepted_profile` in #13's constructor. Test `TestGetVolumeGate`.
- **R1-C6, R1-S6, R1-S7:** `--kek-file`, `/etc/flaminio/pki/kek` (`pki` only), generated once
  with `umask 077`, installed over ssh's stdin; the service checks type and length, `lab.sh`
  root 0600; `meta.sh up` writes the credential and flag only when the file exists. S5's "given
  as the token key is" replaced, as it cannot hold (decisions.md).
- **R1-C7:** the cipher is a field of the device's `Shared`, zeroized when it drops on every
  exit of `run_device`; no admission flag, borrow count or wait.
- **R1-C8:** the test table names file and case per rule; the cipher in
  `flaminio-node/src/crypt.rs`; `device_mode` for S16; `SOURCE_UNAVAILABLE` is
  `ResourceExhausted`, so neither client cycles.
- **R1-C9:** encrypted devices are added without the zero-copy flags, served by
  `engine_io_task`.
- **R1-C10:** `make encryption-check` and its steps, the zero-copy step in direct mode (S19 too),
  the benchmark commands.
- **R1-C11:** wrap before publication, record in the publication transaction.
- **R1-C12:** the in-flight cap dropped (Q12).
- **R1-C13:** forced verification written into S16.
- **R1-C14:** size arithmetic in contracts (at most 415 bytes per `Volume`); #80's fixtures gain a
  maximal descriptor; #80's allowance grows if they exceed it.
- **R1-C15, R1-S5:** `KEKVersion` derived from the KEK; the cluster's identity pinned in
  `/flaminio/v1/keysource`; another KEK is `INSTANCE_UNAVAILABLE`. Test `TestKeySourcePin`.
- **R1-C16, R1-S4:** a key source refused at start with `--insecure`; release and encrypted
  creation refuse an unauthenticated caller. Tests `TestServeRefusesInsecureWithKEK`,
  `TestReleaseUnauthenticated`.
- **R1-C17:** AAD, `Sealed` and HPKE `info` byte for byte; `Version` in the AAD.
- **R1-C18:** Go's standard `crypto/hpke`.
- **R1-C19:** the "Excluded threats" test row.
- **R1-S3:** #16's S15 named a prerequisite (spec S8, design).
- **R1-S10:** the ADR's Consequences state the XTS version leak, the metadata in clear and the
  KEK-holding nodes; claim reworded; S5's notice extended.
- **R1-S11:** `info` from the verified token and the request; the response must match. Test
  `release_for_another_first_epoch_is_refused`.
- **R1-S12:** S11: an instance with a key source is not dumpable (`PR_SET_DUMPABLE`,
  `LimitCORE=0`). Test `TestServeWithKEKIsNotDumpable`.
- **Nits:** all fixed (easy wording and reference changes). Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — f2857b5

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan reviewer, design-code focus (R2-C1…C6); plan reviewer, design-security focus
(R2-S1…S4).

- **R2-S1 — High:** round 1's named risk (an older instance serving an encrypted volume as
  plain) can be closed, and it understates the consequence: an older node plugin formats a
  device with no filesystem signature, so the encrypted volume would be destroyed; the lab's
  rollback deploys an older `meta.sh` without `--kek-file`, so "a binary-only rollback fails
  closed" does not hold there. Evidence: `control/csi/node.go:107–116`; `readHeader` fails on a
  header that does not decode (`meta/service/service.go:282–284`), ListVolumes skips it
  (`:853`), DeleteVolume still removes it (`:535–539`); design.md:117–126.
- **R2-S2 — Medium:** `pki issue` overwrites a VM's KEK with whatever the workstation holds; a
  regenerated workstation KEK (lost state dir, second workstation) destroys the only copies, and
  the pin only reports the loss afterwards. Evidence: design.md:282–285, :143–148, :300.
- **R2-S3 — Low:** an older instance's DeleteVolume leaves the key record behind, contrary to
  "deleted only by the transaction that deletes its header"; no sweep and no test. Evidence:
  `service.go:564–573`; contracts.md:65–66; design.md:80–81.
- **R2-S4 — Low:** the HPKE `info` is built from "its verified capability token", but the engine
  decodes the token unverified until #15. Evidence: `flaminio-node/src/engine.rs:1610–1614`;
  contracts.md:127–130; design.md:175–177; `origin/main:docs/issues/15/design.md:534–537`.
- **R2-C1 — Medium:** a wrap that fails after the claim leaves the hidden `Creating` header, and
  today's code then answers every retry `AlreadyExists` and the CSI path `NotFound`: the id is
  wedged until #76, so "a retry wraps a fresh key" is false. Evidence: design.md:64–69;
  `service.go:415–418`, `:431`, `:287`.
- **R2-C2 — Medium:** the release cannot sit in the synchronous `Engine::start_target`, and the
  new attach step (its wait, its fence against a dropped attach, its place relative to the
  checks and to #15's run mark) is unspecified; design.md gives two different places. Evidence:
  `engine.rs:1491–1506`, `:1596`; `ublk.rs:868–874`; design.md:111–115, :188–191;
  `origin/main:docs/issues/15/design.md:542–552`.
- **R2-C3 — Medium:** the first rollout's order is not stated, and new clients refuse every
  answer from an older instance, plain volumes included: plain attaches and plain creations
  fail whenever a client is upgraded before the last metadata instance. Evidence:
  design.md:88–91, :100–101, :117–126; `control/csi/meta.go:63–80`;
  `flaminio-metaclient/src/lib.rs:145–158`.
- **R2-C4 — Low:** the revision re-reads of `readVolume` and of the release refuse on unrelated
  header writes (another Attach, #13's `fenced`), with no stated outcome or code; Attach's
  post-commit `readVolume` must not fail. Evidence: design.md:50–53, :162–164;
  `service.go:738–745`.
- **R2-C5 — Low:** the source pin's lost create race and its timing relative to the claim are
  unspecified; a wrap after a lost race would orphan a volume. Evidence: design.md:143–148.
- **R2-C6 — Low:** key records can outlive their header: DeleteVolume deletes a header that does
  not decode (its first epoch unknown) and `kv.Op` has no prefix delete; records left by an
  older instance's deletion are never removed. Evidence: `service.go:534–536`, `:526–529`;
  `meta/kv/kv.go:24–29`; contracts.md:65–66.
- **Nit (code 1):** `TestSourceUnavailableIsNotCycled` passes without the change, against "each
  test fails without the change it names". Evidence: design.md:249; `control/csi/meta.go:74–79`.
- **Nit (code 2):** the seal cannot go "in the aligned buffer" after `shared_copy_of`, which
  returns an immutable `Bytes`. Evidence: design.md:219–220;
  `flaminio-uring/src/aligned.rs:56`.
- **Nit (code 3):** "from its verified token": unverified until #15. Evidence: design.md:176;
  contracts.md:127–128; `engine.rs:1611–1612`.
- **Nit (security 1):** `meta.sh up` runs as the ssh user and `/etc/flaminio/pki` is root 0700,
  so "only when that file exists" must be `sudo test -f`. Evidence: design.md:286–288;
  `scripts/guest/pki.sh:10`.
- **Nit (security 2):** say that `PR_SET_DUMPABLE` 0 is set before the KEK file is read.
  Evidence: design.md:197; spec S11.

Changes/resolution:
- R2-S1 and R2-C3 (fixed once): an encrypted volume's header writes `id` as
  `{"name","profile"}`, which no older version decodes (`id` has been a string in every version;
  `first_epoch` would be ignored by versions before #30). An older instance then serves and
  lists no encrypted volume and only deletes one, whatever it is started with; its decode error
  is `Unavailable`, so a client tries the next instance (the reviewer's "does not move on" is
  not so, the conclusion holds). Round 1's named risk is withdrawn in decisions.md, its
  consequence (a reformat) recorded. The new CSI node plugin refuses an `Attach` answer without
  field 20 before calling the data plane. Two rollouts stated in the design, S12 and the docs
  line: every metadata instance first, then data planes, node plugins, CSI controllers and
  `flaminio-meta`, then `--kek-file`; rollback in reverse, unsupported with an encrypted volume
  and failing closed. The CSI controller and `flaminio-meta` take an answer without field 20 to
  a plain request. Rule narrowed: S1's "an answer that does not say which is refused" now
  applies to an encrypted request only (as stated it refused a plain volume made as asked,
  against S4 and intent 2). Tests: `TestOldDecoderRefusesEncryptedHeader` (replaces
  `TestOldHeaderRewriteKeepsKey`), `TestNodeRefusesOldInstanceAttach`, the plain cases of
  `TestControllerEncryption` and `TestCreateEncryptedFlag`.
- R2-S2: the install writes `kek.new` as root and, when `kek` exists and `cmp -s` finds it
  different, removes it and fails naming the VM, `kek` unchanged; `encryption-test.sh` gains
  the case.
- R2-S3 and R2-C6 (fixed once): DeleteVolume lists and deletes `volkeys/<id>/` after its commit
  and in its header-absent branch, conditional on the header's absence, and a creation does
  under its claim, as for chunk records; the contract says a leftover from an older instance's
  deletion stays until the id is deleted or created again. `TestDeleteRemovesKeyRecord` covers
  an undecodable header, an older deletion and both follow-ups.
- R2-S4 and code Nit 3 (fixed once): #15's token verification is a named prerequisite beside
  #16's S15 (design, contracts, test table); the design says a wrong token value only fails the
  attach.
- R2-C1: a wrap that fails undoes the claim (chunk records, then the `Creating` header, under
  the claim's compare); every source check, the pin included, runs before the claim; a crash
  still leaves today's hidden header until #76. `TestCreateEncryptedCrashPoints` asserts the id
  is free.
- R2-C2: `start_target` split into today's checks, field 20's decision, then for an encrypted
  volume the release under `MetaWait::Attach { part: 2, attempt }` (an answer for a dropped
  attach is never opened: its recipient key went with it), then the start and
  `Output::Attached`; with #15, after the checks and before #15's run mark, fenced by #15's
  generation. Tests `release_answer_after_detach_starts_nothing`,
  `no_target_before_release_answer`.
- R2-C4: no revision compares. `readVolume` reads the key record only for a marked header and
  on its absence re-reads the header (`NotFound` or `Internal`); the release's re-read compares
  target, epoch and first epoch, a change `DENIED` (only a deposition changes them, so a retry
  would be refused anyway). Tests gain an unrelated header write.
- R2-C5: the pin is read before the claim; a lost create reads it again and compares; a match
  is cached. `TestKeySourcePin` gains the race.
- Nits: code 1 (the guard test named as such), code 2 (the seal between `copy_of` and
  `Bytes::from_owner`), security 1 (`sudo test -f`), security 2 (`PR_SET_DUMPABLE` before the
  KEK read, design and S11): all fixed.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — b87df55

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan reviewer, design-code focus (R3-C1…C3); plan reviewer, design-security focus
(R3-S1…S3).

- **R3-C1 — Medium:** "fails closed whatever the older instance is started with" is false for
  listings: an older instance leaves every encrypted volume out of ListVolumes, so a drain
  finishes with their replicas still on the node; with #72 an older deletion of such a header
  releases no capacity; round 1's "the only safe one deletes them first" was dropped. Evidence:
  design.md:144–155, S12; `meta/service/service.go:851–854`; `control/drain/drain.go:50–63`;
  `meta/cmd/flaminio-meta/main.go:433–440`; #72 design:174; the old design.md:121–122.
- **R3-C2 — Low:** `TestCreateEncryptedVolume` still asserts "header JSON as today", the opposite
  of the mark, and nothing checks that the service writes the mark:
  `TestOldDecoderRefusesEncryptedHeader` passes on a hand-built fixture. Evidence:
  design.md:312, :49–55; contracts.md, "The header's mark".
- **R3-C3 — Low:** the failed-wrap undo (R2-C1) is specified only against today's code; with #73
  the claim also writes `owners/<id>` and the usage, which the undo leaves charged, with no
  `delete.committed`; with #76 only its rollback deletes a creating header, with its root, and a
  helper wraps at finalization. Evidence: design.md:76–81, :83–85;
  `origin/main:docs/issues/73/design.md:88–97`; `origin/main:docs/issues/76/design.md:49`,
  `:62–81`.
- **R3-S1 — Medium:** the mark now decides whether a volume is encrypted, but nothing checks it
  against the key record and no test pins it; a new-version header write that lost `Profile`
  would serve ciphertext as plain and a node plugin would format it; the plan still says "the
  record decides"; a mark whose profile disagrees with the record's is unspecified. Evidence:
  design.md:49–61, :134, :312; contracts.md:153; decisions.md:66–70; `control/csi/node.go:107–116`.
- **R3-S2 — Medium:** "lists no encrypted volume" is not fail-closed: an older ListVolumes skips
  encrypted volumes and answers `OK`, `drain.Run` reports the node drained at `Remaining == 0`,
  and the operator removes it; a new drain controller reaching an older instance is exposed too.
  Evidence: `service.go:853`; `control/drain/drain.go:50`, `:181`; design.md:146–153.
- **R3-S3 — Low:** the KEK install checks content and mode but not length (a short workstation
  `kek` becomes the cluster's first), ordering across VMs (a refusal stops mid-loop after earlier
  VMs changed), concurrent runs (fixed `kek.new`) or durability (no `sync` before `mv`).
  Evidence: design.md:341–347; `scripts/lab.sh:575–588`.
- **Nit (code 1):** the decoder's answer to a mark whose `profile` is not `xts-aes256-v1`, or
  whose `name` is not the key's id, is unstated. Evidence: design.md:53–55; contracts.md, "The
  header's mark".
- **Nit (security 1):** the release's re-read refuses a header "gone"; it must say absent or
  `Deleting` (`readHeader`'s `NotFound`). Evidence: design.md:195–197;
  `meta/service/service.go:286–288`.

Changes/resolution:
- R3-C1 and R3-S2 (fixed once): round 1's instruction is restored: the only safe rollback
  deletes the encrypted volumes first, with this version (their key records go, and with #72
  their capacity). The design, S12 and the docs line name the exception: an older instance's
  ListVolumes omits encrypted volumes and answers `OK`, so while one serves and an encrypted
  volume exists no drain or node removal is safe. Claim narrowed: "fails closed whatever the
  older instance is started with" now covers per-volume calls only; S12 adds "but also
  unlisted". The `ListVolumesResponse` version marker was not added (cut before adding): under
  both orders no older instance runs beside an encrypted volume, and in a rollback against the
  rule the drain binary is already older, which no marker protects. Risk named, not accepted: a
  rollback against the rule lets a drain of any version retire a node holding an encrypted
  volume's replicas, all of them at RF 1.
- R3-S1, R3-C2 and code Nit 1 (fixed once): the mark and the key record must agree. `readVolume`
  also reads the record for a plain header (one Get, off the I/O path); a record under a plain
  header, or a mark whose `name` is not the id or whose `profile` is not `xts-aes256-v1` or not
  the record's, is `Internal`, never plain and never this profile. Every header write of this
  version keeps the mark (Attach, DeleteVolume, and #13's `fenced` and #76's finalization once
  landed); "header JSON as today" is gone, and "the record decides" (design, contracts) reads
  "the header's mark and the key record decide (they must agree)". Tests:
  `TestCreateEncryptedVolume` asserts the claim header unmarked and the published `id` the
  object; new `TestHeaderWritesKeepMark`; `TestOldDecoderRefusesEncryptedHeader` decodes the
  header the service wrote; `TestKeyRecordDamaged` gains the plain header with a record and the
  mismatched marks.
- R3-C3: whichever lands second wires the undo, #73's and #76's clear plans followed, not
  edited. With #73 the undo's transaction also deletes `owners/<id>`, lowers the charged usage
  and appends `delete.committed`. With #76 there is no undo (the reviewer's simpler option): a
  failed wrap at finalization leaves the operation to a retry, a helper or a DeleteVolume.
  `TestCreateEncryptedCrashPoints` covers both. Test statement narrowed: the spec's "a wrap that
  fails leaves the id free" adds "(with #76, its operation to a retry, a helper or a deletion)",
  since #76 alone removes a creating header.
- R3-S3: before any write to any VM, CA files included, `pki issue` checks the workstation `kek`
  is 32 bytes and compares it with every metadata VM's the run reaches, so a refusal changes no
  VM; remotely a `mktemp` file in `/etc/flaminio/pki`, `wc -c` 32, `sync`, compare again, `mv
  -f`. `encryption-test.sh` gains a 31-byte workstation KEK and a second VM refusing.
- Nits: code 1 (with R3-S1) and security 1 (absent or `Deleting`, design and contracts): fixed.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — e067381

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one plan-reviewer, design-code and design-security focuses together.

- **R4-C1 — Low:** with #73, the failed-wrap undo lowered the namespace usage and appended
  `delete.committed` in one transaction under the claim's compare alone, a read-modify-write of
  `quota/<ns>/<class>`, and left the event's outcome, chain and operation id and what a failed
  compare or a full chain leaves unnamed. Evidence: design.md "Creation"; #73 design.md
  `finishDeletion` and DeleteVolume's header-absent release (lines 127–141), which compare the
  quota's and the tail's revisions.
- **R4-C2 — Low:** with #76 the finalization is the publication, but the design listed it among
  the writers that keep the mark ("write the header they decoded", an unmarked creating header),
  so a literal implementation would publish a plain header with a key record, which
  `readVolume` answers `Internal`. Evidence: design.md "The header's mark and the key record" and
  "With #76"; #76 design, "Conditional helpers" (header byte-for-byte at C until visibility; the
  finalization writes the visible header).
- **R4-B1 — Low:** the new 32-byte pre-check stopped every `pki issue` on a lab whose CAs predate
  the change, since `pki issue` runs `pki init` only when `flaminio/ca.crt` is missing, so
  `issue w` (`cmd_partition`) and `issue c` (`cmd_data_node_up`) failed, the latter reaching no
  metadata VM at all. Evidence: `scripts/lab.sh:574`, `:807`, `:1228` on origin/main;
  design.md "The KEK".
- **Nit 1:** the remote step's compare-then-`mv -f` left a window in which two first installs of
  different KEKs both pass, and did not remove the `mktemp` file on a failed length check.
  Evidence: design.md "The KEK".

Changes/resolution:
- R4-C1: with #73 the undo deletes the chunks and the header under the claim's compare, as
  without it, then releases the creation's own headerless owner record through #73's existing
  release (DeleteVolume's no-header branch): its compares (header absent, owner record, chain
  tail, quota), its retry in the method's loop, its `delete.committed` `header-missing`. A full
  chain or spent retries leave the owner record and charge to #73's next release (the id's next
  creation, answering `Aborted`, or a DeleteVolume); the call answers the wrap's error.
  `TestCreateEncryptedCrashPoints` gains a failed wrap racing a second creation in the same
  namespace (usage exact) and the full chain. #73 is not edited.
- R4-C2: with #76, its finalization marks the header it publishes in the transaction that
  writes the key record, by the creator or a helper; it is listed as the publication, not among
  the writers that keep the mark (design, contracts). `TestHeaderWritesKeepMark` checks a
  finalization by the creator and by a helper. #76 is not edited.
- R4-B1: `pki issue` runs `pki init` when the CA or the `kek` is missing (each piece only if
  missing); the length check and compare-only pass run only when the run reaches a metadata VM.
  The refusal to replace a different KEK is kept. `encryption-test.sh` gains a state dir with
  CAs and no `kek`: `pki issue c` then `pki issue` both succeed.
- Nit 1: fixed. The remote step links the synced `mktemp` file as `kek` with `ln` (fails if
  `kek` exists), then compares; it removes the `mktemp` file on every path. `encryption-test.sh`
  covers both.

No rule or claim narrowed; no risk named or withdrawn. Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — 6b65a96

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Reviewed commit f80d7a6 (diff e067381..6b65a96). R4-C1, R4-C2, R4-B1 and the Nit fixed; nothing
found, Nits included.

Status: clear. Approval remains separate; no approval is inferred.
