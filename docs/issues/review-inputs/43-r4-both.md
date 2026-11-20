# Plan review: issue #43, round 4 (d04a54b..ff76f39), design-code + design-security

## Round-3 findings
- R3-B1 (High): fixed. Step 8 is gone; the StorageClass is only read and created; TestRunsAsAccount now has a second run from kubectl-made objects (StorageClass included) with no refused request; TestAdoption checks the StorageClass gets only a get.
- R3-C2 (Low): fixed by the cut. TestUnsafeDrift's "no PATCH of it" now holds: step 4 skips before the dry run in step 5. No pass-order ambiguity left in TestNewBuild.
- R3-C3 (Low): fixed. S15's lines map one to one onto the table (read by name incl. list/watch of the resource; change by name = status, CSIDriver, four workloads; create any name; own Lease get/update; events create/patch).
- R3-C4 (Low): fixed in principle (one cluster-scoped watch; reads by name; TestRunsAsAccount named as the test; InvalidInput + TestRenderErrorIsInvalidInput + fixture `1:2`). One gap: R4-C2.

## New findings

### R4-C1 - Low
**Retry backoff can break S6's "within a minute", and "30 s after it ends" is not stated for every outcome.**
- design.md step 7: ApplyFailed "is retried with backoff". With controller-runtime a returned error uses the default rate limiter (cap about 1000 s). S6 says a deleted or edited object is back within a minute; lab check 2 and TestRepair/TestStaleWriteConverges assert 60 s. A transient failure (or a blocked state) can push the next pass past that.
- Fix: say every outcome (ok, InvalidInput, PreflightRefused, UnsafeDrift, ApplyFailed) requeues after at most 30 s (cap the backoff at 30 s, or requeue instead of returning the error). Make the period injectable so TestRepair does not need 60 real seconds.

### R4-C2 - Low
**"Uncached get" is stated but not how, and the default client reads from the cache.**
- design.md "A pass": "uncached `get`"; Triggers: "the manager's cache holds nothing else". controller-runtime's default client serves Get from an informer, which would start a list/watch of Deployments etc. and be refused. The design names no `APIReader` or `Cache.DisableFor`/`Cache.Unstructured` setting.
- TestRunsAsAccount would catch a mistake, but only as a failure, not as a design. Fix: name the mechanism (the manager's `GetAPIReader()` for reads, or a direct client for the managed kinds; the cache scoped to `FlaminioCluster`).

### R4-C3 - Low
**Status is up to 30 s stale and some checks read it once.**
- Workloads are no longer watched, so `Converged` after `make node-upgrade` (lab check 3) or after a rollout changes only at the next pass. `operator deploy` polls 300 s (fine); lab check 3 and any `operator-test.sh`/lab check that reads the status once can see a stale `AwaitingNodeUpgrade`.
- Fix: say in S4 or the lab checks that status lags by at most one period and that checks poll.

### R4-C4 - Low
**The only way to remove a dropped field (S9's limit) is the uninstall, which S23 refuses while a volume is attached.**
- design.md "The limit (S9)": `make operator-undeploy` then `make operator-deploy`; S23 step 1 refuses while attached, and step 3 deletes the data-plane DaemonSet. So a build that drops a field needs the cluster detached and the data plane down. A Proposed choice with its consequence stated, so not a finding by itself, but the docs sentence should say "detached, with the data plane removed", and the build that first drops a field should be gated on that (e.g. a TestRender check listing fields dropped against the previous template, which fails unless the follow-up exists).

## Checked, no finding
- Nothing still relies on the cut step: grep of spec, design, adr-proposal, decisions for csaupgrade, step 8, managed-field moves, TestAdoptionResumes finds only the "cut" notes (decisions.md, report.md). `TestNewBuild` is now scoped to objects the operator made; `TestAdoption` no longer asserts kubectl's entry is gone. S9 states the limit; S22 and followups.md carry it.
- No remaining `list`/`watch` of a managed kind: the table grants only get (named) plus create/patch. Apply-creating an absent object needs create (open) and patch (named, URL carries the name): OK for the CSIDriver and the four workloads. The dry-run apply of an absent StorageClass is a create dry run: covered by create.
- Leader election with the table: Get (named), Create (open), Update (named) on the Lease; events create/patch open in `flaminio-operator`; status patch is on `flaminioclusters/status` named; the resource has list/watch open (the informer) and get named. The CEL name rule matches the named rights.
- Forced SSA on an adopted object: a differing value is taken by force; a same-value field is co-owned; a later change forces again; a dropped field stays only while kubectl co-owns it: as stated. S8's second pass: the managed-fields times are ignored in the comparison.
- A new leader gets its first pass from the informer's initial event; the status patch's own watch event causes one extra no-write pass.
- Nit: `events` in the table has no API group; say `""` (core), as the election's recorder uses it.
