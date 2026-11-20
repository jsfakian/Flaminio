# Plan review: issue #43, round 2 (incremental, a0030ec..4328548), design-code + design-security

## Earlier findings
- R1-B1: fixed (forced server-side apply of the rendered fields; refusals only for a controller owner or an immutable field). See R2-C2 for a gap in the adoption step.
- R1-B2: fixed by removal (no teardown; S23 is the uninstall path). Its S23 order is checked below.
- R1-B3: fixed (no finalizer; deleting the definition deletes the resource and nothing else). See R2-C5 for the test that proves it.
- R1-B4: fixed in S15 and the table, except the StorageClass (R2-B1). The revisions right is rightly not granted: DaemonSet `updatedNumberScheduled` is computed from the controller-revision hash under `OnDelete` too. Decisions "does not apply as written" holds, if the status is read after the workload's `observedGeneration` (R2-B2).
- R1-B5: fixed, and decisions' reasoning holds: with no finalizer there is no terminating state, so "stop reconciling on deletion" no longer exists. A definition deleted while the operator runs ends at once.
- R1-B6: fixed (S3, S6, S10 each have a pass/fail rule and a test). See R2-C4 for S6.
- R1-B7: fixed (no marker). R1-B8: fixed by S23 (caveats in R2-C3).
- R1-S1, R1-C1, R1-C3, R1-C5, R1-C6, R1-C7, nits 1-5: fixed. R1-C2: fixed (`cluster_state`; see R2-C3 for one gap).
- R1-S2: fixed (`diskSerialPattern`, literal-only grammar, documented in S2).
- R1-S3: partly fixed: bounds and one character set exist; the port bound and YAML typing remain (R2-C6, R2-S1).
- R1-C4: partly fixed: two mutants exist, none for S6's exception (R2-C4).

## New findings

### R2-B1 — High
**The StorageClass cannot be created: S15 grants `create` but not `patch`, and the operator creates by apply.**
- design.md "Rendering and applying": every managed object goes through forced server-side apply; step 5 dry-runs, step 7 applies "each object left". design.md Contracts table: `storageclasses`: get, list, watch, create. spec.md S15 the same.
- decisions.md "Kubernetes behaviour": "Creating by apply needs `create` and `patch`". The plan's own premise contradicts its own rule for this kind.
- Effect: on a fresh cluster the dry-run of the missing StorageClass is Forbidden, so step 6 is `PreflightRefused` and nothing at all is written. S5 and S6 ("deleted: back") fail; `TestRunsAsAccount` would fail.
- Fix: either grant `patch` on `storageclasses` with `resourceNames: [flaminio]` (and say in S15 that it is for creating by apply; the code never patches it present), or create the StorageClass with a plain `create` and exclude it from the dry-run and the apply. Amend S15, the table, `TestRulesExact`, and add a `TestRepair` case "StorageClass deleted on an RBAC-enforced server".

### R2-B2 — Medium
**Status and the install poll read workload status that may predate the operator's own write.**
- design.md "The resource and validation": status comes from `readyReplicas`, `updatedReplicas`, `numberReady`, `updatedNumberScheduled`. Nothing checks the workload's `status.observedGeneration` against `metadata.generation`.
- Right after step 7 applies a new template, the Deployment or DaemonSet controller has not yet updated its status: counts still show ready and updated for the old template, so `Converged=True` can be reported, and `operator deploy` step 7 (design.md "operator deploy" 7) accepts it. The same hole hides "AwaitingNodeUpgrade" for a moment.
- Fix: S4 and the status rule: a workload counts only when its `observedGeneration` equals its generation, else `AwaitingRollout`. Add it to `TestStatus` (status set by the test with a stale `observedGeneration`), and list the behaviour in decisions.md.

### R2-C1 — Low
**"Converged" in the spec's Terms contradicts S4 and lab check 3.**
- spec.md Terms: "every managed object matches the resource, and every workload is ready". Contracts: `Converged` is `False/AwaitingNodeUpgrade` while ready data-plane pods are at an older revision; lab check 3 expects "the status is converged" only after `make node-upgrade`.
- Fix: Terms: "...and every workload's pods are ready and at the newest revision".

### R2-C2 — Medium
**The move of kubectl's ownership (step 8) is tied to "an object's first apply", which a crash or a restart skips.**
- design.md step 8: "after an object's first apply, one patch moves the fields of kubectl's client-side manager". It is a second request after the apply.
- A crash or leader loss between them leaves the object applied. The next pass finds dry-run equal to live (no write), never repeats step 8, and kubectl keeps the fields: a field a later build drops stays, which is the reason step 8 exists (decisions "kubectl's ownership is moved"). The same for an object a third party applies with `kubectl apply` again later.
- Fix: key step 8 on the live managed fields (a client-side manager entry exists), not on "first apply"; and make it part of "differs" in step 7. Test: fail the patch after the apply, restart, and see the entry gone and the dropped field removed (`TestAdoption` variant).

### R2-C3 — Low
**S23 and the lab guards: three gaps.**
- design.md `operator undeploy` step 2 deletes `flaminio-test` before the refusal ("before it changes anything else", spec S23 1). That is how `csi undeploy` works today (scripts/lab.sh:1066) and #12's design keeps it, so it is consistent, but the test `undeploy-attached` must assert that the fake `kubectl` log has no other delete (not the Deployment, not the resource). Name that assertion; no test states it now.
- Step 3 `kubectl wait --for=delete pod` is faked in the script test, and its behaviour with no matching pod (operator never installed) is not in decisions.md. Use `--ignore-not-found` on the Deployment and the namespace, and give that case a test (`undeploy-no-operator`).
- `cluster_state` (design.md "The lab commands") is "empty output: none" for two commands in sequence: say that an existing definition with no resource is `none`, and that after S11 or S23 a still-running operator is ignored (`csi deploy` proceeds on `none`). Docs (S22) should say that `make operator-deploy`, not `csi deploy`, is the way back.
- The order itself (refuse; operator Deployment and its pod gone; resource; workloads) is safe: no reconciler can recreate a workload while step 4 deletes them, and nothing is deleted before the refusal except the test namespace.

### R2-C4 — Low
**S6's exception, a safety rule, has no injected bug.**
- design.md Tests, invariant row: `mutantRecreate` and `mutantRollingUpdate` only. `TestUnsafeDrift` is the only guard of step 4 and it can pass with step 4 removed if the forced apply restores the same fields (the difference is only whether a pod is removed, which no test without a controller sees).
- Fix: `mutantSkipDriftCheck` (step 4 removed) must fail `TestNoDelete`/`TestUnsafeDrift` through "a PATCH of the data-plane DaemonSet whose live scheduling fields differ".

### R2-C5 — Low
**S12's "GC removes nothing" rests on the absence of owner references, and the tests cannot see it.**
- design.md "Removal (S12)"; `TestRemovalOrders` "the six objects keep their UIDs". The API-server tests have no garbage collector, so they pass even if an owner reference were set.
- Fix: assert in `TestRender` and in `TestRemovalOrders` that no applied object has `metadata.ownerReferences` (and no other object refers to the resource); name that in decisions.md as the replacement for a GC test.

### R2-C6 — Low
**The endpoint port bound has no form in the schema.**
- design.md Contracts: pattern `:[1-9][0-9]{0,4}`, "port up to 65535"; the schema's regex accepts 99999, while `Validate` refuses it. S3 says such a value is "refused when written", and `TestFixtures` requires that the schema and `Validate` agree.
- Fix: write the range into the regex, or add a CEL rule; name the fixture (`:70000`).

### R2-C7 — Low
**S17 and S15 are only part-tested.**
- S17: "read-only root filesystem, no capability, not root, on the witness". `TestPodAdmittedRestricted` proves what `restricted` demands, not `readOnlyRootFilesystem` nor the witness selector and toleration; `TestManifest` checks volume and host namespaces only.
- S15/Contracts table: "(the four names)" on get/list/watch/create of Deployments and DaemonSets reads as a limit; RBAC cannot limit those (the table's own sentence says only `patch` and the Lease carry `resourceNames`). Say so in the table, so that `TestRulesExact` has one meaning.
- Fix: extend `TestManifest` with the three fields and the placement; reword the table.

### R2-S1 — Low
**"One plain YAML scalar" ignores YAML typing, and the claim about #12's template is not checkable.**
- design.md "Why text is safe": every value is "one plain YAML scalar and one `sh` word wherever a template puts it, #12's fragments included". A value such as `true`, `null`, `123` or `1e3` is a valid `diskSerialPattern` and `trustDomain`, and where a template puts it alone (`value: {{X}}`, a list item) a decoder into unstructured objects makes it a bool or number. #12's template does not exist yet; its fragments use the values inside block scalars, which is safe.
- Fix: decode into typed objects (or set the fields as strings), and add the fixtures `true`, `123`, `null`, `1e3` to `TestRender`; state that the claim holds for the templates as written in #12's plan and is retested when #12 lands.

## Checked, no finding
- S15's list equals the table (except R2-B1). Rights for six objects, the status patch and the Lease are enough: create and patch with `resourceNames` on patch only; the Lease's `get`, `create`, `update`; events in the Lease's namespace. Pods, Secrets, ControllerRevisions, delete: none.
- `csaupgrade` exists in client-go; the `diskSerialPattern` grammar is literal in RE2 and unanchored (control/agent/disks.go:107); `:`, `-`, `_` are not shell-special.
- #12's plan: its `csi undeploy` order (test namespace, refusal, DaemonSet, labels) matches S23. No contradiction this text creates, except that design.md says `rbac.yaml` is "unchanged" while also listing #12's ServiceAccount as a possible addition to it; say which.

Reviewed commit: acc9341931e6bc49ebb6ff001dd2fc8ab7b5d3bc
