# #73 plan review, round 11 (incremental; design-code and design-security)

Scope: `git diff cc1ca57..39da093 -- docs/issues/73/ ':!docs/issues/73/report.md'`. HEAD `d01d39a` adds only report.md. Checked against contracts "Identifiers" (contracts.md:10–24), "Activation and the authorization record" (:83–126), "Records" (:133–157), "Budget" (:207–264), "Protobuf" (:266–307), "CLI" (:338–364); design CreateVolume (design.md:88–97), PruneAudit (:193–205), AssignOwner (:173–178); `meta/kv/kv.go:18–33`, `meta/kv/etcd.go:90–97`; `meta/cmd/flaminio-meta/main.go:185–203`.

Checked and sound: every key and field the repair names exists (`ChainStatus.next/pruned/cap`, `audit/tail/cfg`, `audit/sweep/cfg`, `authorization`). The txn has 3 compares and 1 put, well inside etcd's limit and `kv.MaxTxnOps`. A mod-revision compare at 0 tests absence on both stores. `next − 1 − pruned = cap + 1024` is the exact "no key can be set" condition once no sweep is pending: a `cfg` prune ends with at most `cap + 1024` present, and `cfg` takes no reservations. A `default` limit of 0 conflicts with no admission rule. CreateVolume refuses it as over quota (0 + 1 > 0). AssignOwner needs only a limit to be present and charges past it. Growth checks `max_bytes` as before. `parseSize("0")` accepts the CLI's `--class default=0:N`.

## Earlier findings

- R10-B1: fixed as raised (precondition in `ChainStatus`'s fields, pending sweep finished first, txn compares the tail and the sweep's absence, claim reworded). It opens R11-B1 and R11-B2.
- R10-B2: fixed as raised (a `default` limit of 0, class limits may be 0, `max_audit_events` sizing, no PVC rights, UID logged, S11 assigns into it and refuses a creation).
- R10-N1: fixed (contracts.md:153).
- R10-N2: fixed (contracts.md:123–124). The design's usage comment drops the fingerprint (Nit 2).

### R11-B1 — Low: the precondition is checked on one read, and the txn compares revisions from another

**Summary.** The operator requires `next − 1 − pruned = cap + 1024` "in `cfg`'s `ChainStatus`". The txn then "compares the record's and `audit/tail/cfg`'s mod revisions as read" (contracts.md:117–121).
- `ChainStatus` carries no revision (contracts.md:289–291). The tail's mod revision therefore comes from a separate `etcdctl` read, and the record's kept fields come from etcd too, since `changed_by` is not in the `Authorization` message (contracts.md:287–288).
- Suppose `cfg` gets room between the `ChainStatus` read and the `etcdctl` read. A prune by a party that still holds the key could do this. The txn then compares the new revision and commits. That is an unaudited key change made while the audited path was open, which is the case R10-B1 closed.
- So "with `cfg` given room … meanwhile, the txn fails" (contracts.md:121–122) holds only from the `etcdctl` read on.

**Fix.**
- In contracts.md:115–122 and the usage comment's runbook (design.md:260–262), read `audit/tail/cfg`, `audit/sweep/cfg` and the record with `etcdctl get -w json`.
- Take the later reads at the first response's header revision (`--rev`).
- Check `next − 1 − pruned` on that tail value, build the new record from that record value, and compare those reads' `mod_revision`s.
- Keep `ChainStatus` as a cross-check only, or read it after the `etcdctl` reads.

### R11-B2 — Low: the new repair test cases do not isolate the compares

**Summary.** The case is "the repair's txn refused while `cfg` has room or a sweep is pending" (design.md:242; spec.md:239).
- **"While `cfg` has room":** the txn checks no count. Built from reads taken at a state with room, it commits, so the case as worded cannot pass. It can be refused only when room comes after the reads, and then by the tail compare.
- **"A sweep is pending":** a prune's first transaction also writes `cfg`'s tail, because it appends `audit.pruned` to `cfg` (design.md:199–201). A sweep that starts after the reads is therefore refused by the tail compare, and the case still passes with the sweep compare dropped.
- **The record compare:** decisions.md:196–198 says it catches a no-op PutAuthorization, which still commits at `cap + 1024` (contracts.md:118–119). No case puts one between the reads and the txn. "A PutAuthorization at the revision from before the repair refused as stale" tests the service's compare, not the repair's. Dropping the record compare fails no test.

**Fix.** In design S7–S9 and spec S7–S9, name three cases, each refused by one compare alone:
1. Reads at `cap + 1024`, then a prune of `cfg` (by a key holder) ends before the txn: refused by the tail compare alone.
2. A no-op PutAuthorization after the reads: refused by the record compare alone.
3. The txn built from reads taken while a sweep is pending: refused by the sweep compare alone, because it compares absence, not the revision as read.

Drop "while `cfg` has room": the precondition, not the txn, refuses that state.

## Nits

1. **The sweep-finishing PruneAudit "needs no key", but it still needs well-formed arguments** (contracts.md:116–117).
   - The chain hash and receipt must be exactly 32 and 64 bytes. They are checked before any record is read (contracts.md:10–11, 22).
   - `audit prune` takes both positionally (contracts.md:355).
   - A runbook that sends empty values gets InvalidArgument, and the sweep is never finished.
   - Fix: "with any 32-byte hash and 64-byte receipt, e.g. zeros".
2. **The usage comment's operator log holds "who repaired and when"** (design.md:262).
   - contracts.md:123–124 and decisions.md:203–205 also require the new key's fingerprint.
   - The closing reference "(contracts, "Activation and the authorization record")" now follows the holding-namespace clause, which that section does not cover (spec Q3 does).
   - Fix: add the fingerprint, and move the reference before the holding-namespace clause or point that clause to spec Q3.

Reviewed commit: d01d39af016b11c6c484f592705075e24894c7b8
