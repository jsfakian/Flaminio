# Plan review: issue #43 (the operator), round 1, design-code + design-security + plan checklists

Scope: `docs/issues/43/{intent,spec,design,adr-proposal}.md` in `/home/jsfakian/Documents/src/flaminio-43`.
Checked against that worktree's code and, where the plan relies on it, #12's plan in
`/home/jsfakian/Documents/src/flaminio-12/docs/issues/12/`. Nothing was run or changed.
Facts that hold: the line references into `scripts/lab.sh` (991, 1003, 1004, 1064-1080),
`deploy/csi/rbac.yaml:11`, `scripts/guest/k3s.sh:39`, `control/failover/kube.go`, `go.mod`
(no client-go / controller-runtime), ADR 0001 naming both, k3s 1.36 (`scripts/lab.conf:114`),
#12's placeholders, account and `ControllerRevision` rule.

### R1-B1 — High
**The operator never forces field ownership, so adoption, change and repair all stop on a conflict.**
- Design "One renderer and object ownership": "Do not force-take foreign field ownership. Legacy
  kubectl ownership conflicts stop ... not a blanket ForceOwnership" (design.md:72-75); test row
  S9 "no forced takeover".
- The legacy objects are written by client-side `kubectl apply` (`scripts/lab.sh:902-908`, `:1039`),
  which owns every field it set. A server-side apply that sets such a field to another value
  conflicts; one that sets the same value shares it, and then conflicts at the first change.
- S9 in the lab's own check 1: the installer builds a new dated CSI tag
  (`scripts/lab.sh:1025`), so `images.csi` differs from the live value: ApplyConflict, always.
- S7: even after an adoption with equal values, the first image change conflicts with kubectl's
  shared ownership, for ever.
- S6: `kubectl edit`, `patch` or `set image` moves the field to that manager; putting it back
  is a conflict. The ordinary repair case is never repaired, and the S6 test row cannot pass.
- Fix: force on exactly the rendered fields (the controller idiom), after the adoption checks
  (known name, selector, no controller owner). Keep "left alone" for fields the operator does
  not declare. Conflict stays only for identity: owner references, immutable fields.

### R1-B2 — High
**S11 has no approach: teardown is gated on a contract nobody defines, with the finalizer already installed.**
- design.md:103-106, adr-proposal.md:57-61: "Automatic teardown stays blocked with
  DeletionProtocolUnresolved until an attachment-exclusion contract is agreed."
- Result as designed: deleting the resource hangs for ever, even with nothing attached; the
  spec's lab check 6 cannot run; `make csi-undeploy` refuses (S20). There is no way to
  uninstall, and #12's S8 needs an uninstall before the unit benchmarks on fl-a and fl-b.
- The exclusion exists in the code, without admission or a metadata call. An attach completes
  only through the CSI controller: `attachRequired: true` (`deploy/csi/driver.yaml:12`),
  `ControllerPublishVolume` (`control/csi/controller.go:274-288`); kubelet does not stage before
  the VolumeAttachment is attached.
- Fix, one of:
  1. Ordered teardown with the rights S15 already has: list (zero); delete the CSI controller's
     Deployment and wait until its pods are gone; list again, uncached. Any object: put the
     controller back and wait. Zero: no attach can complete any more; remove the rest, the data
     plane last. #12's S4 (a data-plane pod keeps serving up to 3600 s while attached) covers
     direct attaches. S11 then needs one sentence allowing the controller to stop first.
  2. Question 5 (b): no teardown, no finalizer; a lab command uninstalls, with #12's refusals.
     This also removes R1-B3 and the finalizer right of R1-B4.

### R1-B3 — High
**The finalizer breaks S12, and S11 and S12 contradict each other in the spec.**
- design.md:109-110 claims that no owner references means removing the operator or the CRD
  leaves the workloads running. Deleting a CRD deletes its resources, so:
  - operator running, CRD deleted: the resource gets a deletion timestamp and the operator
    runs S11. With nothing attached it removes every workload (after the gate), or hangs (now).
  - operator removed first, then the CRD: the resource keeps its finalizer; the CRD stays
    Terminating until someone strips it by hand, and a reinstall is blocked.
- Spec: S12 ("removing the definition removes no workload") and S11 ("deleting the resource
  removes the managed objects") cannot both hold when the definition goes while the operator
  runs. The S12 test row checks only owner links, so it passes while the rule fails.
- Fix: decide in the spec. Either S12 says the definition cannot go while the resource exists
  (and names the supported order and how to keep the workloads), or the operator reads its
  CRD (one more read right) and releases the finalizer without teardown when the CRD is
  terminating. Add a test for both orders against an API server (it runs CRD cleanup).

### R1-B4 — Medium
**S15 cannot be met together with S4 and S11; the design leaves the two rights "Proposed" (the known item).**
- S15 lists "all" the rights: read the resource, write its status. S11 needs the resource to
  stay (a finalizer); S4 needs the newest revision to name pods behind. The spec test "the
  account's rules are exactly S15's" fails with design.md:137-139.
- Finalizer: RBAC cannot grant "patch metadata". It is `patch` (or `update`) on
  `flaminioclusters` itself, `resourceNames: [flaminio]`, which also lets the account rewrite
  the spec. `flaminioclusters/finalizers` does not grant it. No escalation (S16 already equates
  the account with a writer), but S15 and S16 must say it.
- Revisions: `get/list/watch controllerrevisions` in `flaminio-system` is needed (the hash is
  not in the DaemonSet's status) and matches #12's rule (#12 design, "The newest revision").
  It exposes only pod templates the account can already read. Fine to grant.
- StorageClass: the design grants update/patch/delete (design.md:130) for an object it never
  patches or deletes (design.md:77). Grant get/list/watch/create only; S15 says "update and
  delete the managed objects' kinds" and should except it.
- Fix: amend S15 (and its test) with these exact lines; drop the "Proposed" wording.

### R1-B5 — Medium
**"On deletion, stop ordinary reconciliation" can make the wait of S11 permanent.**
- design.md:98-99. A deletion cannot be undone, and S11's wait lasts as long as a volume is
  attached. VolumeAttachments carry the external-attacher's finalizer: they go only while the
  CSI controller runs, and a detach needs the node plugin. With repair off, a controller or
  node plugin deleted or broken during the wait is never put back, and the wait never ends.
- The spec does not say whether S6 and S7 hold while the resource is terminating.
- Fix: spec: S6 (and S7) hold until teardown starts. Design: keep reconciling; only the
  teardown step is new. Test: delete while attached, delete the controller's Deployment, it
  is back within a minute.

### R1-B6 — Medium
**S3, S6 and S10 are left unresolved, with tests that have no pass criterion.**
- design.md:190-193, adr-proposal.md "Open contracts" 1-3; test rows "expose S3's atomicity
  gap", "record delayed in-flight write counterexample as unresolved".
- S3 "passes but cannot be applied changes nothing": the likely cases (image not imported,
  Secret absent) cannot be known before applying; the operator may not read Secrets (S14). A
  wrong image then stops the failover controller (`strategy: Recreate`,
  `deploy/csi/driver.yaml:184`) and one node plugin.
- Fix: amend the spec, then give each a test that passes or fails:
  - S3: a preflight (dry-run) refusal writes nothing; anything found later is reported per
    workload, and what was applied stays.
  - S6: drift whose repair would remove a data-plane pod is not repaired; status names it.
  - S10: one instance reconciles under a Lease; a deposed instance's in-flight write may land
    and is harmless, as writes are conditional and computed from the same resource. State that
    no safety rests on it.

### R1-B7 — Medium
**The `cluster-uid` marker blocks a reinstall.**
- design.md:62-67: a foreign UID is ApplyConflict; adoption covers only unmarked objects.
- After S12's removal and a new resource (new UID), or after a finalizer stripped by hand,
  every managed object carries a dead UID: all ApplyConflict, with no way out but editing labels.
- Fix: the resource is a singleton by name (S1), so drop the UID marker, or adopt objects whose
  marked UID is not the live resource's. Test: remove operator and definition, reinstall,
  converged with no workload restarted.

### R1-B8 — Medium
**The install manifest holds `flaminio-system`; removing it removes everything. No uninstall path is specified.**
- design.md:120-121: the bundle has both namespaces and the workloads' accounts. Today
  `csi undeploy` deletes only what `driver.yaml` made (`scripts/lab.sh:1065-1071`); the
  namespace and the Secret stay.
- `kubectl delete -f install.yaml`, the obvious way to "remove the operator", deletes the
  namespace: every workload, the controllers' Secret (S11 says never touched), the accounts.
  S12 speaks only of the Deployment and the definition.
- S22 promises "how to uninstall", but no rule or command says what it is, or who removes
  #12's node labels and applies #12's refusals (S4: nothing attached on any socket).
- Fix: a spec rule for uninstalling (the command, its order, its refusals), and S12 extended
  to the install manifest; or split the bundle so that the operator's part can be deleted alone.

### R1-S1 — Medium
**S16 and question 3 (a) understate what the rights amount to.**
- S16: "a privileged pod on every data node". `flaminio-system` admits any pod
  (`deploy/csi/rbac.yaml:11`); the witness's taint is tolerable and its label selectable
  (`scripts/guest/k3s.sh:40-41`). The account can run a privileged pod on the witness: the API
  server, its datastore and the Secrets' encryption key (intent, Why 6). It can also run pods
  as any service account of `flaminio-system` (PV create, node patch).
- Question 3 (a) says pods of `flaminio-system` cannot reach a CA key kept in
  `flaminio-operator`. They cannot mount it; whoever can create a workload there can read it
  on the witness. That is the flaw stated for (b).
- The design says "cluster-equivalent" (design.md:140-142); the spec does not.
- Fix: S16 states root on every node, the witness included, hence cluster administrator;
  question 3 states that (a) protects the key from mounts only, in this lab.

### R1-C1 — Low
**`operator deploy` creates the Secret before its namespace exists.**
- design.md:148-150: identities and Secret, then the install bundle. On a new cluster
  `flaminio-system` comes from the bundle; today the namespace is applied first
  (`scripts/lab.sh:1002`, then `:1003`). The S19 test row would fix the wrong order.
- Fix: bundle (or its namespaces) first, then the Secret, then the resource.

### R1-C2 — Low
**The guards of S19 and S20 are underspecified.**
- design.md:151-152: "failure to determine that state also refuses". On a cluster without the
  definition (every script-installed cluster, the Mac demo), listing the kind fails: the guard
  must read "no definition" as "no resource", or `make csi-deploy` refuses everywhere.
- design.md:147-148: "inspect existing FlaminioCluster (including deleting ones)" does not say
  what follows. A second run must update the resource (the issue's "and upgraded"); a
  terminating one must refuse.
- Fix: state both outcomes; name them in `operator-test.sh`.

### R1-C3 — Low
**The events right does not cover the events the operator would write.**
- design.md:132-133: events in `flaminio-operator` only. client-go records an event about a
  cluster-scoped object in `default`, and about a Deployment in `flaminio-system`.
- Fix: say that events are only the Lease's, with everything else in status; or grant
  `create/patch` on events in `default` and state it in S15.

### R1-S2 — Low
**`diskSerial` is a regular expression, not a serial.**
- `flaminio-agent claim --serial REGEXP` (`control/cmd/flaminio-agent/main.go:34-46`), matched
  unanchored (`control/agent/disks.go:107`); the lab passes `data` (`scripts/guest/agent.sh:9`).
- The grammar of design.md:34 allows `.` (a wildcard) and no anchor, so an exact serial cannot
  be written, and a short value matches more disks than intended. The pod never formats (#12
  S2), which bounds the harm.
- Fix: name and document it as a pattern (spec S2, Inputs), and choose its grammar on purpose.

### R1-S3 — Low
**Values reach #12's `sh -c` fragments under a grammar #12 does not have; no bounds are given.**
- design.md:53-54: "#12 shell fragments retain their strict input grammar". #12's design
  defines none; its claim script and `flaminio-io node` line take `{{SERIAL}}`,
  `{{META_ENDPOINTS}}` and `{{TRUST_DOMAIN}}` unquoted (#12 design, "The DaemonSet").
- design.md:25-26 allows bracketed IPv6: `[...]` is a glob in `sh`, and a flow sequence in
  YAML if substitution is textual.
- No `maxItems`/`maxLength` for the endpoints, the image or the trust domain: CEL rules over
  unbounded strings can exceed the cost budget, and the values reach argv.
- Fix: state each field's bounds; set values on decoded objects, not in text; pass them to
  #12's shell through `env`, quoted, or exclude brackets. Refusal fixtures for each.

### R1-C4 — Low
**The spec's injected bug is not in the design's test table.**
- spec.md:138-139: an operator that deletes the data plane's DaemonSet, or a pod of it, while
  attached must fail the tests. design.md:163-175 has an injected attach, no injected bug.
- Without a controller manager no pod is ever removed, so the check must be on the requests
  (no DELETE on the DaemonSet; no change to its strategy, selector or scheduling fields).
- Fix: name the mutant and the test that fails on it.

### R1-C5 — Low
**An upgrade of the operator changes the managed objects with no change to the resource.**
- The templates are compiled in (design.md:50). A new operator image with another sidecar
  version or data-plane template rolls the controllers and adds a data-plane revision.
- S8 says a restart or "a new instance" writes nothing; S4's generation does not show it.
- Fix: S8 "of the same build"; a rule and a test for a new build (what rolls, what waits for
  `make node-upgrade`).

### R1-C6 — Low
**The operator's pod lacks `runAsNonRoot: true`.**
- design.md:124-126 lists UID 65532 but not `runAsNonRoot`. Pod Security `restricted` (S17)
  refuses a pod without it, so the Deployment would have no pod.
- Fix: add it to the list and to the bundle test.

### R1-C7 — Low
**`controllerTLSSecretName` is a field the spec does not clearly ask for, and the code fixes the name.**
- Spec: Terms lists the Secret and the nodes' identity files as inputs the resource names;
  S2 says names are fixed and identities stay where they are. The design makes the Secret a
  field and the files fixed (design.md:27-29), without saying why they differ.
- The name is literal in `deploy/csi/driver.yaml:85` and `:220` and in `scripts/lab.sh:1003`.
- Fix: settle the spec's Inputs; simplest is a fixed name and no field.

## Nits
1. spec.md:3-4 links `decisions.md`, which does not exist; S11, S16 and S17 cite
   "(question N, a)" as chosen while the questions are open.
2. intent.md:48: "Any `admin` identity may call every metadata method". `ChangeIsr` is
   refused to administrators (`meta/service/auth.go:81-82`).
3. design.md:155-157 lists `control/operator/render.go` and `deploy/render.go` under "one
   renderer"; say which renders, and whether `install.yaml` copies `deploy/csi/rbac.yaml` or
   is built from it.
4. design.md:62: say the markers go on the objects' metadata, never on the pod template (a
   template label is a new revision and rolls the controllers).
5. design.md:175: the S22 row names no test or script.

Reviewed commit: a0030ec1379ce5eeccc00145d2816a3b1f42c120
