# Issue #43: The operator — design

First installer/reconciler step only: [intent](intent.md), [spec](spec.md) S1–S23, Proposed
choices in [decisions](decisions.md) and the [ADR proposal](adr-proposal.md).
**Approved 2026-11-13; not implemented.** The full issue stays open.
Settled at approval, 2026-11-13: the Proposed choices and the ADR proposal, each as stated (see decisions.md, Settled at approval).

The facts it starts from:
- `cmd_csi deploy` (`scripts/lab.sh:991–1047`) applies `rbac.yaml` (`:1002`), makes the Secret
  when missing (`:1003–1021`), builds a dated CSI tag (`:1025`) and applies `driver.yaml`
  (`:1042–1043`) with a client-side `kubectl apply` (`:902–908`): kubectl's manager owns every
  field of the live objects. `render` substitutes `{{NAME}}` in text (`scripts/lib.sh:199–208`).
- `csi undeploy` (`:1065–1071`) deletes the test namespace and what `driver.yaml` made. The
  namespace `flaminio-system` (privileged, `deploy/csi/rbac.yaml:11`), the accounts and the
  Secret (named literally in `deploy/csi/driver.yaml:85` and `:220`) stay.
- `flaminio-agent claim --serial` takes a regular expression
  (`control/cmd/flaminio-agent/main.go:34`, `:41–46`), matched unanchored
  (`control/agent/disks.go:107`); the lab passes `data` (`scripts/guest/agent.sh:9`).
- `control/failover/kube.go:21–56` is a nodes-only HTTP client; `go.mod` has no Kubernetes
  framework. No operator package or resource type exists.
- #12 is not in this checkout. Its plan supplies `deploy/dataplane/dataplane.yaml` (`{{IMAGE}}`,
  `{{META_ENDPOINTS}}`, `{{TRUST_DOMAIN}}`, `{{SERIAL}}`, unquoted in `sh -c` fragments), the
  `OnDelete` DaemonSet, `make node-upgrade`, and the steps and the refusal of `cmd_csi`. It
  gives those values no grammar, and no file for the ServiceAccount `flaminio-dataplane`.
- The issue's audit follow-up (etcd members, progress under API outages) and #82 are later steps.

## The resource and validation (S1–S4)

- **The definition:** `flaminioclusters.storage.flaminio.io`, kind `FlaminioCluster`, Cluster
  scope, `v1alpha1` served and stored, a status subresource, a structural schema, no webhook,
  no finalizer. A CEL rule: `self.metadata.name == 'flaminio'`. Go types in
  `api/kube/storage/v1alpha1`; the schema and the deep copies are written by hand.
- **The fields** (Contracts) are all required, with no default. The Secret's name and the
  nodes' identity files are not fields. No replicas, selectors, flags, pod overrides or keys.
- **Two checks, one set of fixtures:** the schema, and `Validate(Inputs)` in Go before every
  render. Validation reads nothing: no Secret, no image, no metadata call.
- **Status** (Contracts) comes from the workloads' own status: a Deployment's `spec.replicas`,
  `readyReplicas`, `updatedReplicas`; a DaemonSet's `desiredNumberScheduled`, `numberReady`,
  `updatedNumberScheduled`. A workload whose `status.observedGeneration` is not its
  `metadata.generation` is `AwaitingRollout`, whatever its counts: they describe an older
  template. No pod or ControllerRevision is read: data-plane pods behind are counted, not
  named. When it is written is step 9 of the pass. No heartbeat.

## Rendering and applying (S3, S5–S9)

- **One source:** `deploy/templates.go` embeds the template files and holds no logic; the lab
  keeps `render`. `control/operator/render.go` substitutes in the text as `render` does,
  decodes into the kinds' typed objects, and returns the six managed objects: #12's DaemonSet,
  then `driver.yaml`'s in file order. A document that is not one of the six is an error.
- **Why text is safe:** every accepted value starts with a letter or digit and stays within
  `[A-Za-z0-9._/:@,-]`: no space, quote, `$`, glob, bracket, brace or `#`. It is one `sh`
  word, and cannot end a YAML string or block that holds it. This grammar is the only guard.
  IPv6 literals are not accepted in this step.
- **A value alone in a scalar** could be read as a bool or a number (`true`, `123`). Only an
  image stands alone (`driver.yaml:44`, `:107`, `:203`), and its pattern requires a `:` or an
  `@`. #12's plan puts its values inside a script block and one command string. The typed
  decoding turns any other case into an error, and `TestRender` runs on the real files.
- **Applying:** server-side apply, forced, field manager `flaminio-operator`, of exactly the
  rendered fields. The operator sets no label, annotation, owner reference or finalizer.
  Fields it does not declare stay with their managers; a declared one is taken back.

**A pass.** This list is the whole flow; the rest of the plan defers to it. Every request
goes through the reconciler's client. "An error" is any failure not named, a timeout included.
A pass starts when step 10's time comes, or on a watch event of the resource: its creation,
its deletion, or an update that changes its generation. The operator's own status patch
changes none, so it starts no pass.
1. **Read the resource.** Not found: the pass ends here, with no other request and no next
   pass; the watch starts one when a resource exists. An error: go to step 10.
2. **`Validate`, then render.** No request. A failure of either: `InvalidInput`, go to step 9.
3. **Read the six live objects,** by name. Not found means absent. An error: `ApplyFailed`,
   naming the object, go to step 9. `ApplyFailed` is the reason of any failed request about a
   managed object.
4. **Adoption check:** a live object, the StorageClass apart (step 5), with an owner
   reference that has `controller: true` is a refusal: that object gets no dry run, and the
   others go on.
5. **Skips.** The StorageClass, when present: never compared, changed or deleted. The
   data-plane DaemonSet whose live `nodeSelector`, `affinity` or `tolerations` differ from
   the rendered ones: `UnsafeDrift`, naming the fields. The other objects go on.
6. **Dry run** of each object left: a plain create for an absent StorageClass, the forced
   apply for the others. An answer that the object is not accepted (invalid, an immutable
   field such as a selector, forbidden) is a refusal, and the dry runs go on. An error:
   `ApplyFailed`, go to step 9.
7. **Any refusal in 4 or 6:** `PreflightRefused`, naming each refused object, go to step 9.
   No managed object is written.
8. **Write** each object whose dry-run result differs from the live one (`resourceVersion`
   and the managed fields' times apart), in order. An error: `ApplyFailed`; the writes stop
   and what was applied stays.
9. **Status.** `Blocked` takes the first of these that the pass found: `InvalidInput`,
   `ApplyFailed`, `PreflightRefused`, `UnsafeDrift`. Its message lists every object the pass
   found refused, failed or drifted (or `spec`, for `InvalidInput`), whatever the reason;
   `Converged` follows its own order (Contracts). Each workload counts as this pass last got
   it (step 3's read, or step 8's answer); one that was not read keeps the counts read in
   step 1, or none on a first pass. Patch the status only when it differs from the one read
   in step 1, so passes cannot loop on it. An error changes nothing: the next pass computes
   it again.
10. **Next pass:** ask for one `Options.Period` (30 s) from now. No error goes back to the
    framework, so its backoff never applies. A pass has no deadline of its own.

- **Adopted objects keep kubectl's managed-fields entry:** the operator never rewrites managed
  fields. The first pass applies each adopted object once, which makes the operator an owner
  of every field it renders, beside kubectl; a value that differs is taken by force. The
  StorageClass is never changed or deleted.
- **The limit (S9):** a field that kubectl set and that a later build no longer renders (a
  container, a volume, an environment variable) stays on an adopted object, as kubectl still
  owns it. This step's templates are the script's, so nothing is dropped now. A later build
  that drops one says so in `docs/lab.md`: `make operator-undeploy`, then `make
  operator-deploy`, makes the objects anew, owned by the operator alone. That needs every
  volume detached, and removes the data plane until the second command (S23).
- **The reconciler's client:** `operator.New(config, Options)` builds everything, for `main`
  and for the tests alike. It must give the reconciler one client for all its reads and
  writes, made by `client.New` from a copy of the configuration whose `Timeout` is
  `Options.RequestTimeout` (10 s): never the manager's `GetClient()`, which reads a cache.
  The manager keeps the configuration unchanged, so no timeout cuts its one watch, on the
  resource, or its election. `TestRunsAsAccount` runs `operator.New`: a cached read lists and
  watches, which is refused there.
- **Every time bound comes from step 10, in passes:** a change is seen by the first pass that
  starts after it, so it is repaired, and in the status, once two more passes have ended (the
  one that was running, and the next). The reconciler counts its passes, and the tests wait
  on that count, with a short period and timeout.
- **Only the lab needs seconds.** It polls for 120 s: two passes taken as 15 s each at most,
  the 30 s between them, and one change of leader (17 s: the Lease's duration and one retry)
  make 77 s. The other 43 s are the margin, which also holds the pod's restart when the
  leader changes. A slower pass or restart, or a second change, can fail a check, which then
  says so.
- **A new build (S7)** is the same pass with other templates. `TestRender` pins the data
  plane's selector, update strategy and scheduling fields to #12's: a build that changes them
  fails it, and on a live cluster stops at step 5.

## Leadership, deletion and independence (S10–S13)

- **The Lease:** a controller-runtime manager, Lease `flaminio-operator` in
  `flaminio-operator`, 15 s duration, 10 s renew deadline, 2 s retry. It reconciles only as
  leader; on loss it cancels its workers and exits. A Lease is not a fence: a request sent
  before the loss may land, and the leader's next pass puts the object back.
- **Deleting the resource:** with no finalizer it is gone at once, and the pass ends at its
  step 1. The code has no delete call, and the account no delete right.
- **Removal (S12):** the operator sets no owner reference and makes no object but the six.
  The tests assert exactly that; garbage collection itself runs only in the lab (check 6).
  Deleting the definition deletes the resource, which is S11. A new resource finds the objects
  by name: the same pass.
- **Independence:** no metadata client, socket or device (`TestNoStorageImports`).

## The install manifest and rights (S14–S18)

- **`deploy/csi/rbac.yaml`,** applied first, as #12 leaves it: this change does not edit it.
  #12's ServiceAccount `flaminio-dataplane` must be in it: the renderer refuses it in a
  template (Risks).
- **`deploy/operator/operator.yaml`** (`{{OPERATOR_IMAGE}}`): the definition; the Namespace
  `flaminio-operator` (`enforce: restricted`); the ServiceAccount; Contracts' rules, as a
  ClusterRole, a Role in each namespace, and their bindings; the Deployment. Deleting this
  file removes no workload (S12).
- **The Deployment `flaminio-operator`:** one replica; `app: flaminio-operator`; the witness's
  selector and toleration, as the CSI controller's; `runAsNonRoot: true`, UID and GID 65532,
  `seccompProfile: RuntimeDefault`; `allowPrivilegeEscalation: false`, `capabilities.drop:
  [ALL]`, `readOnlyRootFilesystem: true`; `imagePullPolicy: Never`; no volume, host namespace
  or port. Command: `flaminio-operator run`.
- **No Event** is written about the resource or a workload: everything is in the status. The
  only events are the election's, about the Lease, in `flaminio-operator`.
- **S16's statement** goes to `docs/lab.md` and the ADR, in the spec's words.

## The lab commands (S19, S20, S23)

- **`cluster_state`** prints `none`, `present` or `unknown`: `kubectl_lab get crd
  flaminioclusters.storage.flaminio.io --ignore-not-found -o name`, then `kubectl_lab get
  flaminioclusters.storage.flaminio.io -o name`. A failure is `unknown`; empty output, `none`.
  A definition with no resource is `none`: an operator left running then has nothing to do.
- **`csi deploy` and `csi undeploy`** call it before they change anything (after `:995`,
  before `:1066`). `present` and `unknown` die; `none` goes on, the Mac demo included.

**`operator deploy`:**
1. `demo_active` or `unknown`: die.
2. The agents' identities (`:998–1001`), and #12's steps 1 to 4: claim, unit, image, labels.
3. `rbac.yaml` (`:1002`), then the Secret when missing (`:1003–1021`).
4. The CSI image (`:1025–1041`). The operator's (`scripts/dev/operator-image.sh`, a dated
   tag), imported on `$VM_W` only.
5. `operator.yaml`; `kubectl wait --for=condition=Established` on the definition (60 s);
   `rollout status` of the operator (180 s).
6. `deploy/operator/cluster.yaml`: created, or updated on a second run.
7. Poll for 300 s (#12's wait for its DaemonSet) until `observedGeneration` equals the
   generation and `Converged` is `True`,
   or `False` with `AwaitingNodeUpgrade` (it says to run `make node-upgrade`). Otherwise die
   with the conditions' messages.

**`operator undeploy`:**
1. `demo_active` or `unknown`: die.
2. Delete `flaminio-test`; then #12's refusal of `csi undeploy`. Nothing else has changed yet.
3. Delete the operator's Deployment (`--ignore-not-found`). Poll `kubectl_lab get pods -n
   flaminio-operator -l app=flaminio-operator -o name` until it prints nothing (120 s); a
   failure dies. Then delete the resource, when `present`.
4. The removal steps of `cmd_csi undeploy`, as one shared function (#12).

## Files touched

- `api/kube/storage/v1alpha1/{types,deepcopy}.go`: the resource's types (S1, S2, S4).
- `control/operator/{validation,render,reconcile,status}.go`, with `testdata/`: S3–S12.
- `control/cmd/flaminio-operator/main.go`: `run` and `render` (S5, S10, S13).
- `deploy/templates.go`: embeds `csi/{driver,rbac}.yaml`, `dataplane/dataplane.yaml` and
  `operator/operator.yaml` (S5, S18).
- `deploy/operator/{operator,cluster}.yaml`: the operator's manifest, the resource (S14–S19).
- `scripts/lab.sh`, `scripts/dev/operator-image.sh`: the commands, the image (S19, S20, S23).
- `scripts/tools/operator-test.sh`; `Makefile`: two targets, the test in `lint`, `deploy` in
  the `gofmt` list, the API server's binaries for `test` and `race`; `go.mod`, `go.sum`.
- No proto, Rust, metadata client or on-disk change.

## Contracts

**`spec`,** every field required:

| Field | Bound | Pattern |
|---|---|---|
| `images.csi`, `images.dataplane` | 512 | `^[a-z0-9][a-z0-9._/:-]{0,254}(:[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}(@sha256:[a-f0-9]{64})?\|@sha256:[a-f0-9]{64})$` |
| `metadataEndpoints` | 1 to 7, a set; 259 each | `^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?:(6553[0-5]\|655[0-2][0-9]\|65[0-4][0-9]{2}\|6[0-4][0-9]{3}\|[1-5][0-9]{4}\|[1-9][0-9]{0,3})$` |
| `trustDomain` | 253 | `^[a-z0-9]([a-z0-9.-]{0,251}[a-z0-9])?$` |
| `diskSerialPattern` | 64 | `^[A-Za-z0-9][A-Za-z0-9_:-]{0,63}$` |

The schema and `Validate` use these same bounds and patterns, and nothing else.

**Placeholders:** `{{CSI_IMAGE}}`, `{{IMAGE}}`, `{{META_ENDPOINTS}}` (joined by `,`),
`{{TRUST_DOMAIN}}`, `{{SERIAL}}`; in `cluster.yaml`, `{{META_ENDPOINTS_JSON}}` (a JSON array).

**`status`:** `observedGeneration` (the generation read in step 1); `workloads[]`, keyed by
`kind` and `name`, with `wanted`, `ready`, `updated`; `conditions[]` (`metav1.Condition`, so
a `message` of at most 32768 bytes, which the schema states):
- **`Blocked`:** `True` with step 9's reason, else `False` (`NotBlocked`, no message). The
  message has one entry per object found, joined by `; `: `<Kind>/<name>: <text>`. The text
  is the drifted fields, the owner, the API server's refusal, or the error. `InvalidInput`
  has one entry, `spec: <the failure>`. A text is at most 512 bytes, its `...` included: a
  longer one is cut on a rune boundary and ends in `...`. With six entries at most, a
  message stays under 4 kB.
- **`Converged`,** the first of these that holds:
  1. `False`, `Blocked`: `Blocked` is `True`. No message.
  2. `False`, `AwaitingRollout`: a workload's `observedGeneration` is stale, or its `ready` is
     under `wanted`, or, the data plane apart, its `updated` is. One entry per such workload:
     `<Kind>/<name>: <ready> of <wanted> ready, <updated> updated`.
  3. `False`, `AwaitingNodeUpgrade`: the data plane's `updated` is under `wanted`. The
     message: `<n> of <m> flaminio-dataplane pods wait for make node-upgrade`.
  4. `True`, `Converged`.

**The operator's rules,** all of them (ServiceAccount `flaminio-operator/flaminio-operator`),
each verb with what uses it. `resourceNames` limit only the named verbs. An open verb covers
every object of the kind in its scope: RBAC cannot limit a `create`, a `list` or a `watch` by
name. S15 says the same in words.

| Where | Resource | Open verbs | Named verbs | Used by |
|---|---|---|---|---|
| cluster | `flaminioclusters` | list, watch | get: `flaminio` | the one watch; a pass's read |
| cluster | `flaminioclusters/status` | | patch: `flaminio` | the status |
| cluster | `csidrivers` | create | get, patch: `csi.flaminio.io` | a pass's read; the apply (create and patch) |
| cluster | `storageclasses` | create | get: `flaminio` | a pass's read; the plain create |
| `flaminio-system` | `deployments`, `daemonsets` | create | get, patch: the four names | a pass's read; the apply |
| `flaminio-operator` | `leases` | create | get, update: `flaminio-operator` | the election |
| `flaminio-operator` | `events` (core group, `""`) | create, patch | | the election's events |

**Commands:** `flaminio-operator run`; `flaminio-operator render --template csi|dataplane
--csi-image --dataplane-image --meta --trust-domain --serial` prints the substituted text.
`make operator-deploy`, `make operator-undeploy`. Refusals, exit 1:
- `this cluster has a FlaminioCluster: use make operator-deploy or make operator-undeploy`;
- `cannot tell whether this cluster has a FlaminioCluster`;
- `the operator is not installed in the Mac demo`.

## Tests

Go tests are in `control/operator`, against an API server (envtest) unless marked unit.

| Rules | Tests |
|---|---|
| S1–S3 | `TestFixtures` (schema and `Validate` agree on each fixture: every bound, ports `0`, `65536` and `70000`, a bracket, a quote, another name, an unknown field). `TestPreflightRefusalWritesNothing` (`flaminio-failover` live with another selector: no write to a managed object, counted; the status names it). `TestMidwayFailureResumes` (the third apply fails, or is held past a short `RequestTimeout`: two stay, `ApplyFailed` is in the status, the next pass completes; with the status patch failed too, the next pass writes it; the resource's read fails once: no request about a managed object, and the next pass runs; a live object's read fails, or a dry run fails: `ApplyFailed`, no write to any managed object, the next pass completes). `TestRenderErrorIsInvalidInput` (a template that does not decode: `InvalidInput` in the status, no request about a managed object). |
| S4, S8 | Unit `TestStatus` (counts, and each case of `Converged`'s order, from workload status set by the test; full counts under a stale `observedGeneration`, and a rolling controller with data-plane pods behind, are `AwaitingRollout`). `TestSecondPassWritesNothing` (a counting transport: only reads and dry runs on a second pass and after a restart, with no status patch; no pass starts before the period, after a converged pass or after one that patched the status). |
| S5 | Unit `TestRender` (the six objects equal the decoded templates, with no owner reference; each input lands as the same string, `true`, `123`, `null`, `1e3` and the image `1:2` included, or `Render` fails: never another value; a seventh document is an error; #12's selector, strategy and scheduling fields). `operator-test.sh` `render`: `flaminio-operator render` equals `render`, byte for byte. |
| S6 | `TestRepair` (each object deleted is back once two more passes have ended, also after a pass that ended in `ApplyFailed`; an image set by another manager is put back; an undeclared annotation stays; the StorageClass edited stays, deleted is back). `TestUnsafeDrift` (the DaemonSet's `nodeSelector` edited: no PATCH of it is sent, counted, and the field is named; its strategy edited: `OnDelete` again). |
| Step 9 | `TestBlockedTable`: the test builds every combination that the list allows in one pass of an owner refusal, a drift, a dry-run refusal, a dry-run error and an apply error (at most one error; no apply error with a refusal), and adds `InvalidInput` and a failed read alone: 19 rows. Each row checks the reason by step 9's order, that the message has exactly the objects found (or `spec`, for `InvalidInput`), and which writes were sent. One row's error text is 100 kB: its entry's text is at most 512 bytes, valid UTF-8, ending in `...`, and the status patch is accepted. |
| S7 | `TestChange` (each input reaches its templates; the StorageClass is not changed). `TestNewBuild` (other templates: the workloads change; on an object the operator made, a dropped container is gone). |
| S9 | `TestAdoption` (objects made under kubectl's client-side manager with another CSI image: same UIDs, the resource's image, managed fields changed only by the apply; the StorageClass gets no request but a `get`). `TestAdoptionRefused` (a controller owner: `PreflightRefused` names it, no dry run of it is sent, no managed object is written; a StorageClass with a controller owner changes nothing). |
| S10 | `TestTwoInstances` (the follower writes nothing; the leader exits on loss). `TestStaleWriteConverges` (an apply of the previous generation replayed: back once two more passes have ended). |
| S11, S12 | `TestDeleteResourceRemovesNothing` (one pass after the delete, then none). `TestRemovalOrders` (operator then definition; definition while the operator runs; all of `operator.yaml`: the definition's deletion ends, the six objects keep their UIDs and have no owner reference, the request log names no other object; installed again, converged with no write to a workload). |
| Invariant | `TestNoDelete` (through all of the above: no DELETE request; every PATCH of `flaminio-dataplane` keeps its strategy, selector and scheduling fields). `TestMutantsFail`: `mutantRecreate` (delete and create on an immutable field) and `mutantRollingUpdate` must each fail it; `mutantSkipDriftCheck` (step 5's drift skip removed) must fail `TestUnsafeDrift`. |
| S13, S14, S17 | Unit `TestNoStorageImports`; unit `TestManifest` (the operator's pod: no volume, no host namespace, `readOnlyRootFilesystem`, `drop: [ALL]`, UID 65532, the witness's selector and toleration). |
| S15–S18 | Unit `TestRulesExact` (the rules equal Contracts' table). `TestRunsAsAccount` (RBAC on, as the ServiceAccount, from an empty cluster: the six are made, the StorageClass and a Deployment deleted are back; then again from kubectl-made objects, the StorageClass included; no request is refused; delete, Secrets, the resource's spec, pods, bindings and another Deployment's patch are refused). `TestPodAdmittedRestricted` (the template's pod is admitted in `flaminio-operator`; without `runAsNonRoot` it is refused). Unit `TestWorkloadRightsUnchanged` (`rbac.yaml`'s rules equal a pinned copy). |
| S19, S20, S23 | `scripts/tools/operator-test.sh`, VMs and `kubectl` faked: `deploy-order`, `deploy-twice`, `undeploy-order`, `undeploy-attached` (the log's only delete is `flaminio-test`), `undeploy-no-operator`, `demo`, and `csi deploy` and `undeploy` under `none` (no definition; a definition alone), `present`, `unknown`. |
| S21, S22 | `operator-test.sh` `docs`: `make help` and `docs/lab.md` name both commands, `make node-upgrade` and what deleting `rbac.yaml` removes. The existing script tests pass unchanged. |

`make test` and `make race` run the Go tests; a missing API server binary fails them, never
skips. `make lint` runs `operator-test.sh`; `make hygiene` covers the new modules. `sim`,
`contracts`, `crash-test`, `miri` and `sanitize` are not concerned. Nothing was run.

## Lab

The spec's six checks, a later step, only when asked. `make operator-deploy` formats only a
blank data disk and stops the units on fl-a and fl-b, as #12's `make csi-deploy`. The checks
create and delete pods and PVCs in `flaminio-test`: no benchmark, no reformat. Pass: each
check's criterion in the spec; a check that reads the status, or waits for a repair, polls
for 120 s (Only the lab needs seconds). Restore:
`make operator-undeploy`, then `make csi-deploy`.

## Docs

- **`docs/lab.md`, Makefile help:** both commands; what stays scripted; S12's and S16's
  statements; what a new build rolls; `make operator-undeploy` before benchmarks on the units;
  after the resource is deleted, `make operator-deploy` is the way back to the operator.
- **README, ARCHITECTURE §3, §15.2, §17.2, §18:** the installer exists; the rest is planned.
- **`docs/followups.md`:** the steps left; an uninstall by the operator; IPv6 endpoints;
  taking kubectl's ownership of adopted objects.
- **`docs/ci.md`:** the API server's binaries. No benchmark or performance-plan change.

## Risks

- **Safety:** none rests on the operator. It cannot delete; the data plane is `OnDelete`; S6's
  exception keeps it from moving pods. A DaemonSet that someone else sets to `RollingUpdate`
  rolls until the next pass: #12's grace period covers an attached pod.
- **Security:** the account and the resource's writers are cluster administrators (S16): not
  accepted yet. No binding that lets anyone else write the resource is shipped.
  Settled at approval, 2026-11-13: accepted for this step (question 4 (a)) (see decisions.md, Settled at approval).
- **Compatibility:** `make csi-deploy` clusters and the Mac demo are unchanged. Adopted
  objects keep kubectl's entry and annotation: the limit above, for a later build.
- **Kubernetes behaviour** this relies on, with the test of each: [decisions](decisions.md).
- **Open:** the controller-runtime and client-go releases for k3s 1.36
  (`scripts/lab.conf:114`); how CI gets the API server's binaries; #12's plan must put its
  ServiceAccount in `rbac.yaml`, and should bound its own values. `TestRender` runs on #12's
  template once it exists: until then the claim about it rests on #12's plan.
  Settled 2026-11-13 (user's instruction): client-go v0.36.x and the controller-runtime release built on it; `setup-envtest` from `make test` and `make race` for the API server's binaries; #12 puts its ServiceAccount in `rbac.yaml` and bounds its own values (see decisions.md).

## ADRs

- **ADR 0001** names controller-runtime and client-go: consistent. **ADR 0003:** not concerned.
- **[Proposed](adr-proposal.md):** the framework, the resource API, the rights, the namespace,
  no teardown. Approving the design accepts it. #12's privilege ADR is cited by its issue, not
  by number: 0007 is also a Proposed audit ADR.
  Settled at approval, 2026-11-13: the ADR proposal is accepted with the plan (see decisions.md, Settled at approval).

## Parallelizable?

Yes, in Go only (no Rust unit): the types, renderer and manifests; the reconciler; the lab
commands. Each builds against Contracts. Adoption and the rights are reviewed together.
