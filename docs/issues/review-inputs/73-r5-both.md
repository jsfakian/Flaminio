# #73 plan review, round 5 (incremental; design-code and design-security)

Scope: `git diff 98c1242..f262e1f -- docs/issues/73/ ':!docs/issues/73/report.md'`, checked against
`meta/service/{auth,service}.go`, `meta/kv/{kv,etcd}.go`, `pkg/pki/pki.go`, `scripts/lab.sh`,
`control/csi/controller.go`, `control/cmd/flaminio-csi/main.go` and #76's design.

## Earlier findings

- R4-C1 (policy never deletable after a prune): fixed (an `ns-` chain's prune event goes to `sys-`; the chain is retired with its policy).
- R4-C2 (#76 interface): partly fixed. The order and the return in DeleteVolume are stated. #76's paths outside DeleteVolume are not (R5-C5).
- R4-C3 (log line, refusal table untested): fixed.
- R4-C4 (missing record, no recovery): fixed.
- R4-C5 (post-approval text labelled approved): fixed.
- R4-CN1, R4-CN2: fixed.
- R4-S1 (configuration changes unaudited): fixed (`cfg` chain, full record logged). The overdraft does not deliver the property it claims (R5-B1).
- R4-S2 (retiring chains): fixed.
- R4-S3 (any administrator name can Attach): fixed. The replacement rule opens R5-C2 and R5-S1.
- R4-S4 (`serve` exits on incomplete records): fixed.
- R4-S5 (#76 order and return): fixed for DeleteVolume. Residual in R5-C5.
- R4-S6 (Attach before the insecure rule): fixed. Read ordering: R5-S2.
- R4-S7 (pending sweep left out of the bound): fixed. The new last sweep transaction breaks the op limit (R5-C1).
- R4-SN1, R4-SN2, R4-SN3: fixed.

Checked, no finding:
- Every other changed transaction stays within `kv.MaxTxnOps` = 120, which limits compares and ops separately (`meta/kv/etcd.go:91`):
  - ActivateTenancy: at most 68 ops, 3 compares;
  - PutAuthorization: at most 67 ops;
  - PutTenantPolicy: 4/4;
  - DeleteTenantPolicy: about 9 compares, 5 ops;
  - the first prune transaction, the mark and `finishDeletion`.
- "Grants nothing new" is decidable: compare role sets per ID. Only `security-admin` can make a recovery write.
- The space sum 16 × 8193 + 9217 + 524288 holds.
- A full `sys-` chain is always prunable.

### R5-B1 — High: the `cfg` overdraft does not guarantee that a full `cfg` can be pruned

Spec S9 (spec.md:141–143) claims that the overdraft means "a full configuration chain can always be pruned". It does not:
- Pruning needs `cluster-admin` (contracts.md:52).
- On a full `cfg`, granting a role is refused (contracts.md:197–200), and that includes binding a new `cluster-admin`.
- Revoking a role is a recovery write, so it is admitted (contracts.md:98–99, 194–195).

So if the only `cluster-admin` binding is revoked while `cfg` is full, nothing can ever prune `cfg` or grant a role again. That freezes, for the life of the store:
- every grant, security-admin handovers included;
- every policy write and deletion, so new namespaces get no policy and their creations are refused.

The only remedy is etcd surgery.

A compromised `cluster-admin` reaches this state on purpose:
- it fills `cfg` in 8192 PutTenantPolicy calls (each write, a no-op included, appends `policy.put`);
- the security administrator's natural response, revoking that `cluster-admin`, then makes the freeze permanent;
- the security administrator cannot bind a replacement first, because that is a grant.

This is R3-S1's lock-out again, moved from revocations to grants and pruning. No test covers it: the `cfg`-full row in design "Tests" S7–S9 assumes a `cluster-admin` exists.

Fix, any one of:
- let `security-admin` call PruneAudit for `cfg`;
- admit, inside the overdraft, a grant of `cluster-admin` when no binding holds it;
- put authorization events in a `sec` chain that only `security-admin` writes, apart from policy events.

Test: fill `cfg` and revoke the sole `cluster-admin`; a prune or a re-grant must then succeed. Also test that a `cluster-admin` flood of policy writes cannot stop the security administrator from binding a replacement.

### R5-C1 — High: the sweep's last transaction can exceed etcd's 120-op limit, and its tail write is not conditional

Design "PruneAudit" (design.md:176–179) says:
- the sweep deletes event keys in batches of 119;
- "the last deletes the sweep key and sets `pruned = through` in the chain's tail".

When the last batch is full, that is 119 deletes + 1 sweep-key delete + 1 tail put = 121 ops. `Etcd.TxnRev` refuses more than 120 ops (`meta/kv/etcd.go:91–92`), and the in-memory backend does not check the limit, so `backends` tests pass unless a prune covers a multiple of 119 events on etcd.

The failure is deterministic and permanent:
- every PruneAudit of that chain first "finishes a pending sweep" and fails the same way;
- `pruned` never rises, so the chain stays full for life;
- for an `ns-` chain, its namespace's deletions and its policy's deletion are refused;
- for `cfg`, grants and policy writes are refused;
- for a `sys-` chain, its deletions are refused.

The tail write also changes `pruned` in a record that concurrent appends rewrite (`next`, `hash`, `reserved`). The design does not say the tail's revision is compared. A blind write would lose an append and break the chain (Principles: conditional transactions).

Fix:
- Make the batch at most 118, or write the tail in a separate final transaction that compares the sweep key and the tail.
- State the tail compare.
- Test on etcd: a prune of exactly 119 × n events; an append racing the last sweep transaction.

### R5-C2 — Medium: Attach's fixed CSI name is not tied to the `csi-controller` binding

Once active, Attach admits only `spiffe://<td>/flaminio/admin/csi-controller`, "as the lab issues it" (contracts.md:24, 35, 58–64). But the `csi-controller` role binds whatever ID the record names (contracts.md:54–57).

The code fixes no such name:
- `control/cmd/flaminio-csi/main.go:7–8` documents the controller as `.../flaminio/admin/<name>`;
- `deploy/csi/driver.yaml` only mounts a Secret;
- only `scripts/lab.sh:1004` uses `csi-controller`.

Two failures follow:
- A deployment that bound `csi-controller` to another name activates cleanly, keeps CreateVolume and DeleteVolume, and then fails every ControllerPublish with PermissionDenied. No pod can mount a volume.
- Any certificate the CA issues for `admin/csi-controller` attaches whatever the record says.

Neither ActivateTenancy, PutAuthorization nor the upgrade order checks or mentions this. No test covers it.

Fix:
- Refuse a record that binds `csi-controller` to any ID other than the fixed one (or that leaves the fixed ID unbound).
- State the fixed name as a deployment requirement in the upgrade order.
- Test: activation with `csi-controller` bound to `admin/csi` is refused.

### R5-S1 — Medium: once active, an administrative or system-volume Attach needs the CSI controller's key

Once active, Attach admits only the CSI controller's name and agents restarting themselves (spec S1; contracts.md:58–64). Consequences:
- `system-provisioner` can create system volumes that only the CSI controller's certificate can attach.
- Every manual attach needs that certificate, including:
  - the lab's volume workflow (`scripts/lab.sh:715`);
  - the metadata check (`:624`, `:629`);
  - the manual failover under partition (`:824`, run from fl-w).

The plan's remedy is to use the CSI controller's certificate (contracts.md:61–63; design.md:70–71). That certificate also carries `csi-controller`: tenant creations and deletions, and the power to state any owner. Operators would have to copy the broadest tenant-facing credential to admin hosts to do an Attach. That is a least-privilege regression from the approved row (`csi-controller`, `system-provisioner`, `cluster-admin`).

Fix:
- Keep the record-free path for the fixed CSI name and agents.
- For any other administrator, read the record and admit `cluster-admin` (and `system-provisioner` for system volumes).
- Failover still never depends on the record.
- Test: `cluster-admin` attaches once active; `audit-reader` and `deletion-override` are refused.

### R5-C3 — Low: an `ns-` chain's prune can now be refused, against S9, and the case is untested

Contracts.md:192–193 admits an `ns-` chain's `audit.pruned` "as any other event in its `sys-` chain; a full one is pruned first". So PruneAudit of a namespace chain is refused (ResourceExhausted) while its `sys-` chain is full. Spec S9 still says pruning is never refused at a budget (spec.md:137–139).

Prune events also now share `sys-` budget with system and override deletions:
- an exporter that prunes small namespace chains after each deletion turns tenant deletions into `sys-` events;
- those events can refuse an administrator's deletions hashed to the same chain until it is pruned.

The test row checks only that the event lands in `sys-`.

Fix:
- Reword S9 to except this case.
- Add a test: a full `sys-` chain refuses an `ns-` prune, then admits it after that `sys-` chain is pruned.

### R5-C4 — Low: the overdraft's bound and the recovery-write classifier are untested

The `cfg` row of design "Tests" S7–S9 admits a revocation and a key change in the overdraft. It never shows either of these:
- a recovery write is refused past `cap + 1024`;
- a write that revokes one role and grants another, or replaces an ID, is classed as a grant and refused on a full `cfg`.

Repeated receiver-key changes are recovery writes with no other limit, so the 1024 bound is the only cap on them.

Fix: add both cases, and a key change admitted at exactly `cap + 1024`.

### R5-S2 — Low: Attach reads `active` before a non-administrator is refused

Design "Authorize, in order" (design.md:44–49) evaluates the Attach step before "an identity that is not an administrator is refused … with no store read". `active` is not cached while false, so on every inactive store, a node identity's refused Attach costs two etcd reads. This contradicts contracts.md:65–67 and the counting-store test row.

Fix: apply the agent rule and the non-administrator refusal before reading `active`. Both branches decide such callers alike.

### R5-C5 — Low: #76's rollbacks outside DeleteVolume are still unstated (R4-C2 residual)

The plan states #76 only inside DeleteVolume (design.md:132–139). But #76 also:
- resumes "Creating+Deleting" headers from create/recovery;
- performs legacy automatic rollback outside DeleteVolume (#76 design.md:64–67, 80–84, 96).

#73's createVolume "finishes only a header already marked … with `finishDeletion`" (design.md:140–143), and does not say what happens once #76 deletes such a header alone. If #76 leaves an owner record with a `deleting_op` and no header, #73 calls it an orphan (FailedPrecondition, which only the override clears).

#76's plan still does not mention #73.

Fix:
- State that createVolume releases a headerless owner record with a `deleting_op`, as DeleteVolume does, or that #76 may run its rollback only through DeleteVolume.
- Add the follow-up row the report's "Needs other plans" names.

## Nits

1. Contracts.md:213 "an 80-byte key". `/flaminio/v1/tenant/audit/events/ns-<UID>-<k>/<20 digits>` is up to 114 bytes, giving about 1.33 GiB of keys and values. State the real key size.
2. Contracts.md:21 "`k` 1–20 decimal digits" allows leading zeros and values above 2^64−1, so one chain has several names. Require the canonical decimal of a u64 ≥ 1.
3. Design.md:41 says "only Attach and the insecure rule use [`active`]", while design.md:62–64's race loop also checks `active` before each attempt.
4. Design.md:120–122: the no-header branch now covers only "an older binary" and ends "Then NotFound". The #76 bullet (design.md:132–137) routes a header-less owner record with a `deleting_op` through the same loop and says the call "answers success". Name the branch and the code.
5. Contracts.md:86 repairs "the allocation summed from the policies", which cannot restore `chains`; a lower `chains` reuses chain names. Contracts.md:216–219 promises a raised allowance with no receiver key, which a full `cfg` refuses.

Reviewed commit: c69830e887cb703a5c47994724278f68c3ec8d54
