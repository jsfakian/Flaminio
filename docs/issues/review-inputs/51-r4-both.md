# #51 plan review, round 4 (incremental, 4d45c35..30e0643), design-code and design-security

Scope: `git diff 4d45c35..30e0643 -- docs/issues/51/ ':!docs/issues/51/report.md'`, in
/home/jsfakian/Documents/src/flaminio-51.

## Earlier findings
- R3-C1: fixed. contracts.md:15-17 and design.md:22-24 make `snapshot_charge_next` a `*uint32` with `omitempty`. With encoding/json, a nil pointer is omitted, a pointer to 0 is written as `0`, and both decode back to the same value. The header is decoded with plain `json.Unmarshal`, with no `DisallowUnknownFields` anywhere (`meta/service/service.go:282,539`). The service compares headers by revision, never by struct equality, so a pointer field does no harm there. No Rust or script code reads the header JSON: the data plane reads the proto, and `meta/testdata/isr-vectors.jsonl` has its own `after` schema. design.md:237 has every instance upgraded before the first request, so old decoders do not matter. The tests are named (design.md:248).
- R3-B2: fixed. Once #73 is active, the mark compares `owners/<id>` (absence included) and `tenant/authorization`, as #70's step does (#70 design:96-98). AssignOwner is refused before activation (#73 contracts:49,319), and activation rewrites the authorization record, so dropping the owner compare under `inactiveRules` is sound. The counts hold: none before #73 lands; 3 compares under `inactiveRules`; 4 for an unassigned or system volume; 6 for a tenant. Writes are 2 or 4. The mark carries no audit event, so #73's tail compare does not apply. The worst case is 10 operations, well under 120. The `interleaveKV` hook exists (`meta/service/service_test.go:615-646`) and fires once, after the first Get of the key. Without the new compare, the test fails: AssignOwner does not write the header, so the mark commits uncharged.
- R3-C3: fixed. contracts.md:88-91 now gives the batch outcome: `FailedPrecondition` naming the node, with earlier batches kept. The case is named in S2 and in the risk, and tested at design.md:248. #72 defines a marked ledger as unreadable (#72 capacity-contracts:8-12), as the text assumes.
- Nit (refusal vs completion): fixed. contracts.md:59-63 completes the snapshot only after the request's checks admit it. The narrowed S2 wording is necessary. The old wording ("completed by the volume's next request", with "refused, before any write") cannot hold for a next request with an invalid name: that request is of the volume, is refused, and may write nothing. Whatever order the checks run in, a refused request cannot complete the charge, so "next request" had to go. Nothing in intent.md speaks to completion.
- Nit (S24): fixed (design.md:255).

Checked and sound, no finding:
- **The request naming the `CHARGING` snapshot ("checked as an answer only").** Authorize still runs at entry, and `PermissionDenied` comes only from it (contracts.md:118). The method's role (administrator name, then `cluster-admin`) is per method, not per volume, so skipping the body's checks bypasses no authorization. The lifecycle (unknown, creating, deleting) and `expected_first_epoch` are still checked. The checks it skips either ran at the mark or are rechecked:
  - the encryption refusal: #50's mark and its descriptor are set at creation (#50 design:49-64);
  - hydrating: a creation-time state;
  - the 63 count and `B_s`: they already count the snapshot;
  - room: each batch rechecks it at `SoftPPM`;
  - quota: charged at the mark, or by AssignOwner at (1 + count).

  Completing the charge creates no new snapshot, so the request cannot bypass anything.

## Nits
- design.md:292-293: the round-2 risk still says the Charging snapshot waits "until the volume's next request completes it". S2 (spec.md:56) and contracts.md:91 now say "a later request ... that is not refused"; use that wording here.
- design.md:294-295, decisions.md:209: "until ... `ForgetNode` removes its copy" holds only for a released copy. ForgetNode removes released copies and fence holds, not replicas (#13 design:162-180). A down node's replica must first be moved off: say so.

Reviewed commit: 50df1c3e96b61b5d91ed74aacb609c39d65787d0
