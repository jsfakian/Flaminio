# Issue #50 plan review — round 2, design-code focus

Scope: `git diff 9edd590..cc419ea -- docs/issues/50/ ':!docs/issues/50/report.md'` in
`/home/jsfakian/Documents/src/flaminio-50`, checked against that worktree's code and the clear
plans on `origin/main` (#13, #15, #72, #73, #75, #76, #79, #80).

Checked and not findings: the protobuf tags (`Volume` 20–21 reserved for #50 by #13's design,
"Field numbers"; #72 takes only `NodeHeartbeatRequest`/`Node` 72 and leaves `Volume`,
`CreateVolumeRequest` and `GetVolumeRequest` unchanged, `capacity-contracts.md:98-103`; #73
`CreateVolumeRequest` 30; #75 40 in all three; #79 `CreateVolumeRequest` 9;
`GetVolumeRequest.accepted_profile = 2` collides with nothing). `Header.FirstEpoch` exists
(`meta/domain/domain.go:47`), so `/volkeys/<id>/<first epoch>` is addressable from the header.
The key record and the source pin use `kv.Cmp{Rev: 0}` compares and ride the existing
conditional publication and tombstone transactions (`service.go:468`, `:570`); nothing is
read-modify-write. The size arithmetic is right (KeyEnvelope 328 at version 1, descriptor 376,
field 20 380, field 21 35, plain 9 bytes), and the residual against #80's 2048-byte allowance
is deferred to #80's fixtures as #80's "Layered schema profiles" asks. `crypto/hpke` is in the
toolchain the module pins (go 1.26, toolchain 1.27.3; present in the local 1.27.4 GOROOT, with
`NewSender`/`NewRecipient` and no info-length cap). The zero-copy exclusion matches the code
(`add_device` flags `ublk.rs:906-910`, `serve_with(…, zero_copy)` `:1071-1122`, `verify` in
`Shared` from `--no-verify` `:996`). The `Shared` drop ordering for revocation holds
(`run_device` `:1050-1057`). The `vssh … <kek` install works (`scripts/lib.sh:145`).

## Earlier findings

- R1-C1: fixed (own key record; rollout switch and unsupported rollback stated).
- R1-C2: fixed (`encrypted` joins #76/#80 identity; CSI and `flaminio-meta` check the answer).
- R1-C3: fixed (policy input; audit events a follow-up; intent corrected).
- R1-C4: fixed (no new verb; the record decides on every path) — but see R2-C2.
- R1-C5: fixed (gate only on the target's `io`).
- R1-C6: fixed (`--kek-file`, path, conditional `LoadCredential`, installer checks owner/mode).
- R1-C7: fixed (cipher in `Shared`, no admission machinery).
- R1-C8: fixed (tests named per rule; `device_mode` table test) — one test cannot fail, Nit 1.
- R1-C9: fixed.
- R1-C10: fixed (`make encryption-check`, `vol-create ENCRYPT=1`, direct-mode zero-copy step).
- R1-C11: fixed (wrap before publication, one transaction) — but see R2-C1.
- R1-C12: fixed.
- R1-C13: fixed (S16 says it).
- R1-C14: fixed.
- R1-C15: fixed (`KEKVersion` from an HMAC check value; cluster pin) — see R2-C5.
- R1-C16: fixed.
- R1-C17: fixed.
- R1-C18: fixed.
- R1-C19: fixed.
- Nits 1–3: fixed.

### R2-C1 — Medium

**A wrap that fails after the claim wedges the volume id on today's code; "a retry wraps a
fresh key" is false there.** The wrap now runs after the claim and the chunk writes
(design.md:64-69). A `SOURCE_UNAVAILABLE`, the 1 s bound, or a pin mismatch found at the wrap
returns with the hidden `Creating` header in place. Today a retry's claim then fails
(`Rev: 0` compare, `service.go:415-418`) and, the header not `Deleting`, answers `ErrExists`
(`:431`); the CSI controller's `AlreadyExists` path then gets `NotFound` from `GetVolume`
(hidden header, `readHeader` `:287`) and fails for good. Before #76 nothing resumes or removes a
`Creating` header. What used to need a crash or an etcd error mid-creation becomes a routine
key-source outage. `TestCreateEncryptedCrashPoints` ("a retry wraps a new key") cannot pass as
written without #76.
- Fix: on a wrap failure, undo the claim in the handler: delete the chunk records and the
  `Creating` header conditional on the claim revision (`deleteChunks` with `cmp`, then the
  header). Or make #76 a stated prerequisite of encrypted creation. Do every source check,
  the pin included, before the claim. The test then asserts the id is free after a failed wrap.

### R2-C2 — Medium

**The release cannot sit in `Engine::start_target`, and the new attach step is not specified.**
`start_target` is synchronous: when both `GetVolume` and `ListNodes` are in, it builds and
starts the target, and its caller pushes `Output::Attached` and drains I/O at once
(`flaminio-node/src/engine.rs:1491-1506`, `:1596`). A `ReleaseVolumeKey` round trip has to come
between the descriptor check and the target's start, and before `Output::Attached`, since the
frontend takes the cipher after it (design.md:188-191). So design.md:111-115 ("the check above
is in `Engine::start_target`") and :188-191 ("after `GetVolume`") give two different places,
and neither names:
- the new wait (a third `MetaWait::Attach` part or a new variant) and its `attempt` fence, so
  that a release answer for a dropped attach (#6's `Input::Detach`, `ublk.rs:868-874`) is
  zeroized and never starts a target;
- whether the release goes before `start_target`'s own checks (target, geometry, token epoch),
  or after them with the target held unstarted;
- its order with #15's clear plan, which already holds the target unstarted between `GetVolume`
  and `Input::Marked` (`origin/main:docs/issues/15/design.md:542-552`): release before or after
  the run mark, and the generation the release's answer carries.
- Fix: split the decision (at the `GetVolume` answer) from the start: check field 20, send the
  release, hold the attempt, open the key, then call `start_target` (with #15: after the mark).
  Name the wait and fence, and add `engine.rs` tests: a release answer after `Detach` starts
  nothing and drops the key; no target exists before the release answer.

### R2-C3 — Medium

**The first rollout's order is not stated, and the new clients refuse every answer from an older
instance, plain volumes included.** A new data plane fails every attach whose `GetVolume` an
older instance answers ("field 20 absent: `AttachFailed`", design.md:100-101); the CSI
controller, on `OK` without field 20, deletes the volume it just created and fails, plain
requests included (:88-91); `flaminio-meta create` exits 1. Neither client moves on for a
successful answer (`control/csi/meta.go:63-80`, `flaminio-metaclient/src/lib.rs:145-158`).
During a rolling upgrade where any data plane or CSI controller is upgraded before the last
metadata instance, plain volumes stop attaching and plain creations fail. design.md:117-126
orders only the second rollout (the KEK switch).
- Fix: state the first rollout's order (every metadata instance before any data plane, CSI
  controller or `flaminio-meta`) in the design, spec S12 and the docs; and let the CSI
  controller and `flaminio-meta` accept a missing field 20 for a plain request (an older
  instance made what was asked). Add the case to `TestControllerEncryption`.

### R2-C4 — Low

**The revision re-reads refuse on unrelated header writes, with no stated outcome.**
- `readVolume`'s pair read requires the header's revision unchanged (design.md:50-53). Any
  concurrent header write (another Attach; #13's `fenced`, written in `volumes/<id>`) changes
  it, and the design does not say whether `readVolume` then retries or fails. Attach's
  post-commit `readVolume` must not fail after its commit (`service.go:738-745`). Only the
  header's existence with the same `FirstEpoch` is needed: the record of an incarnation is
  written with its publication and deleted only with its header.
- The release refuses on any revision change (design.md:162-164), so the same unrelated
  writes refuse a valid release; its code is unnamed.
- Fix: compare presence and `FirstEpoch` in `readVolume`; in the release, compare target,
  epoch and first epoch on the re-read; name the code (`Aborted`, retried by the attach). Test
  both with a concurrent unrelated header write.

### R2-C5 — Low

**The source pin's lost race and its timing are unspecified.** design.md:143-148: "absent, it
creates it (compare: absent)". If two instances with different KEKs both read it absent, the
loser's create fails, and the design does not say it reads again; an implementation that wraps
anyway makes the orphaned volume the pin exists to prevent. "Before its first wrap" also does
not say whether the result is cached or when it is checked relative to the claim (R2-C1).
- Fix: on a failed create, read again and compare; or compare the pin's revision (or create it
  with `Rev: 0`) inside the publication transaction itself. Check it before the claim. Add the
  race to `TestKeySourcePin`.

### R2-C6 — Low

**Key records can outlive their header.** `DeleteVolume` deletes a header that does not parse
(`service.go:534-536`): its `FirstEpoch` is unknown, so the tombstone transaction cannot name
the record, and `kv.Op` has no prefix delete (`meta/kv/kv.go:24-29`). Records left by an older
instance's deletion are never removed either. The wrapped key then stays in etcd for good,
against contracts.md:65-66 ("deleted only by the transaction that deletes its header").
- Fix: list `/flaminio/v1/volkeys/<id>/` and delete what is found, in the tombstone transaction
  and in `DeleteVolume`'s header-absent branch (as `deleteChunks` does, `:526-529`). Test with
  an unparseable header.

## Nits

1. design.md:249 says "Each test fails without the change it names", but
   `TestSourceUnavailableIsNotCycled` tests today's client behaviour for `ResourceExhausted`
   (`meta.go:74-79`) and passes without the change. The mapping it relies on is
   `TestKeyErrorCodes`'s.
2. design.md:219-220: "seals … in the aligned buffer the write is copied into (`ublk.rs:1263`)":
   `AlignedBuf::shared_copy_of` returns an immutable `bytes::Bytes`
   (`flaminio-uring/src/aligned.rs:56`). The seal goes between `copy_of` and `Bytes::from_owner`.
3. design.md:176 and contracts.md:127-128 build `info` "from its verified token". Until #15
   lands, the engine decodes it unverified (`decode_unverified`, `engine.rs:1611-1612`). Name
   #15, or say the token is unverified until then.

Reviewed commit: f2857b5cbdb5654b640b6ac14fccd1895f7aa2da
