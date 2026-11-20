# #73 plan review, round 7 (incremental; design-code and design-security)

Scope: `git diff e2e87a7..7fcbeac -- docs/issues/73/ ':!docs/issues/73/report.md'` (HEAD `c80ae2f` adds only report.md): the round-6 fix (1fce07e) and the settle commit (7fcbeac). Checked against `meta/service/service.go` (claim `:413–433`, DeleteVolume `:516–577`, readHeader `:275–290`), `meta/kv/{kv,etcd}.go` (120 compares and 120 ops, `etcd.go:91–92`), `control/csi/controller.go`, and #70's design (step 3, "Charges").

## Earlier findings

- R6-B1: fixed. `ChainStatus.hash` is the tail's hash at `next − 1`, and TenantStatus is a `security-admin` call. The no-`audit-reader` test fails without the field.
- R6-S1: mostly fixed. Only a full `cfg` can be pruned this way; the residual is stated and the settle commit accepts it; the non-full refusal is tested. Still open: Q2 is unmarked and the ADR's Decision still says "independent" (R7-S1).
- R6-C1: fixed. The same-owner or `deleting_op` leftover is released before the quota check, and the release compares the header absent. The claim compares `owners/<id>` absent, so no path writes a header while the record exists, and a live volume's record cannot be released. Another owner's record without a `deleting_op` stays `orphan`. Tested at the namespace's limit. Older text left behind: R7-C3.
- R6-C2: fixed. Covered by contracts "Growth" and "Records", the design's Growth bullet, `expand.go` and `TestExpandChargesQuota`. This agrees with #70's step 3 and "Charges", whose 120-op and 120-compare count already includes #73's two writes and four compares.
- R6-C3: fixed as raised (a key removal is refused on a full `cfg`, and tested). The "always" claim still fails through key replacements, and the invariant contradicts the fix (R7-B1).
- R6-C4: fixed (contracts Errors; design PruneAudit order; tested with a `sys-` and an `ns-` chain). Test wording: Nit 3.
- R6-N1: fixed.
- R6-N2: fixed. Each `chains` increment is its own transaction, so the revision bounds every `k`. A 20-digit `k` still fits the 114-byte key.

### R7-C1 — Medium: the settled Q3 answer sends a `pvc-` volume with no PVC to `system` or the override, and the contracts refuse both

The settle commit records the evidence for a legacy owner: "the PVC whose UID the volume id carries … with no such PVC, `system`, or the override deletes it". This text is in decisions.md:37–40 and 57–62, spec.md:265–267 and design.md:295–297.

For the main legacy case, the plan refuses both remedies:
- Every CSI-created volume is named `pvc-<UUID>` (`control/csi/controller.go:232`, `Id: req.GetName()`).
- `system` is refused for "a tenant-form id whose header parses" (contracts.md:298; design.md:172–174; spec S11, spec.md:180–181: "a parsed `pvc-<UUID>` volume only to a tenant").
- The override is refused for "`OverrideDelete` of an unassigned volume" (contracts.md:298; design.md:166). S6 also says the override deletes an *assigned* volume.

A realistic case has no path: a Retain PV whose PVC was deleted before activation. It stays unassigned, so DeleteVolume refuses it. The one path the contracts admit goes unnamed: a tenant assignment to a namespace that has a policy and a `default` limit.

Fix:
- Restate the answer. With no such PVC, assign the tenant: the namespace from the PV's `claimRef` (its UID read from the API) and the PVC UID the id carries. With that namespace gone, use a namespace the administrator designates, with its own policy. Then the override or DeleteVolume deletes the volume.
- Keep `system` for non-tenant ids and headers that do not parse, as S11 says.
- Make the change in decisions.md (both entries), spec Q3 and design Risks.

### R7-B1 — Low: `cfg` can still freeze through key replacements, and the invariant contradicts the round-6 fix

1. **Freeze through key replacements.** A key replacement is a recovery write, and the number of them is unbounded (contracts.md:107–110, 206–207). Revocations alone cannot use up the 1024-event overdraft: on a full `cfg` grants are refused, so at most 64 bindings only shrink. Key replacements can. Once the overdraft is spent, every write is refused. If the last key set is one whose private half nobody will sign with, nothing can prune `cfg`, so it is frozen for life. Examples: a key set by mistake, a decommissioned receiver, or a compromised `security-admin` or a script. This is R6-C3's state reached by replacement instead of removal. So these claims are false:
   - "`security-admin` can always make that prune" (contracts.md:209–213);
   - "always possible" (design.md:274–276);
   - "can always set the key" (decisions.md:158–160; spec S9, spec.md:153–156).
   No test covers it.
2. **The invariant still says "only past a fixed overdraft".** spec.md:44–46 reads "A revocation or a receiver-key change is refused for audit space only past a fixed overdraft". A key removal is now refused at `cap` (contracts.md:109–110). design.md:274 also lists "a receiver-key change" among the writes that are "always possible".

Fix, either:
- State the precondition wherever "always" appears: `security-admin` must keep the private half of the last key it sets in the overdraft. Test: the overdraft used up by key changes, then a prune with the last key admitted and the next key change refused.
- Or let `security-admin`'s PruneAudit of a full `cfg` carry the receiver key it verifies against. That call writes the key, with its `authorization.receiver` event, in the prune's first transaction, admitted as `audit.pruned` is. Then the overdraft never gates the escape.

In either case, reword spec.md:44–46 and design.md:274 to say that a removal counts as an ordinary write.

### R7-C2 — Low: a `security-admin` prune retried after a lost reply or a crash answers `not-full`, not `pruned`

The PruneAudit order (design.md:192–197) is:
1. finish a pending sweep;
2. for `security-admin`, `present < cap` → FailedPrecondition `not-full`;
3. … then "`through <= pruned` answers `pruned` (a replay or a lost reply)".

After a successful prune of `cfg` through `next − 1`, `present` is 1, the `audit.pruned` event. So `security-admin`'s retry fails at step 2:
- after a lost reply;
- after a crash between the first transaction and the sweep's end (step 1 finishes the sweep, then step 2 refuses).

`audit prune` exits 1 for a prune that was done. The replay rule holds only for `cluster-admin`. The replay tests (design.md:241, "replayed", "crashed mid-sweep") do not cover `security-admin`.

Fix: check `through <= pruned` before `not-full`, and test both retries by `security-admin`.

### R7-C3 — Low: two older passages still say only a `deleting_op` record is released by a creation

The round-6 rule releases a headerless record of the request's own owner too:
- design.md:88–92;
- spec S5, spec.md:106–109;
- contracts.md:298, where `orphan` means "of another owner and with no `deleting_op`".

Two unchanged passages contradict it:
- design.md:158–161: "with a `deleting_op`, a creation of that id release it, and only one without a `deleting_op` is an orphan";
- decisions.md:215–217 (round 5), which says the same.

An implementer following design.md:161 rebuilds R6-C1's stuck retry.

Fix: change both to "with a `deleting_op` or of the creation's own owner; only another owner's, without one, is an orphan". Or mark the round-5 decision as replaced by round 6.

### R7-S1 — Low: the R6-S1 fix leaves Q2 unmarked and the ADR's Decision still says the receiver is "independent"

- The ADR proposal's Decision still reads "Pruning needs an independent receiver's Ed25519 signature" (adr-proposal.md:45–46). Its own Consequences now state the self-prune exception (adr-proposal.md:80–83). The Decision is the text the numbered ADR will carry.
- Q2's approved answer still reads "pruning by hand: `cluster-admin`" (decisions.md:33–34; spec.md:257–259). Neither the round-6 fix nor the settle commit marks it as changed, although R6-S1's fix asked for that.

Fix: in the ADR Decision, add "…except a full configuration chain, which the security administrator may prune with a key it holds (Consequences)". Mark Q2 in decisions.md and spec Q2: `security-admin` also prunes a full `cfg` (round 6), settled 2026-11-13.

## Nits

1. design.md:113–114: "Inactive: today's claim, no owner record; an owner is refused." moved from the "Without an owner" bullet into the Growth bullet. It is about CreateVolume, not growth. Move it back.
2. contracts.md:139 defines `bytes` as "its size at the claim plus every growth step's". The table (contracts.md:132) gives a system owner 0 bytes, and an assigned legacy volume is charged its size at assignment. Say "a tenant's size at its claim or assignment plus every growth step's; 0 for `system`".
3. design.md:241 says "(PermissionDenied, nothing read)" for `security-admin` on a `sys-` or `ns-` chain. Authorize must read both activation records to learn the caller's role (design.md:50–54). Say "nothing read beyond Authorize's two records".
4. decisions.md:40, 179–180, 203–204: "(see decisions.md)" inside decisions.md points nowhere. Point to "Settled at the user's instruction of 2026-11-13".

Reviewed commit: c80ae2f67345a36bc9940d24d14ef12e77b5ff3f
