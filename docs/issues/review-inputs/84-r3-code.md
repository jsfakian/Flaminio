## Earlier findings
Round 2 had no code findings; the security findings (restarts, guest inputs, leftovers, fixtures) are addressed in the revision as far as the code focus sees, with the exceptions below.

### R3-C1 — Medium
A leftover record whose volume answers `NotFound` is cleared with only the guest `cleanup`, but NotFound does not mean cleanup finished. `delete_volume` (scripts/lab.sh:1733-1790) deletes in the metadata service first, then deletes replicas on every data node, then waits for the tombstone. A failure or timeout after the metadata delete (the likely one is the tombstone wait) leaves NotFound with replicas and a pending tombstone; the next run then clears the record, and the unfinished cleanup is lost. Evidence: kernel-check.md "An earlier record"; fixture "an earlier record whose volume answers NotFound"; fixture "`delete_volume` fails ... the next run is `refused`/`leftover`", which only holds if the failure came before the metadata delete.
Fix: clear a record only after `delete_volume <id>` (idempotent, tolerates NotFound, lab.sh:1741) succeeds, then the guest cleanup; add a fixture for a record whose volume is NotFound but whose delete failed after the metadata delete.

### R3-C2 — Low
The host script's way of calling `delete_volume` is not stated. lab.sh runs its command `case` when sourced, so the existing checks only extract single functions with sed (scripts/checks/resync.sh:10) and the script test uses `awk` extraction (lab-deletion-test.sh:21). `delete_volume` depends on about ten helpers (`detach_all`, `meta_retry`, `lost`, `cmd_node`, ...). Fix: name the mechanism (for instance `VOL=<run> scripts/lab.sh vol delete`, which is `cmd_vol delete`, lab.sh:727, and matches the `VOL=<id> make vol-delete` text), and say how the script test fakes it.

### R3-C3 — Low
Lab plan race: `make node-up ... && make block-contracts` runs the check right after the node starts, but the node registers as alive in the metadata service only some seconds later (`ensure_nodes` waits up to 60 s for this, lab.sh:1890-1896; `node.sh up` and `cmd_node up` do not). The check's `nodes-not-alive` guard can refuse the first run as written, and `refused` is not evidence. Fix: have the check wait a bounded time for both nodes (it changes nothing yet), or add the wait to the lab commands; add a fixture for the delayed case.

### R3-C4 — Low
Fixture gaps for the `leftover` rule: no row for a well-formed record whose volume still exists (must be `refused`/`leftover` naming `VOL=<id> make vol-delete`), nor for a `metactl get` error other than NotFound (must refuse, not clear). Without them the "NotFound only" test in the rule cannot fail. Fix: add both rows.
Reviewed commit: 8913c756970e304b0b9ca9d66bfb237b430eab19
