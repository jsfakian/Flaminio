# Issue #73 plan review — round 3 (incremental, design-security)

Scope: `git diff ca5ee3a..7a91186 -- docs/issues/73/ ':!docs/issues/73/report.md'` in
`/tmp/flaminio-plans-2026-11-12/73` (972b2f0, the round-2 fix, and 7a91186, the approval), checked
against the code of that worktree and, where the plan relies on it, #76's approved spec.

## Earlier findings

- R2-S1: fixed for tenants (per-namespace `ns-` chains). What remains: security events share
  `sys-00` with `cluster-admin` (R3-S1), and the total space has no bound (R3-S2).
- R2-S2: fixed (one record in the store, read on each call). New exposure: R3-S3.
- R2-S3: fixed (key in the record, which `cluster-admin` cannot write; the prune compares the
  record's revision; `audit.pruned` holds the receipt and fingerprint).
- R2-S4: fixed (insecure instances refuse every new method; handlers refuse with no role).
- R2-S5: fixed (namespace read first; NamespaceLifecycle forbids new PVCs in a terminating namespace).
- R2-S6: fixed for creation. AssignOwner still lets `system` take a tenant id (R3-S9).
- R2-S7: fixed.
- R2-S8: fixed (per caller, bounded table). Counts are lost when the table is cleared (R3-S8).
- R2-S9: fixed (stated; follow-up in intent).
- R2-S10: fixed.
- R2-S11: fixed.
- R2-S12: fixed (`after >= next − 1` gives an empty page, so `after + 1` cannot wrap).
- R2-C1: fixed. The "being created" test is weaker than #76's (R3-S11).
- R2-C2, R2-C3, R2-C4, R2-C5, R2-C7, R2-C8: fixed.
- R2-C6: fixed as a stated exception, but untested (R3-S13).
- R2-CN1–CN3: fixed.

### R3-S1 — High: `cluster-admin` can fill `sys-00` and lock the security administrator out of revocation and the receiver key

`tenancy.activated` and `authorization.put` go to `sys-00` (contracts:115). `authorization.put`
follows the "any other event" rule, `present + reserved + 1 <= 8192` (contracts:141), so
PutAuthorization is refused, with nothing written, once `sys-00` is full. `cluster-admin` writes
`sys-<SHA-256(namespace UID)[0] & 15>` with every `policy.put` (contracts:115–116). The service
cannot check that a namespace UID exists, and repeated puts on one UID each add an event. So a
compromised or scripted `cluster-admin` fills `sys-00` with about 8192 PutTenantPolicy calls.
After that, `security-admin` can neither:
- revoke that `cluster-admin`'s binding, which is the separation the plan sets out to give
  (design:221, "`cluster-admin` [must not] change a binding or the receiver key"); nor
- set the receiver key, which may be absent at activation (contracts:66) and is the only route to
  PruneAudit (design:150).
With no key, `sys-00` can never be pruned, so the lock is permanent and only direct etcd access
undoes it. Ordinary use reaches the same state: over the store's life, 1/16 of policy,
assignment, system-deletion and forced-confirmation events land in `sys-00`. No test covers it.

Fix:
- Give `tenancy.activated` and `authorization.put` their own chain (e.g. `sec`) that only
  `security-admin` writes.
- Always admit `authorization.put`, as `audit.pruned` is (its only writer is the security
  administrator), or keep reserved headroom for it.
- Test: fill `sys-00` with `policy.put` events from `cluster-admin`; PutAuthorization then still
  revokes that binding and sets a receiver key, and a prune of `sys-00` follows.

### R3-S2 — Medium: nothing bounds the total audit space, so tenants within their allowances can bring on etcd's space alarm

Each policy adds up to 65536 events × 2 KiB = 128 MiB (contracts:88, design:214–215). Nothing
limits:
- the number of policies;
- the sum of their allowances;
- the events of `ns-` chains whose policy is gone, which are never pruned without a receiver
  (contracts:145–147).
Every new namespace UID brings a fresh lifetime budget. So S9's "the budgets bound the store's
audited operations for its life" (spec:124) is false under namespace churn, for example CI
namespaces that each get a policy.

The runbook's "etcd quota of at least twice that" (design:216) is checked once, but
`allocated_events` grows with every PutTenantPolicy. etcd's practical ceiling is 8 GiB. That is
about 30 namespaces at 65536, or about 250 at 8192. Past it, tenants that each stay inside their
own allowance together trigger the space alarm, which refuses every write, fencing included
(design:218). That contradicts the invariant "No tenant's activity can refuse another
namespace's … operation" (spec:38).

The formula also undercounts a chain whose allowance was lowered below what it already holds:
it should use `max(cap, present + reserved)`.

Fix:
- Keep a store-wide `audit/allocated` record, updated in PutTenantPolicy's and
  DeleteTenantPolicy's transactions (and compared there).
- Compare it with a total set by `security-admin` in the authorization record.
- Refuse a policy that would exceed the total (`ResourceExhausted`).
- Count a gone policy's chain at `present` until it is pruned.
- Tests: a policy past the total is refused; delete-then-recreate cycles across new UIDs stop
  at the total.

### R3-S3 — Medium: the CSI controller's Attach now depends on the authorization record, but Risks says it reads no tenant record

Once active, Attach by `csi-controller` needs a role, which comes from the record "read on every
call that needs one" (contracts:37, 52–54; design:44–48). Only the agent's `only_if_target`
attach skips the read. Yet Risks still says "Attach and ChangeIsr read no tenant record … so a
full chain or a receiver outage cannot block failover" (design:211–212). ControllerPublish for a
pod moved off a lost node is a CSI Attach. It now fails in two cases:
- the record does not parse or has another schema (`Internal`, contracts:82–83), the same on
  every instance; or
- a `security-admin` PutAuthorization drops or mistypes the `csi-controller` ID. Revocation is
  immediate on every instance, so this one write stops all creations, deletions and attaches at
  once.
Validation requires a `security-admin` binding but not a `csi-controller` one (contracts:71–72),
and no test covers Attach with a record that lacks the binding or does not parse.

Fix:
- Correct the Risks bullet: CSI Attach depends on the record.
- Refuse an authorization record whose bound `csi-controller` set is empty, or have the CLI
  print the diff and require `--drop-csi-controller` before removing that binding.
- Tests: Attach by the CSI identity with a record that does not parse, and with one whose
  `csi-controller` binding was removed; the agent's `only_if_target` attach still commits in
  both cases.

### R3-S4 — Medium: nothing keeps a usable `security-admin`, and the record can never be deleted

ActivateTenancy and PutAuthorization check only that some `security-admin` binding exists
(contracts:71–72). If that binding is an ID no certificate holds (a typo in an exact SPIFFE ID),
or a PutAuthorization hands it to such an ID, no binding and no receiver key can ever change
again. Activation is permanent and nothing deletes the record (contracts:66–68), so only direct
etcd access recovers. The receiver key then stays unset or frozen, so nothing is ever pruned.

Fix:
- ActivateTenancy requires the caller's own ID to be bound to `security-admin` in the record it
  writes.
- PutAuthorization refuses a record that drops the caller's own `security-admin` binding, so a
  handover takes two puts: add the new ID, then the new ID removes the old.
- Tests: both refusals, and a two-step handover.

### R3-S5 — Low: an authorization record missing after activation falls back to the administrator-name rule

`active(ctx)` caches true "(it is never deleted)" (design:39–40). Authorize, however, reads the
record on each call and, when it is absent, applies the administrator-name rule to existing
methods and to ActivateTenancy (design:45–46). If the record disappears (an etcd-level delete,
or a partial restore), every instance fails open at its next call, and any administrator name
can activate again with bindings of its choosing. Owner, policy and `audit/` records still show
that the store was active.

Fix:
- On an instance that has cached active, an absent record is `Internal` for role-gated calls,
  and ActivateTenancy is refused.
- At start, `serve` treats a store holding `audit/tail/` but no record as active-and-broken.
- Test both.

### R3-S6 — Low: "a binding removed is refused at once" does not cover calls already authorized

Authorize reads the roles before the handler runs. An override or a policy write authorized
just before PutAuthorization revokes its binding still commits after the revocation, because no
audited transaction compares the record's revision (only PruneAudit does, design:153–154).
contracts:54 and spec:48–49 say "at once"; design:41 says "at its next call".

Fix: state "at the caller's next call", or have audited transactions compare the authorization
revision that Authorize read. Test it with `interleaveKV`.

### R3-S7 — Low: a refused role-gated call now costs a store read, for node identities too

Authorize "otherwise read[s] the record" for any identity whose node, agent or target rule does
not allow the call (design:44–48). Today, a node identity calling DeleteVolume or
PutTenantPolicy is refused without touching the store (`meta/service/auth.go:62–85`). Under the
plan, every node certificate can drive one linearizable etcd Get per refused call. That weakens
S7's "no caller can … delay a mutation by being refused" (spec:102).

Fix: bindings hold administrator IDs only (contracts:69–70), so refuse a non-administrator
identity before reading the record. Add the flood test with a counting store that shows no
read.

### R3-S8 — Low: suppressed refusal counts are lost when the table is cleared

The suppressed count is logged "before that caller's next logged line", and the table "is
cleared each second" (contracts:217–219). A caller that bursts past 10 refusals and then stops
never has its count logged, so the burst shows as 10 lines.

Fix: when the table is cleared, log every nonzero count (at most 1025 lines). Test a burst
followed by silence.

### R3-S9 — Low: AssignOwner `system` takes a tenant volume id, which a system creation may not

A system creation "cannot take a tenant volume id" (spec S3; contracts:196). AssignOwner
`system`, though, "takes any such header", parsed and published or still creating (design:131–
132). `cluster-admin` can therefore make a legacy `pvc-<UID>` volume, or an in-flight
activation-window creation, into a system volume: uncharged and unprotected, deletable by
`system-provisioner`, while the PVC's own DeleteVolume is refused `wrong-kind` forever. That is
the outcome R2-S6 closed for creations.

Fix: allow `system` on a tenant-form id only when the header does not parse. Test that a
parsed `pvc-` header is refused `system`.

### R3-S10 — Low: repeated or non-canonical binding IDs are not refused

The proto carries `repeated Binding{id, roles}` (contracts:173–174), and the stored form is a
JSON object (contracts:65). Validation lists repeated roles but not repeated IDs
(contracts:68–72). Two `Binding`s for one ID, each checked on its own, can give that ID
`deletion-override` and `cluster-admin`; and for a JSON file, Go's decoder silently keeps the
last duplicate key. `pki.ParseID` also accepts non-canonical spellings: it decodes the path, so
`admin/%63si` parses as `csi`. Such an ID never matches an exact string compare, and would
bypass the "bound alone" check if matching ever compared parsed identities.

Fix:
- Refuse a repeated ID, and an ID that differs from `pki.ParseID(id).String()`.
- In the CLI, decode the file with duplicate-key detection.
- Add these to `roles_test.go`.

### R3-S11 — Low: "still being created" is the `Creating` flag alone, which #76 calls a weakened classifier

Protection is bypassed for a header that "parses and is still creating" (design:113–114;
spec:81–83). #76's approved spec allows its unpublished rollback only on proof: the `Creating`
flag, epoch 0, no target and a recognized writer. It names "Creating alone" as the
`weakened_unpublished_classifier` that its tests must catch, and keeps other `Creating` shapes
on conservative deletion (#76 spec:22–25, 82–87, 121–127). A decodable header with `Creating`
set but a nonzero epoch or a target therefore passes #73's protection check while #76 refuses
to treat it as unpublished. The design also routes such headers "after this plan's checks"
(design:118–120).

Fix: treat a header as "being created" only when `Creating` is set, `epoch` and `first_epoch`
are 0 and `target` is empty; any other `Creating` shape counts as published. Test a protected
`Creating` header with epoch > 0: deletion is refused.

### R3-S12 — Low: a pending sweep cannot finish once the receiver key is removed

PruneAudit checks the key first and only then finishes a pending sweep (design:150–151), and
`receiver_key` may be removed by a PutAuthorization (contracts:66). If the key is removed, or
the record changes, after a prune commits and before its sweep ends, the events from `from` to
`through` stay in etcd for good. They are counted in neither `present` nor `allocated_events`.

Fix: finish a pending sweep before any key check, since it needs no key. Extend the
crash-mid-sweep test so the key is removed before the retry.

### R3-S13 — Low: the activation-window exception has no test

S11 states that a creation, move, drain or attach authorized just before activation may commit
after it, and that a volume so created is unassigned (spec:136–138; design:57). The S11 tests
cover only the deletion race (spec S11 tests; design:192). Nothing checks that such a volume
gets no owner record and no charge, is reported in `unassigned`, and can be deleted only after
assignment (or, while still being created, as S5 allows).

Fix: an `interleaveKV` case that activates between a creation's authorization and its claim,
asserting those outcomes.

## Nits

- contracts.md:1 ("contracts — draft") and :4 ("Everything here is Proposed"), and design:230–231
  ("Q1–Q4 stay open; no risk is accepted"), contradict decisions.md "Settled at approval"
  (contracts settled as written; Q1, Q2 and Q4 settled). Say which parts stay open: Q3's evidence.

Reviewed commit: 7a911860ee5d0c0f3a181ba6e1594606b2ceb34d
