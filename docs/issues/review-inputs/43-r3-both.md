# Plan review: issue #43, round 3 (4328548..d04a54b), design-code + design-security

## Earlier findings
- R2-B1: fixed (StorageClass by plain create, open create; but see R3-B1 for step 8 on it).
- R2-B2: fixed (observedGeneration rule in S4, status, TestStatus, decisions).
- R2-C1: fixed. R2-C2: fixed in principle (step 8 keyed on live entries, TestAdoptionResumes); gaps in R3-B1, R3-C2.
- R2-C3: fixed (poll, undeploy-no-operator, cluster_state, docs). R2-C4: fixed (mutantSkipDriftCheck).
- R2-C5: fixed. R2-C6: fixed (the regex covers 1..65535 exactly; I checked each alternative). R2-C7: fixed except R3-C3.
- R2-S1: fixed (typed decoding, fixtures); see R3-C4.

## New findings

### R3-B1 — High
**Step 8 applies to "each live object", including the StorageClass, on which the operator has no patch right.**
- design.md step 8: "each live object that has a managed-fields entry of kubectl's client-side manager gets one patch". Step 3 skips a present StorageClass; the Contracts table gives `storageclasses` only get, list, watch, create.
- The adoption case is the main one: `make csi-deploy` created the StorageClass with `kubectl apply`, so it carries kubectl's entry. Step 8 sends a PATCH, which is Forbidden. `ApplyFailed` stops the pass, so later objects are not applied and the status stays Blocked forever. `TestAdoption`, run with RBAC off, would not see it; `TestRunsAsAccount` starts from an empty cluster.
- Fix: step 8 covers only the five objects the operator applies; say that the StorageClass keeps kubectl's entry. Add to `TestRunsAsAccount` (or a new test) an adoption run under the account, with a kubectl-made StorageClass and a kubectl-made workload.

### R3-C2 — Low
**Step 8 on a DaemonSet skipped for `UnsafeDrift` is unspecified and clashes with the test wording.**
- Step 8 runs "in every pass that finds one", so it sends a PATCH to a drifted data-plane DaemonSet that step 4 skipped. `TestUnsafeDrift` says "no PATCH of it is sent, counted", and `mutantSkipDriftCheck` is detected by such a PATCH.
- Also, step 8 runs after step 7, so a field the new build dropped is still owned by kubectl during that pass's apply; it goes in the next pass (the patch triggers one). Say so, or run step 8 before step 5, so `TestNewBuild` on an adopted object has one defined number of passes.
- Fix: state that the step-8 patch changes only `metadata.managedFields` and that the tests count PATCHes that change spec or scheduling; or skip step 8 on a drifted DaemonSet. State the pass order.

### R3-C3 — Low
**S15 reads as limits that RBAC cannot express; the table is right, the spec is not equal to it.**
- spec.md S15: "In `flaminio-operator`: its Lease, and that Lease's events"; the table grants `events` create and patch on every event of the namespace, and `leases` create on any. "Deployments and DaemonSets in `flaminio-system`: read, create, change" does not say that create covers any name and that change covers four.
- Fix: S15 says "create events in `flaminio-operator`; create a Lease there, get and update only its own", and the workloads' create is any name. `TestRulesExact` then has one meaning, and S16's reader sees the real reach.

### R3-C4 — Low
**The informers' scope is not stated, and the rights are namespaced.**
- The table gives list and watch on Deployments and DaemonSets by a Role in `flaminio-system`. A controller-runtime manager's default cache lists them cluster-wide and would be Forbidden. Design says only "watches ... (other names are dropped)", and "direct `get`" which the default client does not do (it reads the cache).
- Fix: say the cache is limited to `flaminio-system` for those kinds and the pass uses an uncached reader; `TestRunsAsAccount` already catches a mistake, say it is the test for it.
- Also: a render error after `Validate` (typed decoding of a value like `1:2` as an image) has no named outcome. Say it is `InvalidInput` and add one fixture.

## Checked, no finding
- The rights table against the verbs: create-by-apply and dry-run need create and patch, both granted; the Lease needs get, create, update; csaupgrade is a patch of the named objects. The status patch is on the subresource, named.
- The port pattern: alternatives cover 1 to 65535, no leading zero, no 0.
- The undeploy poll: terminating pods stay listed, a missing namespace lists nothing with status 0, so the wait holds and the no-operator case passes.
- `cluster_state` with a definition and no resource is `none`; the lab docs line names the way back.

Reviewed commit: d0c5d23b5f828b14e1ae7ccf8a6bc1ba2e26111c
