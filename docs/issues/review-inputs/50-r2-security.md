# Issue #50 — plan review, round 2, focus: design-security

Scope: `git diff 9edd590..cc419ea -- docs/issues/50/ ':!docs/issues/50/report.md'`, checked
against the code in /home/jsfakian/Documents/src/flaminio-50 and the clear plans #15, #16, #73,
#74, #79 and #83 on origin/main.

## Earlier findings
- R1-S1 (Critical): mostly fixed. The key record keeps the wrapped key out of the header, and
  field 20 lets new clients spot an older instance. The residual risk is only named, and it can
  be closed (R2-S1).
- R1-S2 (High): fixed. The record decides in `Engine::start_target` (`engine.rs:1596`), with no
  new verb.
- R1-S3 (Medium): fixed. #16's S15 is a named prerequisite (contracts.md:130-132; design.md:177-180).
- R1-S4 (Medium): fixed. `--insecure` with `--kek-file` is refused, and so is a release or
  encrypted creation with no caller. The #73 row is added. Tests are named.
- R1-S5 (Medium): fixed. KEKVersion is an HMAC check value, with a pin under
  `/flaminio/v1/keysource`. `TestKeySourcePin` is named. See R2-S2 for the lab's overwrite.
- R1-S6 (Medium): fixed. The KEK goes over ssh's stdin into `pki` only, and the script test
  checks argv and `ENV_VARS`.
- R1-S7 (Medium): fixed. The credential and flag are written only when the file exists. Only
  type and length are checked in the service. One Nit below.
- R1-S8 (Medium): fixed. A follow-up is recorded and refusal logs are bounded per caller, with a test.
- R1-S9 (Medium): fixed. `TestEncryptedCreateFeedsTenantPolicy`, and the check runs before any
  key call.
- R1-S10 (Medium): fixed. The ADR's Consequences and S5's notice now state the three leaks.
- R1-S11 (Low): fixed in principle. The `info` is built from token and request, and the
  response must match. But the token is not verified today (R2-S4).
- R1-S12 (Low): fixed. `PR_SET_DUMPABLE` 0 and `LimitCORE=0` are set, with a test. One Nit on
  ordering below.
- R1-S13 (Low): fixed. The gate applies only to the target's `io`, and `accepted_profile` is set
  in #13's constructor.
- Nits (R1): fixed.

### R2-S1 — High
**The named risk "an older instance serves an encrypted volume as plain" can be closed, and its
stated consequence understates it: it is data loss, not only disclosure.**
design.md:122-126 and decisions.md ("Risk named, not accepted") say only that the application
gets ciphertext and the replicas get plaintext. Today's node plugin goes further: it formats any
device with no filesystem signature (`control/csi/node.go:107-116`). Ciphertext read as plain has
no signature, so an older plugin and data plane, answered by an older instance, run `mkfs` over
the encrypted volume and destroy it.

The path is also more realistic than "against the rollout rule" suggests. The lab's own rollback
deploys an older `meta.sh` together with the older binary. That older `meta.sh` writes no
`--kek-file`, so the claim that "a binary-only rollback that keeps [the flag] fails closed"
(design.md:119-120) does not hold for the lab's rollback path.

The code already gives a way to close the risk. Today's service treats a header that does not
parse as fail-closed:
- `readHeader` returns the decode error (`meta/service/service.go:282-284`), so GetVolume,
  Attach, ChangeIsr and MoveReplica all fail on that volume.
- `ListVolumes` skips the volume (`:853`).
- `DeleteVolume` still removes it (`:535-539`, whose comment states exactly this).
- Neither metadata client moves on for that error (`flaminio-metaclient/src/lib.rs:145-158`),
  so the attach fails.

**Fix:**
- Give an encrypted volume's header a tripwire: a known field encoded with a JSON type that
  today's `domain.Header` rejects. The new decoder reads it through its own type, and the key
  record stays the source of truth.
- Then every pre-#50 instance fails every call on an encrypted volume except its deletion. Move
  "Named, not accepted" to "what still holds".
- Add test `TestOldDecoderRefusesEncryptedHeader`: today's `Header` decode fails, and today's
  DeleteVolume still removes the volume.
- Also have the new CSI node plugin refuse an Attach answer with no field 20 before it calls the
  data plane.
- Correct the risk text to name the reformat.

### R2-S2 — Medium
**The lab install overwrites a VM's KEK with whatever the workstation holds, so a regenerated
KEK destroys every encrypted volume. The pin only reports the loss afterwards.**
`pki issue` runs `cat > kek.new && mv -f kek.new kek` on every metadata VM (design.md:282-285).
`pki init` makes a new KEK whenever `$LOCAL_STATE_DIR/pki/kek` is missing, for example after a
lost state dir or when the lab is driven from a second workstation. The next `pki issue` then
replaces the only copies of the cluster's KEK on fl-a, fl-b and fl-w. The pin turns the result
into `INSTANCE_UNAVAILABLE` (design.md:143-148), but the old KEK is already gone. The text
"never a replacement KEK" (design.md:300) applies only to the check.

**Fix:**
- The install refuses to replace an existing VM `kek` whose bytes differ: compare under `sudo`
  through stdin, or compare the check value with the pin. A different KEK stops `pki issue` with
  a message.
- Add an `encryption-test.sh` case: a second, different workstation KEK is refused and the VM
  file is unchanged.

### R2-S3 — Low
**An older instance's DeleteVolume leaves the key record behind, contrary to "deleted only by
the transaction that deletes its header".**
The tombstone transaction of today's code deletes the header and writes the tombstone only
(`service.go:564-573`). It does not know `volkeys/`. Keying by first epoch keeps the leftover
from matching a later incarnation, but the wrapped key of a deleted volume then stays in etcd
for good, with no sweep and no test. contracts.md:65-66 and design.md:80-81 claim more than
mixed versions give.

**Fix:**
- Say so in the contract.
- A creation that claims the id removes `volkeys/<id>/` leftovers under its claim, as it already
  does for leftover chunk records (`service.go:574-576` comment). Alternatively, the new
  DeleteVolume removes the whole prefix.
- Add a case to `TestOldHeaderRewriteKeepsKey`, or a sibling test, for an old-style deletion
  followed by a re-creation.

### R2-S4 — Low
**"Its verified capability token" is not what the engine has today.**
`start_target` decodes the token with `decode_unverified`; "Its signature is not checked here
(#15)" (`flaminio-node/src/engine.rs:1610-1614`). Contracts.md:127-130 and design.md:175-177 take
the HPKE `info`'s volume, first epoch, epoch and node from a "verified" token. Only #15
(origin/main, `docs/issues/15/design.md:534-537`) makes `Input::Attach` verify it.

The binding still fails closed, because the service takes first epoch and profile from the key
record. Even so, the stated property rests on a check that does not exist.

**Fix:** name #15's token verification as a prerequisite next to #16's S15, or say "decoded
(verified once #15 lands)" and state what holds before then.

## Nits
- design.md:286-288: `meta.sh up` runs as the ssh user, and `/etc/flaminio/pki` is root 0700
  (`scripts/guest/pki.sh:10`). "Only when that file exists" must be `sudo test -f`. Otherwise the
  KEK is silently never loaded, and the faked-VM script test would not notice.
- design.md:197 / spec S11: say that `PR_SET_DUMPABLE` 0 is set before the KEK file is read,
  not only "at start".

Reviewed commit: f2857b5cbdb5654b640b6ac14fccd1895f7aa2da
