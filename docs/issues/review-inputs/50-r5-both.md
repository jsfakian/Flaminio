# Issue #50 plan review: round 5, design-code and design-security

Scope: `git diff e067381..6b65a96 -- docs/issues/50/ ':!docs/issues/50/report.md'` in
/home/jsfakian/Documents/src/flaminio-50. Checked against `scripts/lab.sh` and the clear plans
#73 and #76 on origin/main.

## Earlier findings
- R4-C1: fixed. With #73, the undo deletes the header and the chunks under the claim's compare.
  It then goes through #73's own headerless release, which compares the header (absent), the
  owner record, the tail and the quota (#73 design.md:136-141). That release retries in the
  method's loop (`retries = 32`, #73 design.md:186-187). Its event is `delete.committed`
  `header-missing` with no `deleting_op`, so it is admitted "as any other event", which matches
  #73 contracts.md:217-219. The full-chain case leaves the record to the id's next creation,
  which releases it and answers Aborted (#73 design.md:88-92). The race and the full chain are
  both tested.
- R4-C2: fixed. #76's finalization is now the publication. It writes the marked header and the
  key record in one transaction, run by the creator or a helper. This is in design.md "The
  header's mark", the "With #76" bullet and contracts.md. The header stays byte for byte at C
  until visibility (#76 design.md:49-55). `TestHeaderWritesKeepMark` checks a finalization by
  the creator and one by a helper.
- R4-B1: fixed. `pki issue` runs `pki init` when `flaminio/ca.crt` or `kek` is missing; today's
  line is correctly cited as `scripts/lab.sh:574`. `init` makes each piece only if it is missing
  (`:567-570`). The length check and the compare-only pass now run only on runs that reach a
  metadata VM, so `issue c` (`:1228`) does neither. A test case covers this.
- Nit 1: fixed. `ln`, then a compare, and the `mktemp` file is removed on every path. Two first
  installs of different KEKs cannot both succeed, and the test row covers the late `kek`.

## What the revision broke
Nothing found. Checked:
- the undo's header-compare failure, when a concurrent deletion marked the header (the deletion
  owns it, and the release's header-absent compare cannot pass under a marked header);
- `spec.md:174` "leaves the id free", which is #73's self-healing headerless record;
- that no stale `mv -f` or all-runs pre-check wording remains in the design, contracts or spec;
- that a hard link of the `0600` `mktemp` file keeps the `stat` root 0600 check true.

Reviewed commit: f80d7a665daa4ba340c51674122de83ada1636a8
