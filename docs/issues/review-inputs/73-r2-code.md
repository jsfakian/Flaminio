# #73 round 2, design-code review (diff 094005f..ca5ee3a)

## Round 1 status
The cut text (intents, AckAuditExport, webhook, S12, retention, KMS hooks) is gone from spec, design,
contracts and decisions; what remains refers to them only as out of scope. The 23-method table matches
the 13 methods of meta.proto plus the 10 new ones, and auth.go's switch (auth.go:62-85). Txn sizes are
within 120 (finishDeletion about 6 ops, sweep batch 119 deletes + 1 put). Quota release sits in the
header-deleting transaction. `pvc-<UID>` matches the provisioner's default name. PVC `get` is already in
rbac.yaml:35. Nothing else to report on those.

## Findings

1. **High: an interrupted creation of a protected volume can never be cleaned up.**
   Evidence: design "CreateVolume once active" and "DeleteVolume": the claim writes a `Creating`
   header, the owner record (protected from the policy) and the charge in one transaction; the header is
   cleared only at the end of createVolume (service.go:413,466). A metadata crash, lost leadership or
   etcd error between them (16+ chunk batches) leaves it. `readHeader` hides Creating headers
   (service.go:287), so the retried CreateVolume gets AlreadyExists, then GetVolume NotFound, forever.
   Today the PVC's DeleteVolume cleans it. Under the plan DeleteVolume refuses a protected volume
   ("protected, refused"), AssignOwner refuses a creating one, and the charge stays. The CLI's
   `--no-protection` implies protection is the default, so this is the common case. Only an override
   (a separately credentialed role) clears it, and the PVC's DeleteVolume is retried forever. S4 says an
   interrupted creation never releases a charge, but no rule or test covers cleaning it.
   Fix: a header with `Creating` set was never exposed (no attach, no data), so DeleteVolume may delete
   it whatever the protection, by the owner kind's role, with the audit events. Say so in S5 (an
   explicit exception) and add a test: crash the creation after the claim, then PVC delete releases the
   charge, on both backends.

2. **Medium: the budget's `reserved` leaks when an older binary deletes the header of an admitted
   deletion.** Design "OverrideDelete, no header" and contracts "Budget": the `header-missing`
   `delete.committed` is "as any other event" and takes nothing from `reserved`, even when the owner
   record carries a `deleting_op` (an admitted deletion). The slot is never released, and nothing
   (not even pruning) lowers `reserved`; a few such cases shrink a chain permanently, and S9 says
   a reservation is released by the deletion's completion. Fix: if the owner record has a `deleting_op`,
   the header-missing commit releases that slot; test it.

3. **Low: an orphan owner record (older binary deleted the header) blocks recreating the id.**
   Design "A failed claim": a claim failing on `owners/<id>` with no header and no tombstone is read
   again and ends in Aborted after 32 attempts. The CSI controller turns that into endless retries with
   no hint to the operator. Fix: when the failed claim finds an owner record and no header, answer
   FailedPrecondition `volume %q has an orphan owner record; the override removes it`, at once, not
   after 32 backoffs. Test it.

4. **Low: the contracts' claim about an older binary is wrong.** Contracts "CLI": "an older binary
   answers PermissionDenied: its interceptor knows no such method". gRPC answers `Unimplemented` for an
   unregistered method before any interceptor runs. The "any error refuses" rule still holds, but the
   `main_test.go` stand-in "answering PermissionDenied" does not model it. Fix: say Unimplemented and
   make the stand-in answer both.

5. **Low: the Kubernetes lookup has gaps.** (a) `failover.APIClient.do` returns an untyped
   `fmt.Errorf` with the HTTP status text (kube.go:75-77), so the contracts' NotFound versus
   Unavailable split needs a typed error from `do`; the design does not say it changes it.
   (b) Namespace and name from request parameters go into the URL path with no validation or
   `url.PathEscape` (AGENTS: ids reaching sockets are validated). Fix: check DNS-subdomain format and
   escape; state the `do` change; test a 404 and a 500 against `httptest`.

6. **Low: the race guard covers only audited calls.** Design "The race": a call authorized under
   `inactiveRules` compares `activation` absent only in DeleteVolume and ConfirmDeletion. A
   CreateVolume by any Admin name that read inactive and claims after activation commits creates an
   unassigned, uncharged volume after activation; MoveReplica, DrainNode and Attach likewise run once
   by a non-`cluster-admin` administrator. The window is one call, but S11 and the invariant
   say nothing of it. Fix: compare `activation` absent in CreateVolume's claim too (cheap, the claim
   is already conditional), or state the exception in S11.

7. **Low: ActivateTenancy and TenantStatus pass for anyone on an insecure inactive instance**
   (design "Authorize, in order"). One unauthenticated RPC permanently activates the store, after which
   that instance refuses to start or to serve. Contracts list `cluster-admin` for ActivateTenancy.
   Fix: an insecure instance refuses `ActivateTenancy` (FailedPrecondition).

8. **Low: tests that cannot fail or are missing.** The insecure branch of Authorize (active and
   inactive) is in no test row (S1 says "over mTLS"); the 10 lines per second limit and the
   "refusals suppressed" count (contracts) are not in S7-S9's flood test, which checks only store
   writes; S2's "a refusal names only the caller's own namespace" has no message check; the
   cached-`active` behaviour (reads until found, then cached) has no test.

## Nits
- Design "Deletion": "Header marked with a `deleting_op`" and the contracts put `deleting_op` in the
  owner record, not the header; say "owner record" throughout.
- Spec S7: "one event with its mark and one with its tombstone" omits marked-unassigned (one event) and
  the unparseable header (both in one transaction).
- Contracts "Partition of an event": `audit.pruned` "the chain it prunes" is also in the Budget text as
  "always admitted"; the event may exceed `cap` by one, say so.
