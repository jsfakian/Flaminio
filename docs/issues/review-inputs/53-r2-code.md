# #53 plan review, round 2 (incremental), design-code focus

Scope: `git diff 525f70c..0c4dfc0 -- docs/issues/53/ ':!docs/issues/53/report.md'`, checked
against #52's clear plan (flaminio-52 docs/issues/52), #16's design and origin/main's
`scripts/lab.sh`, `scripts/guest/{pki,build}.sh` and Makefile.

## Earlier findings
- R1-C1: fixed (S7 narrowed to plain archives; graph, records, RPCs and tests deferred with #52's S8).
- R1-C2: fixed (`cover` reads first; a refused extension is covered only if the read-back shows E; tests added).
- R1-C3: fixed (one root per operation under #52's `backup_uuid`; a late root ends the operation). See R2-C1 for a gap the fix opens.
- R1-C4: fixed (I = store Date + `min_period_seconds`; descriptor locked at init; test).
- R1-C5: fixed (`list` root-only, 2 requests a root, `graph_checked: false`; `verify`/`restore` the graph).
- R1-C6: fixed (`CanaryProbe`, named canaries, an 11-row table with safe forms, owner renewal). See R2-C2.
- R1-C7: fixed (owner evidence file defined; digest recorded, not verified; change exits 3; uncertainty flag).
- R1-C8: mostly fixed (second SeaweedFS, commands, buckets, credentials, periods, steps, `down` gate). See R2-C2 to R2-C5.
- R1-C9: fixed (tests named per rule; the missing rows added).
- R1-C10: fixed (follow-up in the docs list; intent's line names #73).
- R1-C11: fixed (pending #58's review; `schema: 2`; hand-off in the round's report).
- R1-C12: fixed (P, D, E checkpointed; another P exits 2).

### R2-C1 — Medium
**A run whose cover overruns D still PUTs a root it knows is late, which ends the operation and burns its grant.**
Only a rerun re-picks D: contracts "One root per operation" ("Before the root's PUT ..., a rerun
whose store time plus uncertainty is past D picks a new D and E") and design.md:87-88. Within the
first run, nothing checks the latest store Date against D before the root's PUT. A late root then
ends the operation: the worker exits 1, the administrator must abort the grant, and the next
attempt needs a new grant and operation, which re-reads the content it did not record
(design.md:85-88, contracts.md:61-66). The case is realistic. D is chosen only after all data
arrive (design.md:74). Every block uploaded in the run carries only I = S + min < E (contracts
"Initial lock"), and every reused block carries the base's earlier floor. So `cover` sends a read,
an extension and a read-back for nearly every version of the graph, all of them after D is chosen.
On a large volume that takes longer than a 5-minute commit budget.
**Fix:** before the root's PUT, every run checks that the latest store Date plus uncertainty is
at most D. If it is past D, the run picks a new D and E, checkpoints them and covers the graph
again, once. If D is still past after that, the run exits 1 with the grant active and the
checkpoint kept, and sends no root PUT. Say that the commit budget must be sized to the graph's
cover time. Add the test `retention.rs`: `known_late_root_not_put`.

### R2-C2 — Medium
**The lab's locked canary fails probe row 4, so the lab's step 3 cannot pass.**
Contracts, Probes, row 4 requires `canary/locked` in COMPLIANCE with R >= store time + 1 h, and a
stale canary is uncertain: exit 1, nothing published. The lab's owner step (design.md:223) writes
`canary/locked` "until the check's start plus 40 min". So the probes of step 3's `repo init
--immutable`, and of each later backup, all fail.
**Fix:** make the two agree. Either the lab writes R as the check's start plus at least 1 h plus
the check's duration (for example 2 h), with `cleanup-after` and `down` waiting for it, or row 4's
margin becomes a profile value that the lab sets below 40 min.

### R2-C3 — Medium
**The lock store cannot be brought back after fl-w restarts, so the later-session `down` the plan relies on may be impossible.**
`scripts/guest/backup-lock-store.sh` has only `up|down` (design.md:201). `down` "refuses unless
the store is up" (design.md:250). `up` generates both credentials into files "created exclusive"
(design.md:205-207), so a second `up` fails on the existing files. #52's store, which this copies,
has `stop|start` for that reason (52/design.md:334). The lab has `make vm-stop` and `make
vm-restart` (`Makefile:164-171`, VM=w allowed). After a reboot between the check and the end of
the retention, nothing starts the store, and `down` refuses forever. Separately, design.md:253's
"so there is no early wipe" is not true of the lab: `make vm-destroy VM=w CONFIRM=yes` deletes
fl-w's root disk, and the store's volume with it.
**Fix:** add `start|stop` as #52's `backup-store.sh` has them. `start` remounts the volume and
restarts the store with the existing credentials, and `down` names `start` when the store is
down. Add script-test cases for both. Reword the claim to "the store's own commands have no early
wipe; destroying fl-w would wipe it", and state that in docs/lab.md.

### R2-C4 — Low
**The `backup-owner` pair is specified less fully than #52 specified its two names under #16's `pki.sh` rules.**
Design.md:212-218 gives the pair's directory and owner (`/etc/flaminio/backup-owner-pki`,
`flaminio-backup-store` 0700) and says only "as #52 adds its two". #16's `pki.sh` refuses any name
not in its list, and each name needs its directory, its first key's owner and mode, and its share
of the temporary-directory sweep (16/design.md:120-132, 186-196). For its two names, #52 spells out
(52/design.md:353-374, 410-420):
- the first key's owner and mode (`... 0600`);
- `file ca.crt` into the directory, only if it exists;
- the `.new` sweep pattern;
- the `identities w` line, or its absence, for renewal;
- script-test cases.

#53 gives none of these for `backup-owner`. Also, the lock store serves #52's `backup-store` pair,
and #52's renewal restarts only #52's store after renewing that pair (52/design.md:353-359). The
lock store would keep the old certificate until it expires.
**Fix:** list the five items for `backup-owner`: the first key `flaminio-backup-store` 0600, `file
ca.crt` into `backup-owner-pki`, `backup-owner-pki.[0-9a-z]*.new` in the sweep, and either an
`identities w` line `backup-owner CA ID --client` or "not renewed, the lab check being shorter
than its life". Extend `backup-lock-scripts-test.sh` to cover them. Make the renewal's restart
step also restart the lock store (`backup-lock-store.sh stop`, then `start`; see R2-C3).

### R2-C5 — Low
**The lab steps leave ports, the example's build and install, and file paths unnamed.**
- **Ports:** a second SeaweedFS on the same VM as #52's needs its own master, volume and filer
  ports and their gRPC ports, not only the S3 port. Design.md:204 names one
  `BACKUP_LOCK_STORE_PORT`, and #52 sets none, so both instances take the defaults and collide.
- **The example:** `lab.sh deploy` runs `guest/build.sh`, which is `cargo build --release`
  (`scripts/guest/build.sh:3-6`, `scripts/lab.sh:219-231`). That builds no examples, so
  `lock_lab` is never built on fl-w. The plan also does not say where it is installed so that the
  `flaminio-backup-store` and `flaminio-backup` users can run it.
- **Files:** the lock repository's worker config, its repository key file, and the evidence file's
  install destination are unnamed. #52's `repo init` refuses an existing key file
  (52/design.md:315), so they need paths separate from #52's.

**Fix:**
- Ports: name every listener in `lab.conf`, and bind the internal listeners to loopback, which
  S1's interface check also wants.
- The example: add a build step (`build.sh -p flaminio-backup --example lock_lab`) and `install
  -m 755` to `/usr/local/bin/flaminio-lock-lab` in `backup-lock-store up`.
- Files: name them, for example `/var/lib/flaminio-backup/lock.{conf,key,evidence.json}`.

### R2-C6 — Low
**"No request sent" for `Covered` contradicts the read that `cover` always sends.**
Contracts' `cover` reads the exact version's retention first, then says that COMPLIANCE >= E "is
`Covered`, with no request sent". Design.md:112-113 says such a version "needs no request". The
test `reused_version_already_covered` says "(no request sent)". But that read is a
GetObjectRetention, counted in `retention_reads`. A test asserting zero requests fails against the
design, and the lab's "lock requests per kept block" (design.md:242-243) becomes ambiguous.
**Fix:** say "no extension request (one retention read)". The test asserts exactly one
GetObjectRetention and no PutObjectRetention. State the per-version cost: 1 read, or 3 when
extended.

### R2-C7 — Low
**`object_store_has_no_destructive_call` cannot be the compile-time check it claims.**
Design.md:171 calls it "a compile-time check that only `CanaryProbe` builds a delete, bypass or
bucket request". The production `ObjectStore` adapter holds an aws-sdk-s3 `Client` (#52 contracts,
"Object client"), and that client exposes `delete_object` and the bucket calls. No compile-time
check stops the adapter from calling them, and the trait having no such method does not prove that
it doesn't.
**Fix:** either wrap the SDK client in a module-private type that only `probe.rs` can widen,
asserted by a `trybuild` compile-fail case, or make the test runtime: the serverless fake records
every request an `ObjectStore` run sends, and the test asserts that none is a delete, a bypass or a
bucket mutation.

## Nits
- contracts.md:69-72: `list`'s request bound leaves out the GET of the descriptor authority's version.
- contracts.md:129: row 7 does not name its retention mode (COMPLIANCE, presumably).

Reviewed commit: 36b64f06e32886259e255b96f7136795eb390230
