# Issue #50 plan review — round 1, design-code focus

Scope: `docs/issues/50/` (intent, spec, design, contracts, adr-proposal, decisions) in
`/home/jsfakian/Documents/src/flaminio-50`, checked against that worktree's code and the clear
plans on `origin/main` (#13, #15, #16, #73–#76, #80, #84).

Checked and not findings: `Volume` 20–21, `CreateVolumeRequest` 20 and `GetVolumeRequest` 2 are
free (#72 as merged claims only tag 72 on the node messages; #13's design reserves 20–21 for
#50); no on-disk format, journal or chunk-state number is claimed; the I/O path gains no
metadata or KMS call and no lock; the dependency on Proposed ADR 0009 is stated (design, Policy
boundary; adr-proposal). Not verifiable offline: the crate versions `aes 0.9.4`,
`xts-mode 0.6.0`, `hpke 0.15.1`.

### R1-C1 — High

**An older metadata instance destroys the wrapped key.** The envelope lives only in the header
JSON. Every header write is a decode into `domain.Header` and a re-encode
(`meta/service/service.go:282`, `:541`, `:724`), with no `DisallowUnknownFields`. An instance
older than this change that runs one `Attach` on an encrypted volume writes the header back
without `encryption`: the only copy of the wrapped key is gone, and the record now reads as a
plain volume, which a new data plane would then serve as plaintext over ciphertext.

- Evidence: design.md:47 has only "All metadata instances must support the new profile before
  encrypted provisioning", with no mechanism, and nothing on rollback. Spec S5 lets an instance
  run without the KEK, so nothing forces every instance to be new.
- #75 names this same hazard and guards it (`origin/main:docs/issues/75/design.md:27`,
  `:48-61`: the switch, "rollback is unsupported", the older binary exits on the unknown flag);
  #13 does too (`design.md:1298-1299`). Here the dropped field is the volume's data.
- Fix: state the guard and the rollback rule as #75 does (a durable switch that admits
  encrypted creation only once every instance is new; no rollback while an encrypted volume
  exists). Keep the envelope in its own etcd key, written by the publication transaction and
  deleted with the volume, which no older code path rewrites; a header without the descriptor
  whose key record exists is refused, not served as plain. Add the test for that refusal.

### R1-C2 — High

**A creation can answer success with a plain volume for an encrypted request.**

- An older metadata instance drops the unknown `encrypted = 20` and creates a plain volume.
  The CSI controller discards the response (`control/csi/controller.go:231`, `_, err =`), so
  an `encryption: "true"` class gets plaintext replicas. The design forbids relying on unknown
  fields for the data plane (design.md:51-52) but relies on exactly that here.
- On `AlreadyExists` the controller compares size, chunk, replicas and min ISR only
  (`controller.go:239-247`): an existing plain volume of that name passes.
- #76's create identity lists its fields without encryption (`docs/issues/76/spec.md`, Terms),
  and #80's `CheckCreateVolume` compares "every future non-capacity identity field"
  (`docs/issues/80/contracts.md`). The plan never says `encrypted` joins that identity.
  design.md:43 cites "#76's creator ownership", which #76 does not have (S3: no creator, lock
  or lease; helpers advance it).
- Fix: `encrypted` is part of #76's canonical create identity and of #80's comparison. The CSI
  controller and `flaminio-meta create` check `Volume.encryption` in the answer, on both the
  OK and the AlreadyExists paths, and fail otherwise. Tests: a fake service that ignores the
  field; a same-geometry plain volume under an encrypted request.

### R1-C3 — Medium

**#73's two hand-offs are not taken** (`docs/issues/batch-2026-11-12.md:222`).

- #73's creation check allows "unencrypted … (every volume is, until #50)"
  (`docs/issues/73/spec.md` S2; `allow_unencrypted`, `contracts.md:145`, `:299`). #50 must
  feed `CreateVolumeRequest.encrypted` into it; the plan does not mention #73.
- #73 leaves key operations' audit events to #50 (`spec.md:133`, `decisions.md:110-115`:
  "each later issue owns its hook and test, recorded in `docs/followups.md`"). design.md:76
  says "Audit caller/volume/epoch/outcome" without saying whether that is #73's event append
  or a log line; intent.md:120-121 says the audit log and the unencrypted policy do not exist.
- Fix: a rule and a test for the policy check; for wrap and release, either #73's append or a
  follow-up recorded as #73 asks; correct the intent's out-of-scope line.

### R1-C4 — Medium

**The attach paths of an encrypted volume are not all specified, and the new verb is redundant.**

- The data plane also attaches at startup, `--attach VOLUME --token PATH`
  (`flaminio-io/src/ublk.rs:731-770`), which the lab's node restart uses
  (`scripts/guest/node.sh:60-63`). It has no profile argument; the design gives it no rule.
  Spec S19's "a restarted data plane's attach fails with S6's class" runs on this path.
- A new daemon given the plain `attach` line for an encrypted record (an older plugin) has no
  stated behaviour either.
- contracts.md:60 gives `attach-encrypted VOLUME TOKEN-BASE64 xts-aes256-v1` without the
  `[MS]` field of today's line (`control.rs:5`, `:84`; `control/csi/dataplane.go:107-114`;
  kept by #15, `design.md:349`). Followed as written, an encrypted attach loses #6's deadline.
- The `GetVolume` gate already refuses an older daemon, and the record tells a new one. The
  verb adds a second source of truth and the cases above.
- Fix: the daemon decides from the record on every path (`attach`, `--attach`); drop the verb,
  or define each verb-and-record combination and keep `[MS]`. Name the tests.

### R1-C5 — Medium

**The `GetVolume` gate would refuse replica-side reads.** design.md:47-49 and contracts.md:43
refuse every `io` caller of `GetVolume` on an encrypted volume that sends no
`accepted_profile`. #13 adds two replica-side `GetVolume` reads, the sweep's and the lookups',
through one constructor `get_volume_call` (`origin/main:docs/issues/13/design.md:312-315`), and
#75 sets its flag there. The plan names neither.

- If only the attach sets the field, a new replica node's sweep and lookups are refused for
  every encrypted volume ("a failed read drops every waiter", #13 design.md:534): released
  replicas are never reclaimed. An older replica node is refused in any case, against S17
  ("below the target, unchanged").
- Fix: gate only the caller that is the record's current target, so replica-side reads are
  untouched; or set the field in #13's constructor and state what older replica nodes lose.
  Add the test.

### R1-C6 — Medium

**How the KEK reaches the service contradicts the lab's unit, and its flag and path are unnamed.**

- Spec S5: "given as the token key is". That is `LoadCredential` under `DynamicUser=yes`
  (`scripts/guest/meta.sh:20-36`; kept by #16, `design.md:212-214`). The process reads a
  credential copy, not root's file, so design.md:57's check "root owner/no group/world
  access" cannot hold on what it opens (to be confirmed on the lab's systemd: the copy is
  either chowned to the service user or carries an ACL that shows in the group bits).
- A `LoadCredential` of a missing absolute path stops the unit. Then S5's "an instance that
  cannot read it still serves everything else" fails, and the lab step "move KEK files aside
  and restart all metadata instances" (design.md:161-162) brings no instance up, so the
  attach fails with no answer instead of S6's class.
- No server flag, credential name or path is given. #16 separates `pki` from `io-pki`
  (`design.md:153`, `:409`); the KEK must be stated as in `pki` only.
- Fix: name the flag and the file; say who checks ownership (lab.sh at install, the service
  only length and type); load the credential only when the file exists, or from a directory;
  cover it in `scripts/tools/encryption-test.sh`.

### R1-C7 — Medium

**The key's revocation rests on a mechanism the code does not have.** design.md:97-100:
"closes new crypto admission for the device generation, waits asynchronously for active
synchronous crypto borrows", "existing generation routing rejects late completions". No
generation exists in `flaminio-io` or `flaminio-node`; it is #15's plan, which the design does
not name as a dependency. Who counts borrows and who waits is not said.

- The code already gives the ordering: `run_target` returns only when every queue thread has
  ended, then `Arc::try_unwrap(shared)` succeeds (`ublk.rs:1050-1057`); a deposition kills the
  device (`:619-629`) and reaches the same point; `attach_and_serve` then detaches (`:888-895`).
- Fix: the cipher is a field of the device's `Shared`, zeroized on its drop there, on every
  exit of `run_device`; no admission flag, borrow count or wait. Say how it sits in #15's
  per-device `Shared` if #15 lands first.

### R1-C8 — Medium

**The test plan names few concrete tests, and one rests on a fake that does not exist.**

- design.md:136-147 names areas ("cipher tests", "node/metaclient tests", "RF2 memory/
  file-backed node tests"), with only `pkg/keysource` and `scripts/tools/encryption-test.sh`
  as files. No test name, so none can be checked to fail without the change.
- The crate of `VolumeCipher` and the "frontend adapter" is not named. The engine tests are
  `flaminio-node/tests/engine.rs`; `flaminio-node` does not depend on `flaminio-uring`
  (`flaminio-node/Cargo.toml:13-17`), where the locked memory goes; `flaminio-io` has one unit
  test and no engine harness (`ublk.rs:1753`).
- S16's row uses "fake ublk queue tests". The tag tasks take libublk's concrete `UblkQueue`
  (`ublk.rs:1207`, `:1360`); no fake or trait exists, and spec.md:169 says the frontend runs
  only in the lab. So S16, and a read path that skips the decryption, have no test before the
  lab.
- The Go clients cycle on every `Unavailable` too (`control/csi/meta.go:63-86`); S6's
  no-cycle rule at creation has no named Go test.
- Fix: name the module and each test (file and function) per rule; for S16 a pure function
  that picks the device's mode and verification from (encrypted, flags), unit-tested, with the
  lab check for the rest.

### R1-C9 — Low

**S16's mechanism is not chosen.** design.md:126 says "copied/staged". Zero copy is fixed per
device when it is added (`add_device`, `ublk.rs:906-910`) and per queue (`serve_with`,
`:1098`). Fix: an encrypted device is added without the zero-copy flags and served by
`engine_io_task`; no staged decrypting variant of `engine_io_task_zc`.

### R1-C10 — Low

**The lab plan is not runnable as written.** design.md:158-168 names no make target, check
script or `lab.sh` command, and omits S19's bullet "on a `--zero-copy` node the pattern reads
back, and S16's line is logged". Under #74 zero copy works only in direct mode
(`docs/issues/74/spec.md` S7), which that step must say. Fix: name the commands and add the
step.

### R1-C11 — Low

**The separate envelope write is not needed.** design.md:39-43 persists the envelope in its own
conditional write before publication and then tracks a new header revision. The first epoch is
the claim's revision, known before the publication write (`service.go:415`, `:465-468`): wrap
then, and put the descriptor in that same write. One crash point and one revision fewer; a
resumed creation makes a new key, as nothing was written under the old one.

### R1-C12 — Low

**The cap of 8 key calls in flight contradicts Q12.** decisions.md:39-40 settles "a bound on
calls in flight, with step 2". design.md:67, contracts.md:31 and the S3/S5/S6 test row add it
in this step. Fix: drop it here, or record the change in decisions.md and give it a spec rule.

### R1-C13 — Low

**Forced verification has no spec rule.** design.md:126-127 forces verified reads "regardless
of … no-verify flags"; S14 says "verified as today", and today `--no-verify` skips it
(`ublk.rs:996-999`, `:1678`). Fix: add it to S14 or S16, or remove it.

### R1-C14 — Low

**"Within #80's admission budget" is asserted, not shown.** contracts.md:45. #80's header
allowance is 2048 bytes (`docs/issues/80/design.md:30`), and its contract requires "bounded
complete shapes and revised fixtures" before a schema addition (`contracts.md`, Layered schema
profiles). The envelope alone may be 1024 bytes (contracts.md:21). Fix: give the arithmetic
and extend #80's worst-response fixtures with a maximal descriptor.

### R1-C15 — Low

**Where the static source's `ID` and `KEKVersion` come from is not said.** contracts.md:5-6
bounds them; the source is a raw 32-byte file (design.md:56-57). Two instances given
different files answer `INVALID_ENVELOPE`, which is never retried elsewhere. Fix: say whether
they are flags or derived from the KEK, and how S5's "the same on every instance" is checked.

### R1-C16 — Low

**Release with authentication off.** With `TrustDomain` empty `Authorize` passes every call
(`auth.go:42-44`), and a handler written like `ChangeIsr` skips its caller check
(`service.go:801-804`). design.md:82 says "required mTLS" without the mechanism. Fix: a key
source is refused at start with `--insecure`, and `ReleaseVolumeKey` refuses a caller without
an identity; test both.

### R1-C17 — Low

**contracts.md is not exact where it says so.** The order and widths in
`packed_nonce_ciphertext` (:40-41); the length prefix of Kind, ID and KEKVersion in the AAD
(:19-20); the widths of the epochs in the HPKE `info` (:52-53); `Envelope.Version` is outside
the AAD. Fix: give each encoding byte for byte, as the Go–Rust vectors need.

### R1-C18 — Low

**CIRCL is proposed where the standard library serves.** design.md:103-104 and
decisions.md:18-19. The module is at `go 1.26.0` with toolchain 1.27.3 (`go.mod:3-5`), whose
`crypto/hpke` implements RFC 9180 with this suite. Fix: make it the Go candidate.

### R1-C19 — Low

**The ADR's tamper and replay tests are in no test row.** adr-proposal.md:52 ("Tamper/replay
tests must demonstrate the explicitly excluded data threats") and Proposed ADR 0009's
Confirmation ask for them; design.md:136-147 has none. Fix: add the row and name the tests.

## Nits

1. Text that still says nothing is settled, beside the "Settled" lines: spec.md:3-5 and :173;
   design.md:147 and :170-172; adr-proposal.md:11, :14 and :44-45. An implementer cannot tell
   which holds.
2. spec.md:5 says "Q1–Q9"; there are twelve questions.
3. Numbers run into words throughout design.md, contracts.md and adr-proposal.md
   ("Generate64", "a1s context", "max8", "random12-byte", "defaultfalse"); "a1s" and "max8"
   read as identifiers.

Reviewed commit: 9edd5903655a66e3ffbfe6c1d0c47d5d5050b266
