# Issue #43 — report

## Review rounds

### Plan review round 1 — a0030ec1379ce5eeccc00145d2816a3b1f42c120

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b1 (design-code and design-security).

- **R1-B1 — High:** The operator never forced field ownership, so adoption, change and repair
  all stopped on a conflict. Evidence: the live objects come from a client-side `kubectl apply`
  (`scripts/lab.sh:902–908`, `:1042–1043`), and each deploy builds a new dated CSI tag
  (`:1025`).
- **R1-B2 — High:** S11 had no approach: teardown waited on a contract nobody defined, with
  the finalizer already installed, so the resource could never be deleted and nothing could be
  uninstalled. Evidence: the draft's `DeletionProtocolUnresolved` gate; #12's S8 needs an
  uninstall.
- **R1-B3 — High:** The finalizer broke S12, and S11 and S12 contradicted each other when the
  definition goes while the operator runs. Evidence: deleting a definition deletes its
  resources; the S12 test checked only owner links.
- **R1-B4 — Medium:** S15 could not be met with S4 and S11: the finalizer and the revisions
  needed rights that it did not list; the StorageClass had rights it never used. Evidence: the
  draft's "two adjustments remain Proposed".
- **R1-B5 — Medium:** Stopping reconciliation on deletion could make S11's wait permanent.
  Evidence: VolumeAttachments go only while the CSI controller runs.
- **R1-B6 — Medium:** S3, S6 and S10 were left unresolved, with tests that had no pass
  criterion. Evidence: the draft's rows "expose S3's atomicity gap" and "record ... as
  unresolved".
- **R1-B7 — Medium:** The `cluster-uid` marker blocked a reinstall. Evidence: a new resource
  has a new UID, and adoption covered only unmarked objects.
- **R1-B8 — Medium:** The install manifest held `flaminio-system`, so removing it removed
  everything; no uninstall path was specified. Evidence: today `csi undeploy` leaves the
  namespace and the Secret (`scripts/lab.sh:1065–1071`).
- **R1-S1 — Medium:** S16 and question 3 (a) understated the rights. Evidence:
  `flaminio-system` admits any pod (`deploy/csi/rbac.yaml:11`); the witness's taint is
  tolerable and its label selectable (`scripts/guest/k3s.sh:40–41`).
- **R1-C1 — Low:** `operator deploy` created the Secret before its namespace. Evidence: today
  the namespace comes first (`scripts/lab.sh:1002`, then `:1003`).
- **R1-C2 — Low:** The guards of S19 and S20 were underspecified. Evidence: a cluster without
  the definition fails to list the kind; a second run was not described.
- **R1-C3 — Low:** The events right did not cover the events the operator would write.
  Evidence: events only in `flaminio-operator`, for objects elsewhere.
- **R1-S2 — Low:** `diskSerial` is a regular expression, not a serial. Evidence:
  `control/cmd/flaminio-agent/main.go:34`, `:41–46`; `control/agent/disks.go:107`;
  `scripts/guest/agent.sh:9`.
- **R1-S3 — Low:** Values reached #12's `sh -c` fragments under a grammar #12 does not have,
  with no bounds. Evidence: #12's design, "The DaemonSet"; bracketed IPv6 was allowed.
- **R1-C4 — Low:** The spec's injected bug was not in the design's tests. Evidence: the test
  table had an injected attach only.
- **R1-C5 — Low:** An upgrade of the operator changes the managed objects with no change to
  the resource. Evidence: the templates are compiled in; S8 said "a new instance".
- **R1-C6 — Low:** The operator's pod lacked `runAsNonRoot: true`. Evidence: the draft's list
  of pod fields.
- **R1-C7 — Low:** `controllerTLSSecretName` was a field the spec did not ask for. Evidence:
  the name is literal in `deploy/csi/driver.yaml:85` and `:220`.
- **Nits 1–5 — Nit:** a link to a missing `decisions.md` and questions cited as chosen;
  `ChangeIsr` is refused to administrators (`meta/service/auth.go:81–82`); two renderers
  listed; markers on the pod template; the S22 row named no test.

Changes/resolution:
- **R1-B1:** forced server-side apply of exactly the rendered fields; refusals only for a
  controller owner or an immutable field; kubectl's ownership is moved at adoption. S6 and S9
  say "whoever set" the fields.
- **R1-B2, R1-B3, R1-B5:** question 5 (b), Proposed: no teardown, no finalizer, no delete
  right. S11 and S12 rewritten; `TestRemovalOrders` covers each order. R1-B5 no longer applies.
- **R1-B4:** S15 is an exact list, equal to the design's table. No right on the resource but
  read and status; StorageClasses read and create. The revisions right is not granted: the
  status counts the pods behind (S4).
- **R1-B6:** S3, S6 and S10 amended as proposed, each with a test that passes or fails.
- **R1-B7:** no marker at all; objects are found by name.
- **R1-B8:** the install manifest is two files, `deploy/csi/rbac.yaml` unchanged; new S23,
  `make operator-undeploy`, with its order and #12's refusal; S12 covers the manifest.
- **R1-S1:** S16 and question 3 (a) state it; intent, Why 6, names the witness.
- **R1-C1, R1-C2:** the order and the three states (`none`, `present`, `unknown`) are in the
  design, with script test cases.
- **R1-C3:** no Event but the election's, about the Lease.
- **R1-S2:** the field is `diskSerialPattern`, with a literal-only grammar (S2).
- **R1-S3:** bounds and one character set for every input; no IPv6 literal; the design says
  why text substitution is then safe.
- **R1-C4:** `TestNoDelete` and `TestMutantsFail`, with two named mutants.
- **R1-C5:** S7 covers a new build; S8 says "the same build"; `TestNewBuild`.
- **R1-C6:** added, with `TestPodAdmittedRestricted`.
- **R1-C7:** the name is fixed; no field.
- **Nits:** `decisions.md` created, with the Kubernetes behaviour the plan depends on and the
  test of each; the questions are cited as Proposed; the intent corrected; one renderer named;
  the S22 row names `operator-test.sh` `docs`.
- **Needs #12's plan:** the file of the ServiceAccount `flaminio-dataplane`; a grammar or
  quoting for its template values.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 43285486b5a60b0401333cd05d355a50527e3bf1

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b2 (incremental design-code and design-security).

- **R2-B1 — High:** The StorageClass could not be created: S15 granted `create` but not
  `patch`, and the operator created by apply. Evidence: design, "Rendering and applying" and
  the rules table; decisions, "Creating by apply needs `create` and `patch`".
- **R2-B2 — Medium:** The status and the install poll read workload status that may predate
  the operator's own write. Evidence: nothing compared a workload's `observedGeneration` with
  its generation.
- **R2-C1 — Low:** "Converged" in the spec's Terms contradicted S4 and lab check 3. Evidence:
  Terms said "every workload is ready"; the status is not converged while pods are behind.
- **R2-C2 — Medium:** The move of kubectl's ownership was tied to "an object's first apply",
  which a crash or a restart skips. Evidence: design, step 8, a second request after the apply.
- **R2-C3 — Low:** S23 and the lab guards had three gaps: no assertion that a refused
  undeploy deletes nothing else; the wait with no operator pod; a definition with no
  resource. Evidence: design, "The lab commands"; `scripts/lab.sh:1066`.
- **R2-C4 — Low:** S6's exception, a safety rule, had no injected bug. Evidence: the invariant
  row had `mutantRecreate` and `mutantRollingUpdate` only.
- **R2-C5 — Low:** S12's "garbage collection removes nothing" could not be seen by tests that
  run no garbage collector. Evidence: `TestRemovalOrders` checked UIDs only.
- **R2-C6 — Low:** The endpoint port's bound had no form in the schema. Evidence: the pattern
  `:[1-9][0-9]{0,4}` accepts 99999.
- **R2-C7 — Low:** S17 and S15 were only part-tested, and the table's "(the four names)" read
  as a limit on verbs that RBAC cannot limit. Evidence: `TestManifest` and the rules table.
- **R2-S1 — Low:** "One plain YAML scalar" ignored YAML typing, and the claim about #12's
  template was not checkable. Evidence: `true` or `123` is a valid trust domain or pattern.

Changes/resolution:
- **R2-B1:** the StorageClass is made by a plain create, never an apply; its rights stay read
  and create. The rules table now names, for each verb, what uses it, and which verbs carry
  `resourceNames`. `TestRunsAsAccount` starts from an empty cluster and deletes the
  StorageClass under RBAC.
- **R2-B2:** S4 and the status rule: a workload whose `observedGeneration` is not its
  generation is `AwaitingRollout`; a `TestStatus` case; listed in decisions.
- **R2-C1:** Terms corrected.
- **R2-C2:** step 8 is keyed on the live managed fields, in every pass; `TestAdoptionResumes`.
- **R2-C3:** `undeploy-attached` asserts the only delete is `flaminio-test`; the wait is a
  poll of `kubectl get`, with `undeploy-no-operator`; a definition alone is `none`, with a
  test case; the docs say `make operator-deploy` is the way back.
- **R2-C4:** `mutantSkipDriftCheck` must fail `TestUnsafeDrift`, which counts PATCH requests.
- **R2-C5:** the tests assert that no managed object has an owner reference and that no other
  object is written; decisions names this as the replacement, and lab check 6 for the rest.
- **R2-C6:** the port's range is in the pattern; fixtures `0`, `65536`, `70000`.
- **R2-C7:** `TestManifest` covers the three fields and the placement; the table is reworded.
- **R2-S1:** typed decoding; the fixtures `true`, `123`, `null`, `1e3`; the claim about #12's
  template rests on #12's plan until the file exists. `rbac.yaml` is not edited by this change.
- **Needs #12's plan:** as in round 1 (the ServiceAccount's file; a grammar or quoting for its
  values; which refusal `make csi-undeploy` applies).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — d04a54b3f5ec0528942172e93f472fb318b6e168

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b3 (incremental design-code and design-security).

- **R3-B1 — High:** The step that moved kubectl's field ownership applied to "each live
  object", the StorageClass included, on which the operator has no patch right: an adopted
  cluster would stay `ApplyFailed`. Evidence: design, step 8 and the rules table;
  `make csi-deploy` makes the StorageClass with `kubectl apply`.
- **R3-C2 — Low:** The same step on a DaemonSet skipped for `UnsafeDrift` was unspecified and
  clashed with `TestUnsafeDrift`'s "no PATCH"; its order relative to the apply left
  `TestNewBuild` with an undefined number of passes. Evidence: design, steps 4, 7 and 8.
- **R3-C3 — Low:** S15 read as limits that RBAC cannot express, and was not equal to the
  table. Evidence: "its Lease, and that Lease's events"; a create covers any name.
- **R3-C4 — Low:** The informers' scope was not stated while the rights are namespaced, and a
  render error after `Validate` had no named outcome. Evidence: a manager's default cache
  lists cluster-wide; design, "Triggers" and step 1.

Changes/resolution:
- **R3-B1, R3-C2:** the step is cut, with `TestAdoptionResumes`. No finding had asked for it.
  The design and S9 state the consequence instead: adopted objects keep kubectl's entry; the
  forced apply owns every field the operator renders; a field that a later build drops stays
  on an adopted object until the cluster is installed anew, and that build's docs must say
  so. Recorded as Proposed in decisions, with a follow-up. `TestRunsAsAccount` gains an
  adoption run under the account, with a kubectl-made StorageClass.
- **R3-C3:** S15 is rewritten in RBAC's terms: by name where a verb can carry one, "any name"
  for creations. The table says that an open verb covers every object of the kind.
- **R3-C4:** one watch, on the resource; the managed objects are read by name with an
  uncached `get`, and a pass runs again after 30 s. The table loses `list` and `watch` on the
  managed kinds, and their `get` is named. A render that fails is `InvalidInput`, with
  `TestRenderErrorIsInvalidInput` and the fixture `1:2`.
- **Needs #12's plan:** as in rounds 1 and 2.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — ff76f396490124bbe2587d63c1d78c9bd120613b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b4 (incremental design-code and design-security).

- **R4-C1 — Low:** A retry "with backoff" could break S6's "within a minute", and "30 s after
  it ends" was not stated for every outcome. Evidence: design, step 7; the framework's default
  backoff grows far past a minute.
- **R4-C2 — Low:** The "uncached `get`" named no mechanism, and the default client reads from
  a cache. Evidence: design, "A pass" and "Triggers".
- **R4-C3 — Low:** The status is up to 30 s stale, and some checks read it once. Evidence:
  the workloads are no longer watched; lab check 3.
- **R4-C4 — Low:** The only way to remove a dropped field (S9's limit) is the uninstall, which
  S23 refuses while a volume is attached. Evidence: design, "The limit (S9)"; S23, step 1.
- **Nit:** `events` in the rules table had no API group.

Changes/resolution:
- **R4-C1:** every outcome asks for the next pass 30 s later, and no error goes back to the
  framework; a pass has a 10 s deadline. Both times are fields the tests set; the tests assert
  "within two periods", also after an `ApplyFailed` pass.
- **R4-C2:** reads go through the manager's `GetAPIReader()`; the cache is limited to
  `FlaminioCluster`. Decisions says these names are to be checked against the pinned version,
  and that `TestRunsAsAccount` fails if a read is cached.
- **R4-C3:** S4 says the status lags by up to 30 s and that its readers poll; the lab checks
  poll.
- **R4-C4:** S9 and the design say the way out needs every volume detached and removes the
  data plane until the reinstall. No gate is added: decisions records it as a follow-up.
- **Nit:** the row says the core group.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 3b96e6054f688b94f8f8464a7df14f9a66f8aa04

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b5 (incremental design-code and design-security).

- **R5-1 — Low:** A pass cut by its 10 s deadline had no stated outcome, status or test.
  Evidence: design, "The period"; no test made a pass exceed the deadline.
- **R5-2 — Low:** `cache.Options.ByObject` does not limit the cache to one kind: another type
  read through the cached client still starts an informer. Evidence: design, "Triggers";
  decisions.
- **R5-3 — Low:** "Within a minute", "up to 30 s" and the 60 s polls ignored the pass's own
  time. Evidence: spec S4 and S6; the lab checks; `TestRepair`'s "two periods".
- **R5-4 — Nit:** a confirmation, not a defect: S9's limit sentence agrees with the design and
  S23. No change.

Changes/resolution:
- **R5-1:** the deadline is cut; no finding had asked for it. Each request has the client's
  timeout, and one that fails or does not answer ends the pass as any API error does, with
  the status as the pass's last request. `TestMidwayFailureResumes` gains the case of a
  request that does not answer.
- **R5-2:** the reconciler is handed only an uncached reader and a writer, so it holds no
  cached client and cannot start an informer; the cache serves the one watch. The `ByObject`
  name and claim are dropped from the design and decisions.
- **R5-3:** one derivation, in the design's "The period", in passes: a change is repaired,
  and in the status, once two more passes have ended. S4 and S6 are stated in passes; the
  tests wait on a pass count; the lab's 60 s poll gives its terms (30 s between two passes,
  two passes of 15 s at most), in the spec and the design.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — b8ace901b1f54da16708364b93df7e690c141cdc

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b6 (incremental design-code and design-security).

- **R6-1 — Low:** "The client's timeout (10 s)" was stated as a fact, but nothing sets one by
  default, and no test could set it. Evidence: design, "The period"; `TestMidwayFailureResumes`.
- **R6-2 — Low:** "The status is the last request of every pass" contradicted `InvalidInput`
  "with no request" and `TestSecondPassWritesNothing`. Evidence: design, step 1, "The period"
  and the S4, S8 row.
- **R6-3 — Low:** The lab's 60 s poll had no margin and left out a change of leader.
  Evidence: 15 s + 30 s + 15 s is exactly 60 s; the Lease's duration is 15 s.
- **R6-4 — Low:** The guard against a cached read was a test only: nothing said how the
  reader and the writer are built. Evidence: design, "Triggers"; a manager's own client reads
  a cache.
- **Nit:** decisions said "read by name every 30 s"; it is 30 s after a pass ends.

Changes/resolution:
- **R6-1, R6-4:** one statement, "The reconciler's client": `operator.New(config, Options)`
  builds the operator for `main` and for the tests; it gives the reconciler one client, built
  without a cache from a copy of the configuration whose timeout is `Options.RequestTimeout`.
  The manager keeps the configuration unchanged, so its watch and election are not cut.
  `TestRunsAsAccount` runs that function; `TestMidwayFailureResumes` sets a short timeout.
- **R6-2:** one rule: the status is written at the end of a pass, whatever its outcome, only
  when it differs from the one read; a failed patch is written by the next pass.
  `InvalidInput` and a preflight refusal now say "no request about a managed object"; the
  tests' wording agrees. A pass that finds no resource makes no other request.
- **R6-3:** the poll is 120 s: two passes of 15 s at most, the 30 s between them and one
  change of leader (17 s) make 77 s; the rest is margin. Spec, design and decisions quote it.
- **Nit:** corrected.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — abb4f5e86c15bb71995802d85e5e11dd74cf4706

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b7 (incremental design-code and design-security).

- **R7-1 — Low:** A failed read of the resource had no outcome: it was neither "found" nor
  "not found", so the chain of passes could stop on an unreachable API server. Evidence:
  design, "The period" and "Deleting the resource"; no test failed that read.
- **R7-2 — Low:** "No request about a managed object" contradicted the pass's first sentence,
  which read the live objects before step 1. Evidence: design, "A pass" and
  `TestRenderErrorIsInvalidInput`.
- **R7-3 — Low:** The operator's own status patch wakes the watch, and nothing said whether
  it starts a pass or why passes cannot loop. Evidence: the one watch is on the resource.
- **R7-4 — Low:** The lab's 17 s for a change of leader left out the pod's restart. Evidence:
  design, "Only the lab needs seconds"; one replica, which exits on loss of the Lease.

Changes/resolution:
- **All four:** the pass is written once, as ten numbered steps in the design, each with what
  a success, a not-found, an error (a timeout included) and a refusal do: the status reason,
  whether anything is written, and whether the next pass is asked for. The sentences that
  restated parts of it (the status rule, "The period", the pass in "Deleting the resource",
  three decisions) are removed or point to the list.
- **R7-1:** only a resource that is not found ends the chain; an error on that read goes to
  the step that asks for the next pass. `TestMidwayFailureResumes` gains that case, and
  `TestDeleteResourceRemovesNothing` checks one pass, then none.
- **R7-2:** the inputs are checked and rendered (step 2) before the live objects are read
  (step 3).
- **R7-3:** the watch starts a pass on a creation, a deletion or a change of generation, so
  not on a status patch; and a pass patches the status only when it differs from the one
  read. `TestSecondPassWritesNothing` checks that no pass starts before the period.
- **R7-4:** the 43 s margin is named as also holding the pod's restart, in the design, the
  spec and decisions.
- Also stated while closing the steps: a workload's counts come from the last answer the
  pass got (the write's, after a write), and `ApplyFailed` is the reason of any failed
  request about a managed object.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — 36670aa5d5c3e77d3c07a85b2abc5f8d8302d469

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b8 (incremental design-code and design-security).

- **R8-1 — Low:** The reason's precedence when a pass finds two outcomes was unstated, so a
  failed apply or a refusal could hide the drifted fields that S6 says the status names.
  Evidence: design, step 9; no write happens in any case.
- **R8-2 — Low:** Errors in steps 3 and 6 had no test: a read error taken as "absent", or a
  dry-run error taken as a refusal, passed every listed test. Evidence: the cases of
  `TestMidwayFailureResumes`.
- **R8-3 — Low:** An object refused at step 4 was not said to leave step 6. Evidence: design,
  steps 4 to 6; `TestAdoptionRefused`.
- **R8-4 — Nit:** Step 9 on a first pass: there are no counts to keep.
- **R8-5 — Nit:** S3 said "reported for its workload"; the reason covers all six objects.

Changes/resolution:
- **R8-1:** step 9 gives the order (`InvalidInput`, `ApplyFailed`, `PreflightRefused`,
  `UnsafeDrift`) and says the message lists every object found refused, failed or drifted,
  with the drifted fields, whatever the reason. `TestUnsafeDrift` gains the drift with a
  failed apply.
- **R8-2:** `TestMidwayFailureResumes` gains a failed read of a live object and a failed dry
  run: `ApplyFailed`, no write to any managed object, the next pass completes.
- **R8-3:** step 4 says the refused object leaves the pass and gets no dry run;
  `TestAdoptionRefused` asserts it.
- **R8-4:** step 9 says "or none on a first pass".
- **R8-5:** S3 says "for its object".

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — a78216ff38b92a6d716183eccfad6de097e18633

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b9 (incremental design-code and design-security).

- **R9-1 — Low:** Only one case of the reason order was tested (a drift with a failed apply);
  an implementation that kept only the winning reason's objects passed. Evidence: design,
  step 9 and the tests of `PreflightRefused`.
- **R9-2 — Low:** The `Blocked` message had no bound: it joins up to six objects, each with
  an API server's error text, and a condition's message is limited to 32768. Evidence:
  design, Contracts; an over-long message would fail the status patch in every pass.
- **Nit 1:** Contracts' message format did not cover a refusal's text.
- **Nit 2:** Step 4 applied to the StorageClass, which step 5 says is never compared.

Changes/resolution:
- **R9-1:** one table test, `TestBlockedTable`: it builds every combination the list allows
  in one pass (19 rows) and checks the reason, the objects in the message and the writes
  sent. The prose case in `TestUnsafeDrift` is removed.
- **R9-2:** an entry's text is cut at 512 bytes and ends in `...`; with six entries at most a
  message stays under 4 kB, against the 32768 that the schema states. One row of the table
  has a 100 kB error and checks that the status patch is accepted.
- **Nit 1:** the entry's text is the drifted fields, the owner, the refusal or the error;
  `InvalidInput` has its own entry.
- **Nit 2:** the StorageClass is exempt from step 4; `TestAdoptionRefused` checks that one
  with a controller owner changes nothing.
- **Also closed, from re-reading steps 4 to 9:** dry runs go on after a refusal; the status's
  `observedGeneration` is the generation read in step 1; `Converged` has a stated order of
  reasons and a message for `AwaitingRollout`, with `TestStatus` covering each case.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — 13c1e9040fa1436d8932ffc0bcd0c381ad7d2339

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b10 (incremental design-code and design-security).

- **L1 — Low:** The 512-byte cut was stated on an entry's text and tested on the whole entry,
  which has a `<Kind>/<name>: ` prefix; the cut's boundary was not stated. Evidence: design,
  Contracts and `TestBlockedTable`.
- **L2 — Low:** Step 4's "leaves the pass" read as ending the pass, which would make eight
  rows of the table unreachable. Evidence: design, step 4.
- **Nit 1:** Step 7 said "naming the object" while step 9 lists every refused object.
- **Nit 2:** `InvalidInput`'s `spec:` entry is not an object, though step 9 and the table
  said "the objects found".
- **Nit 3:** "The StorageClass is not written at all" against steps 6 and 8, which create it
  when absent.

Changes/resolution:
- **L1:** the bound is on the text, `...` included, at most 512 bytes, cut on a rune
  boundary; the table's row asserts that, and that the text is valid UTF-8. Decisions says
  the same.
- **L2:** step 4 says that object gets no dry run and the others go on.
- **Nit 1:** step 7 says "naming each refused object"; S3 says the status names each.
- **Nit 2:** step 9, the table and decisions say "or `spec`, for `InvalidInput`".
- **Nit 3:** "never changed or deleted", in the design and decisions; `TestChange` says "not
  changed".

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 11 — f23ee17c30ba8a00c575a15a42ca0737d864fd9d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r43b11 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
