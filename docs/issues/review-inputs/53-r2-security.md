# #53 plan review, round 2, design-security

Scope: `git diff 525f70c..0c4dfc0 -- docs/issues/53/ ':!docs/issues/53/report.md'`, checked against
`pkg/pki/pki.go`, `meta/service/auth.go`, `flaminio-net/src/identity.rs` and #52's clear plan.

## Earlier findings
- R1-S1: fixed. S1 has the deployment prerequisite, the validation profile checks non-S3 interfaces,
  and S9 and the lab say that fl-w proves S3-API enforcement only. Gaps remain in R2-S1, R2-S5 and R2-S7.
- R1-S2: fixed by `completed-roots`. A residual denial of service is R2-S4.
- R1-S3: fixed. S5 now binds the worker. The owner's cap is unchecked and named as a risk.
  PutObjectLegalHold is in the Deny list and is probe row 8. ARCHITECTURE §14.1 is in the docs list.
- R1-S4: partly fixed. Row 1 reads retention and the bucket default is refused, but a legal hold
  still masks the delete probe (R2-S2).
- R1-S5: fixed. The evidence file, its checks, the "attestation" wording and the unprobed list are
  defined. The residual is R2-S8.
- R1-S6: fixed. Key references are deferred with #52's S8, and the encrypted refusal is kept and tested.
- R1-S7: fixed. Commands, credentials, buckets, canaries, id quoting and `down` are specified. New
  gaps are in R2-S1, R2-S6 and R2-S7.
- R1-S8: fixed. The residual is R2-S3.
- R1-S9: fixed. The followups row is added and no metadata is mutated.
- Nit (descriptor naming): fixed.

### R2-S1 — Medium
**The lab's `repo init --immutable` cannot pass as written. The one way to make it pass is to claim a qualification that the plan says does not exist.**
- **No profile is qualified.** The worker accepts the evidence only if "the backend profile is one it
  knows as qualified" (contracts.md:170-171; design.md:171 `unknown_backend_profile_refuses`). Rows
  5–7 and 11 also need the "profile's qualified" refusal codes and order (contracts.md:135-140). The
  plan says no profile is qualified, and that the fl-w store cannot pass the non-S3 interface check
  (spec.md:159-161; contracts.md:207-212). So lab step 3 ("the probes pass", design.md:235-236) exits
  3. Otherwise the worker would have to ship SeaweedFS as "qualified", and every output of a
  cluster-reachable store would carry that claim.
- **The locked canary expires too early.** The lab locks `canary/locked` until "the check's start
  plus 40 min" (design.md:223). Row 4 requires R >= store time + 1 h (contracts.md:126). So the
  first probe fails as stale (exit 1), before any backup.

**Fix:** add a candidate profile state, e.g. `backend_profile_id` `seaweedfs-<version>` with
`qualified: false`. The worker accepts it only with an explicit lab flag. Every JSON output reports
it as `qualified: false` and never as an immutable guarantee. Test both: refused without the flag,
labelled with it. Lock `canary/locked` for at least 1 h plus the check's duration (`down` already
waits), or make row 4's margin a profile value the lab sets under 40 min.

### R2-S2 — Medium
**A legal hold on `canary/unlocked` still lets a delete-capable credential pass row 3.**
- **The hold masks the delete.** Row 1 requires only "none, or retain-until before store time"
  (contracts.md:123). It never requires the hold to be OFF, although `retention()` returns the hold
  state (contracts.md:88). With a hold set on U, DeleteObjectVersion is refused by the lock. AWS
  answers a lock-refused delete with AccessDenied, which row 3 accepts. The policy denies the
  uploader PutObjectLegalHold on that canary (contracts.md:112-113), but the owner or any other
  principal can still set a hold.
- **"Expired" is not needed.** Row 1 also admits an expired retention with no margin for the store's
  time uncertainty. The owner writes U with no retention, so "expired" serves no purpose.
- **Not tested.** `unlocked_canary_locked_is_uncertain` covers only a bucket default and the
  uploader (design.md:171).
- **Multi-delete is not probed.** The table never probes DeleteObjects (multi-object POST
  `?delete`). S3-compatible backends often authorize it on a separate code path.

**Fix:**
- Row 1 reads GetObjectRetention and GetObjectLegalHold of U. Require no retention and the hold OFF;
  drop "or expired". Row 4 also requires L's hold OFF, so that row 8 really changes nothing.
- Add a "held" case to the test.
- Add a row: DeleteObjects naming U, AccessDenied required.

### R2-S3 — Low
**The descriptor authority falls through to a later version when the oldest one does not authenticate.**
contracts.md:29-30 says the authority is "the oldest version ... that authenticates", but spec S6
(spec.md:95) says "its oldest version". Under the contracts' wording, a corrupt oldest version or a
mis-supplied key promotes the next version that authenticates, possibly a forged `immutable:false`
one. A bucket-only reader with no recorded version (S8) is then downgraded.

**Fix:** the authority is the oldest version. If it cannot be read or does not authenticate,
refuse (exit 3, or exit 1 on a transport error); never fall through. Add
`descriptor_oldest_unauthentic_refuses`. The guide states that the owner's cleanup never deletes
that version while the repository is in use.

### R2-S4 — Low
**One forged newer root can force full backups for the length of its lock.**
contracts.md:79-80 requires #52's *newest* compatible base to be listed in `completed-roots`, or the
backup is full. #52 never says what "newest" orders by (#52 design.md:124). If it orders by the
manifest's self-reported `created_at`, a forged far-future root stays newest while it is locked.
Every later backup is then full, which costs time and quota.

**Fix:** choose the newest compatible base among the `completed-roots` entries, ordered by the
store's LastModified. Report `base_not_own` only when no entry is eligible. Extend
`forged_root_never_base`: the next backup stays incremental from the worker's own newest root.

### R2-S5 — Low
**The non-S3 interface check covers delete only.**
The check requires each interface to be unreachable or to refuse "an unauthenticated delete of a
locked version" (spec.md:49-50; contracts.md:203-204). An admin path that rewrites a locked
version's bytes or its retention passes the check. #52's own lab step 6 alters a locked block
version in exactly this way. The archive then fails AEAD on restore, so its data is lost while the
lock still reports intact.

**Fix:** require each interface to refuse an unauthenticated delete, overwrite, or retention or
metadata change of a locked version.

### R2-S6 — Low
**`down` lists and deletes while the uploader is still live, and ignores legal holds.**
design.md:250-253 has `down` list the store's versions, then delete them, with the uploader
credential still accepted. Between the listing and the deletes, a running worker or the attempts
script can extend a version's retention. Step 4's legal-hold attempt (design.md:240) can also leave
a hold, which "locked past the store's Date" does not cover. Nothing says that a refused delete
stops `down` before it stops the store and deletes the volume file, which would be the early wipe
the design forbids.

**Fix:**
- `down` first removes the uploader identity from the store's configuration, or refuses while a
  worker runs.
- Its listing includes legal holds; the owner clears a hold that the attempts set.
- On any refused delete, it exits before stopping the store or deleting anything.
- Script test: a fake refused delete leaves the volume file and both credentials in place.

### R2-S7 — Low
**The second SeaweedFS instance's listeners below S3 are unspecified.**
Only `BACKUP_LOCK_STORE_PORT` is named (design.md:204). The instance's master, volume and filer
HTTP and gRPC listeners would collide with #52's instance on fl-w. If they are bound to fl-w's
network address, fl-a and fl-b can delete locked versions below S3 during the check. That
undermines step 4's "exact versions intact" and step 6's "refused".

**Fix:** bind every non-S3 listener to 127.0.0.1, on `lab.conf` ports distinct from #52's. The
script test checks those flags. This simply closes, for the lab, the part of the named R1-S1 risk
that reaches beyond fl-w.

### R2-S8 — Low
**"A backend upgrade needs a new repository" has no detection; the backend identity is self-declared.**
`backend_profile_id` and `backend_version` come only from the owner evidence (contracts.md:167-177).
The worker detects an upgrade only if the owner edits the evidence. A file that names a qualified
profile for an unqualified backend makes the worker apply that profile's codes and claim it. The
probes still run.

**Fix:** state that an upgrade is detected only through the owner's evidence (the owner's duty, in
the guide). Report the profile in the output as attested (`backend_profile_source: evidence`).
Where a profile defines a store-reported version, compare against it as well.

## Nits
- design.md:171 `object_store_has_no_destructive_call` ("only `CanaryProbe` builds a delete") is
  false for the same crate's `examples/lock_lab.rs`. Scope the check to the library.

Reviewed commit: 36b64f06e32886259e255b96f7136795eb390230
