# Issue #70, plan review round 3 (incremental), design-code

Reviewed: git diff 4057eb9..82b7bec (HEAD a56309e adds only report.md).

## Round-2 findings
- R2-C1: fixed. engine.rs is in Files touched; the refresh check is stated once; a_malformed_refresh_is_refused covers each refusal. See R3-1 for one ordering detail.
- R2-C2: fixed. Args, securityContext and mount match the attacher (driver.yaml:71-76); *restricted exists; rbac.yaml already grants PV patch, PVC update, leases.
- R2-C3: fixed. a_failed_creation_keeps_the_old_size, the local path and the preflight account are in.
- R2-C4: fixed. faultKV (any kv.KV wrapper; Txn false/err/cancel, backoff <= 64 ms so 32 retries is fast), NodeExpand and ControllerExpand cases added.
- R2-C5: fixed. interleaveKV's txnKey hook runs after the first applied write (service_test.go:643-662); a drain there gives ErrPlacement on step 2.
- R2-C6: fixed. Appended chunks are created on placed replicas as build does (sim/mod.rs:392-396), clients and moves draw as stated (:1049, :1262), suffix_writes is paired with growths.
- Nits: fixed.
- R2-S1: documented and accepted risk; spec, design Targets, Risks and decisions agree; tests added (a_suffix_chunk_without_a_slot_waits, GrowExt4 ignoring cancellation).
- R2-S2, S4, S5: fixed. R2-S3: partly fixed, see R3-2.

## Findings

### R3-1 Low: the refresh check must run before the reply is converted
Design Targets, "Engine refresh". In engine.rs:1555 the refresh converts the reply with `rt.chunk_meta`, which calls `node_id`, which pushes new node names onto `rt.nodes` and `rt.addrs` (:382-390), and `missing_addrs` (:407) then starts a ListNodes call (:1566). A refused reply ("not installed, the core not called") would still have grown the node table and issued a call if the check sits after the conversion, and no test would notice. Fix: say the check runs on the raw `pb::Volume` before `chunk_meta`; have `a_malformed_refresh_is_refused` assert that `nodes` is unchanged and that no ListNodes call is issued.

### R3-2 Low: the lab's restore trap is cleared by csi_failover, and the added file is not checked after it
Design Lab. csi_failover sets its own `trap ... EXIT` and ends with `trap - EXIT` (lab.sh:1156-1157, 1189 region). Called from csi_expand it replaces and then removes the "uncordon, delete namespace" trap, so a failure afterwards, or a failure inside it, leaves the node cordoned and the namespace in place. Also csi_failover checks only the 256 MiB blob (`verified on $other`, lab.sh:1179); "the added file verifies" after it needs an explicit exec step that the design does not name. Fix: run csi_failover in a subshell, or reinstate the expand trap after it; name the post-failover step (an exec that verifies the added file and the PVC's 6Gi) and that it runs before the final cleanup.

## Nits
- spec.md S9 is titled "Advertise only what is supported", but the settled answer (spec.md:109-110, decisions.md:20) advertises while the metadata flag is off. Add "refusals name the flag" to S9 so the title does not contradict it.
- design.md Tests (S5, S8): "creates empty on its placed replicas at once ... for creation at the next Hello" is garbled. The sim creates the chunks before the Hello, so the Fence-arm creation is not exercised there; say replica/tests.rs covers it.
