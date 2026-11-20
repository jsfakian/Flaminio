# #53 plan review, round 3 (incremental), design-code and design-security

Scope: `git diff 36b64f0..d3f8efb -- docs/issues/53/ ':!docs/issues/53/report.md'`. Checked against
#52's clear plan (flaminio-52 docs/issues/52 at ae97cf5), #16's design on origin/main,
origin/main's `scripts/guest/build.sh`, `scripts/lab.sh` (`issue` flags) and `scripts/lab.conf`.

## Earlier findings
- R2-C1: fixed (every run re-checks Date+u<=D before the root's PUT, re-covers once, never PUTs a known-late root). Gaps in R3-C1.
- R2-C2: fixed (canary R = start + 2 h; probes end in the first hour; `cleanup-after` includes R).
- R2-C3: fixed (`stop|start`, `down` names `start`, the wipe claim reworded). See R3-B3.
- R2-C4: fixed (directory, first key, `file ca.crt`, sweep, `identities w` line, renewal restart, tests); consistent with #16 and #52.
- R2-C5: fixed for ports, the example (build.sh passes `$@` to cargo) and the files. See R3-B2 and R3-C2.
- R2-C6: fixed.
- R2-C7: not fixed in effect. The runtime test cannot fail (R3-C3).
- R2-S1: fixed (candidate state, `--allow-unqualified-profile`, `qualified: false`). See R3-S1.
- R2-S2: fixed (rows 1 and 4 read the hold, "expired" dropped, row 12 added, tests). See R3-B1.
- R2-S3: fixed.
- R2-S4: fixed.
- R2-S5: fixed.
- R2-S6: fixed in intent. See R3-B3.
- R2-S7: fixed in intent. See R3-B2.
- R2-S8: fixed.
- Nits (code: `list`'s bound, row 7's mode; security: the lab example): fixed.

### R3-B1 — Medium
**Rows 1 and 4 cannot pass: "no retention" and "hold OFF" come back as error codes, which the classification calls uncertain.**
- **The codes.** On S3, GetObjectRetention on a version with no retention answers an error
  (`NoSuchObjectLockConfiguration`, a 404), not a body. GetObjectLegalHold on a version never held
  may also answer an error rather than `Status: OFF`. Contracts, Probes, then says "NotFound,
  NotImplemented and any other code" are uncertain, exit 1. So row 1 ("no retention, hold OFF") and
  row 4's "hold OFF" never pass, and lab step 3 exits 1.
- **The table.** The profile's codes cover only rows 5 to 7 and row 11 (contracts, Backend
  profiles). An implementer who takes "any 404" as "absent" also accepts `NoSuchKey` and
  `NoSuchVersion`, where the canary itself is gone.

**Fix:** a profile's codes also name the exact absence answers for rows 1 and 4 (the code and
status for no retention, and for no hold or hold OFF). Any other 404 (`NoSuchKey`,
`NoSuchVersion`) stays uncertain. Add both cases to `unlocked_canary_locked_is_uncertain` and
`locked_canary_stale_fails`, and list the absence answers among what the lab's run checks for the
candidate.

### R3-S1 — Medium
**"Never reports an immutable guarantee" under a candidate profile is neither defined nor tested.**
- **Not defined.** The output keeps `immutable_profile`, `retention_floor`, `candidate_state`,
  `uploader_check` and exit 0. The descriptor still says `immutable: true` (contracts.md:17). No
  text says which of these change, or are left out, under a candidate (contracts.md:204-206,
  spec.md:46-48).
- **Not tested.** `candidate_profile_needs_flag` (design.md:184) asserts only `qualified: false`
  and `source`. A consumer that reads `candidate_state` or `retention_floor`, but not
  `backend_profile`, sees a guarantee.
- **Open case.** For `list`, `verify` and `restore`, which take no flag, the plan does not say
  what happens to a repository whose profile id is unknown to this worker build. The sentence
  "an unknown id refuses" is scoped to the three setup commands.

**Fix:**
- Define what a candidate output must do. For example: `candidate_state` reads
  `completed-unqualified`, never `completed`, and a top-level `immutable_guarantee: false` sits
  beside `retention_floor`.
- Say that `list`, `verify` and `restore` report `qualified: false` for an unknown id and do not
  refuse, as S8 requires.
- Extend the test to assert those fields for every command.

### R3-C1 — Low
**The deadline re-check's retry is deterministic and has no remedy, and it leaves no margin for the root's own PUT.**
- **The retry.** After a new D' and E' > E, every version must be extended again, at 3 requests
  each (contracts.md:101-106). So the second cover costs as much as the first. A graph whose cover
  overran the budget once overruns it again, and so does every rerun.
- **The remedy.** The commit budget is fixed in the descriptor at `repo init` (contracts.md:17-18),
  and `backup` takes only `--retain-for`. The decisions' risk ("which a rerun retries",
  decisions.md:258) and design.md:104 ("retry with explicit bounds") point to a remedy that does
  not exist. The real one is a new repository with a larger budget.
- **No margin.** A check that passes at Date+u = D leaves zero time for the PUT. The root is then
  late and burns its grant, which the plan calls "a PUT slower than the last check foresaw". The
  check foresees nothing.

**Fix:** word the risk as "fails every run until the repository is recreated with a larger budget
(or the graph shrinks)", and drop "retry with explicit bounds". Require
`Date + u + put_allowance <= D`, with `put_allowance` a stated constant (for example 2*u, or the
largest root-PUT time the profile measured). Test the boundary in `deadline_edges`.

### R3-B2 — Low
**The lock store's listener layout may not be buildable as written, and the refusal check cannot tell which listeners are the store's.**
- **One bind address.** In `weed server`, a single `-ip.bind` is shared by master, volume, filer
  and S3. S3 on fl-w's address with the other six on 127.0.0.1 (design.md:227-235) then needs
  separate `weed master|volume|filer|s3` processes, each with its own bind and the filer's peers on
  loopback. That changes `up`, `stop`, `start` and the fakes. The plan defers only "the bind flags".
- **An S3 gRPC port.** Recent releases also open an S3 gRPC port (`-s3.port.grpc`, default S3 port +
  10000, so 18334) on S3's bind address. It is not among the named listeners.
- **The check.** `ss -Hltn` prints no process or uid. So "no listener of the store but S3" can
  only mean the configured ports, and a listener on an unnamed port (as above) passes. With
  process attribution, it would refuse every `up`.

**Fix:** state the process layout (separate processes, or `weed server` with S3 on loopback behind
a named forwarder), and name the S3 gRPC port in `lab.conf` on 127.0.0.1. Make the check `sudo ss
-Hltnp`, filtered by the store's PIDs (or `-e` by its uid), refusing any listener of those
processes off loopback other than the S3 one. Script-test a stray listener on an unnamed port.

### R3-B3 — Low
**`down` removes the uploader, then restarts with `start`, whose "existing credentials" can bring it back.**
- **The conflict.** `start` "starts the store again with the existing credentials, refusing when
  one is missing" (design.md:224-226). `down` removes the uploader's identity from the
  configuration and then runs `stop` and `start` (design.md:300-304), and deletes the credential
  files only at the end. If `start` builds the identity configuration from the two credential
  files, as `up` does, it re-adds the uploader. If `down` deleted the uploader's file first,
  `start` would refuse.
- **The same gap elsewhere.** A refused `down` "leaves the store serving the owner alone". A later
  `start` after a reboot, or #52's renewal restart (stop, start), has the same ambiguity.

**Fix:** say that `start` starts with the identity configuration as it stands and never
regenerates it, and that only `up` writes it. Alternatively, `down` writes a marker that
`start` honours. Script-test: after `down` removes the uploader, a `stop` and `start` leaves it
removed.

### R3-C2 — Low
**Two lab-check steps cannot run as written.**
- **The flag.** design.md:279 passes `--allow-unqualified-profile` to "every worker command it
  runs". `list`, `verify` and `restore` do not take it (contracts.md:219-221; design.md:151), so
  #52's CLI parser refuses the unknown flag (exit 2) at steps 4 and 5.
- **Step 7.** Step 7 runs `repo init --immutable` on `flaminio-nolock` and expects "its JSON naming
  the bucket's missing lock" (design.md:292-294). The only named files are `lock.conf`, `lock.key`
  and `lock.evidence.json` for `flaminio-lock` (design.md:259-262). `lock.key` exists after step 3,
  and `repo init` refuses an existing key file. The evidence names `flaminio-lock`, and the
  bucket-match check comes before the settings read-back (contracts.md:186-188). So step 7 fails
  on the key file or the evidence, not on the missing lock.

**Fix:** pass the flag only to `repo init`, `repo check` and `backup`, and assert `qualified:
false` in every recorded output. Name step 7's own config, key file and evidence (for example
`nolock.{conf,key,evidence.json}`), written by the owner steps for `flaminio-nolock`.

### R3-C3 — Low
**`object_store_has_no_destructive_call` cannot fail.**
#52's serverless fake implements the `ObjectStore` trait (52/contracts.md:418-419; 52/design.md:28).
That trait has no delete, bypass, legal-hold or bucket method (contracts.md:113). So a fake that
records "every request the library's commands send through `ObjectStore`" (design.md:184) never
sees one, with or without the rule. The production aws-sdk-s3 adapter, the code R2-C7 was about,
is never exercised.

**Fix:** either run the production adapter against a local HTTP S3 fake and assert the wire
operations, or add a `make hygiene` check that the adapter's module calls none of
`delete_object`, `delete_objects`, `put_object_legal_hold`, `put_bucket_*`,
`put_object_lock_configuration` or `bypass_governance_retention`, `probe.rs` and `examples/`
excepted.

## Nits
- design.md:184 `multi_delete_allowed_refuses_setup`: add a response naming U in neither Deleted nor Errors (uncertain, exit 1).
- design.md:244-246: say that `backup-lock-store up` puts `ca.crt` after issuing `backup-owner`, as #52's `up` does for `backup-store`.

Reviewed commit: 5b472370eeb90b7345b3ce6ecc8bdfdb79cd99b8
