# #53 plan review, round 4 (incremental), design-code and design-security

Scope: `git diff 5b47237..b0f608e -- docs/issues/53/ ':!docs/issues/53/report.md'` (HEAD afb3fc5 adds
only report.md). Checked against origin/main's Makefile (`hygiene`, lines 110-119) and docs/ci.md,
and #52's clear plan (flaminio-52 at ae97cf5: design.md:330-360, contracts.md:495-515).

## Earlier findings
- R3-B1: fixed (the profile names absence answers; other 404s, NoSuchKey and NoSuchVersion stay uncertain; tests extended). Gap in R4-B2.
- R3-S1: fixed (`immutable_guarantee`, `completed-unqualified`, list/verify/restore never refuse; tests for six commands). Gap in R4-S1.
- R3-C1: fixed (`put_allowance` = 2u, new D = Date + budget, the risk reworded, "retry with explicit bounds" cut, `deadline_edges` boundary). u >= 1 s (zero refused, contracts.md:50-53), so the allowance is never zero.
- R3-B2: fixed in layout (one loopback `weed server`, a `socat` forwarder, the S3 gRPC port named, `ss -Hltnp` by PID, flags marked to be checked). Gap in R4-B1.
- R3-B3: fixed (only `up` writes the configuration; `start` never regenerates it; script test). Gap in R4-B3.
- R3-C2: fixed for the flag and step 7's files. Gap in R4-C1.
- R3-C3: fixed (the runtime test cut, a `make hygiene` grep added). Gap in R4-C2.
- Nits (multi-delete "in neither" case; `ca.crt` after `backup-owner`): fixed.

### R4-B1 — Low
**The lock store's PID-filtered listener check can pass with nothing checked.**
- **No presence requirement.** The check refuses only when it finds a bad listener under the recorded
  PIDs (design.md:246-250). It never requires the expected ones to be present. Two realistic cases
  then pass without checking anything:
  - The recorded PID is a wrapper's. The store runs as `flaminio-backup-store`, so `sudo -u … weed
    server &` gives `$!` = sudo's PID, while `ss -p` shows weed's.
  - The check runs before `weed server` has opened its later listeners. Filer, S3 and S3 gRPC start
    after the master.
- **The script test misses it.** With a fake `ss` (design.md:338-341), the test proves only the
  filter, not that the recorded PID is the listening process.

**Fix:** after starting, wait up to a stated bound until every named port is listening under the
recorded PIDs: weed's eight on 127.0.0.1, and socat's one on fl-w's address. Refuse (stop both) if
any is missing, then apply the off-loopback rule. Record the PID of the process that listens (exec
under `runuser`/`setpriv`, or read the PID from the port). Script-test that a recorded PID with no
listeners, or with only some, refuses.

### R4-B2 — Low
**Nothing shows that a profile's absence answer means absence.**
- **What the lab records.** Lab step 3 only records the answers rows 1 and 4 got "against the
  candidate's absence answers" (design.md:303-306). The validation profile lists the absence answers
  but no test that tells them apart (contracts.md:254-256).
- **How a profile could be wrong.** It could take as "no hold" a code the store also returns for
  "unsupported", a held version or a missing version (for example NotImplemented, NoSuchVersion, or
  the same 404 for everything). Row 1's hold check would then always pass. That is exactly the case
  where a held U masks a delete-capable uploader (contracts.md:160-163).

**Fix:**
- The validation profile requires the same request to answer differently on three versions: one
  with no retention or hold (the absence answer), one with a retention or a hold (a body), and a
  deleted version.
- A unit test over the shipped profile table refuses an absence answer equal to NotImplemented,
  NoSuchKey, NoSuchVersion, AccessDenied or any 5xx.

### R4-C1 — Low
**Step 7 can still exit 1 rather than 3.**
- **No uploader rights on the bucket.** The owner steps apply the uploader policy "rendered for
  `flaminio-lock`" only (design.md:283-284). So the uploader's read-back of `flaminio-nolock` can be
  AccessDenied (uncertain, exit 1). Also, `nolock.evidence.json`'s `uploader_policy` ("the applied
  document, verbatim", contracts.md:196) names a policy never applied to that bucket.
- **No code for a missing lock.** A bucket without Object Lock answers GetObjectLockConfiguration
  with an error code: ObjectLockConfigurationNotFoundError (404) on S3. That is the same gap R3-B1
  fixed for rows 1 and 4. The profile names no such code, and an unnamed code fails "exit 1 or 3"
  (contracts.md:217-218).
- **No exit code.** The exit-3 list does not name a missing lock (contracts.md:249-251), and
  `refuses_unlocked_bucket` gives no exit code (design.md:188).
- **No writer for `nolock.conf`.** Nothing says who writes it (design.md:278-279).

**Fix:**
- The owner steps also apply the uploader policy rendered for `flaminio-nolock`, and the nolock
  evidence carries that document.
- The profile's codes name the answer for no lock configuration, mapped to exit 3. Another 404
  (NoSuchBucket) stays exit 1.
- `refuses_unlocked_bucket` asserts both.
- Name the writer of `nolock.conf` (the check, from `lock.conf` with the bucket changed).

### R4-S1 — Low
**When `immutable_guarantee` is true is undefined, and an unknown id's `candidate_state` is untested.**
- **The true case.** contracts.md:223 says only that a qualified profile's outputs may say `true`.
  It does not say whether that holds for a qualified `backup` that ends with a late root (exit 1), or
  for a `list` in which some archives are incomplete or not covered. A consumer reading the one field
  (R3-S1's concern) sees a guarantee there.
- **The unknown-id test.** `unknown_backend_profile_refuses` asserts `qualified` and
  `immutable_guarantee` for list/verify/restore but not `candidate_state` (design.md:188), although
  the rule at contracts.md:223 forbids `completed` there.

**Fix:** `immutable_guarantee: true` only under a qualified profile, and only for a completed,
covered archive. Make it per archive in `list`; `false` in every failing output. Test it with an
injected qualified profile. Add `candidate_state` never `completed` to the unknown-id case.

### R4-C2 — Low
**The hygiene grep leaves out bucket deletion, ACLs and a legal hold sent on PutObject.**
- **What it claims.** The rule is that `ObjectStore` has "no delete, shortening, bypass, legal-hold
  or bucket-mutation call", and that `CanaryProbe` is "the only code that sends a destructive, bypass
  or bucket request" (contracts.md:121, :128).
- **What it misses.** The word list (design.md:188) misses:
  - `delete_bucket`, `delete_bucket_policy` and the other `delete_bucket_*` calls;
  - `put_object_acl`;
  - `put_public_access_block` and `delete_public_access_block`;
  - PutObject's `object_lock_legal_hold_status` (a legal hold sent with the PUT).
- **Docs not updated.** The Makefile's `hygiene` help (Makefile:111) and docs/ci.md's `hygiene`
  entry (line 23) describe only cargo-deny and the like. The design does not update either.

**Fix:** add `delete_bucket[a-z_]*`, `put_object_acl`, `[a-z_]*_public_access_block` and
`object_lock_legal_hold_status` to the list, or turn it into an allowlist of the aws-sdk-s3 operations
the adapter may call. Update the help string and docs/ci.md.

### R4-B3 — Low
**The store's identity configuration is never deleted and has no path, owner or mode.**
- **A persistent secret.** It holds the owner's secret (design.md:227-228), and `start` now depends
  on it ("refusing when that configuration is missing").
- **Not named.** It has no path, owner or mode, and nothing says whether it lives inside the
  volume that `stop` unmounts.
- **Not deleted.** `down` deletes "its volume file and both credentials" but not this file
  (design.md:331-332). So the owner's secret outlives `down`, and a later `up` may refuse an
  existing configuration or overwrite it.
- **A contradiction.** "Only `up` writes that configuration" (design.md:228-229) contradicts
  `down`, which edits it to remove the uploader (design.md:323-325).

**Fix:**
- Name its path, owner (`flaminio-backup-store`) and mode 0600, written exclusive by `up`.
- Say that `down` rewrites it only to remove the uploader, and deletes it with the credentials.
- Script-test that no file holding either secret remains after `down`.

## Nits
- contracts.md:22: "Unknown profile refuses (exit 3)" now reads against list/verify/restore never refusing an unknown backend id; say "an unknown `immutable_profile_version`".
- contracts.md:219-220: "a top-level `immutable_guarantee` beside `retention_floor`": `repo init`/`repo check` have no floor and `list`'s is per archive; say "top-level" only.
- design.md:236-240: give the `socat` forwarder a `max-children` bound (it forks per connection, reachable from fl-a and fl-b).

Reviewed commit: afb3fc5f0fd0db9f7c9fdc2b828552eeb91840e0
