# #73 plan review, round 12 (incremental; design-code and design-security)

Scope: `git diff d01d39a..1cc9a08 -- docs/issues/73/ ':!docs/issues/73/report.md'`. HEAD `3b298c6` adds only report.md.

Checked against:
- contracts "Identifiers" (contracts.md:8–24), "Activation and the authorization record" (:83–129), "Records" (:135–148), "Budget" (:207–264), "Protobuf" (:266–307) and "CLI" (:341–356);
- design PutAuthorization (design.md:59–62), `appendEvents` (:181–187), PruneAudit (:193–205) and Docs (:254–265);
- spec S7–S9 tests (spec.md:227–253) and Q3 (:280–295);
- `meta/kv/kv.go:11–52`.

## Earlier findings

- R11-B1: fixed. The reads now come from `etcdctl get -w json`, at one revision through `--rev`. The precondition is checked on that tail, the record is built from that value, and the txn compares those reads' `mod_revision`s.
- R11-B2: fixed as raised. Three cases are named and "while `cfg` has room" is dropped. Cases 1 and 3 each isolate their compare. Case 2 does so only under a property the plan does not state (R12-B1).
- R11-N1: fixed (contracts.md:116–118).
- R11-N2: fixed (design.md:261–264).

Checked, no defect:
- **The `etcdctl` steps.** `get -w json` reports `header.revision` and each kv's `mod_revision` (the value is base64). `get --rev=<n>` returns each key and its `mod_revision` as of that revision. A key absent at that revision returns no kvs. In `etcdctl txn`, `mod("<key>") = "<n>"` is valid, and `mod("<key>") = "0"` or `version("<key>") = "0"` tests absence. A `--rev` read that hits a compaction fails safe, and the operator rereads.
- **The three cases can be built.**
  - Case 1: a cfg prune's first transaction compares the record but does not write it (design.md:200–201), and its last transaction deletes the sweep key. After the prune ends, only the tail has moved.
  - Case 3: start from cap + 1023, and let a prune crash after its first transaction (a failing store). The reads then show cap + 1024 and a sweep. Nothing moves the tail or the record after that, so only the sweep compare refuses. The case stands in for a skipped precondition: a sweep that starts after the reads rewrites `audit/tail/cfg` too, because `audit.pruned` of `cfg` goes to `cfg`. Not a defect.
- **The corrected round-10 entry (decisions.md:197–201) is accurate.** The tail and sweep compares keep the precondition true at commit, given the round-11 reads at one revision. The precondition, not the txn, refuses a `cfg` that has room.

### R12-B1 — Low: case 2 isolates the record compare only if a PutAuthorization with no event leaves `audit/tail/cfg` unwritten, and the plan does not say so

**Summary.** Case 2 is "a no-op PutAuthorization after the reads (the record compare)" (design.md:242, spec.md:239–241). Two passages in the plan bear on it:
- PutAuthorization "compares `expected_revision` and `cfg`'s tail, and writes the record and its events" (design.md:59–60).
- `appendEvents(tail, cap, events)` "returns the tail's revision compare and the writes" (design.md:183–184).

Neither says what `appendEvents` returns when the event list is empty. An implementation that always writes the tail back, even unchanged, moves its `mod_revision`. The no-op is then refused by the tail compare as well. With the record compare dropped, the case still passes, which is the gap R11-B2 closed. None of the cases says how "alone" is checked: each asserts only that the txn is refused.

**Fix.**
- In design.md:183–184, state that with no events `appendEvents` returns only the tail compare, and no tail write, so a no-op PutAuthorization leaves `audit/tail/cfg` unwritten.
- In design and spec S7–S9, have each case also assert that the other compares still hold:
  - case 1: the record's revision is unchanged and the sweep key is absent;
  - case 2: the tail's revision is unchanged and the sweep key is absent;
  - case 3: the tail's and the record's revisions are unchanged.

Reviewed commit: 3b298c6c1d5e0fb265a5c133d16207878a090717
