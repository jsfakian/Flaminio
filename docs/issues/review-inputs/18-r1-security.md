# Issue #18 plan review: round 1, design-security

Scope: docs/issues/18/ (intent, spec, design, decisions), checked against the code at the commit below, and against #13 (review-clear), #72, #78 and #79 where #18's placement touches them.

### R1-S1 — High
**Under `strict`, an unlabelled node counts as a zone of its own, so a drain sends replicas to a new node before its labels arrive, and the spread breaks for good.**
The spec defines domains "as at creation": a node without the label is in the one empty domain (spec.md Terms; `domainKey`, meta/domain/domain.go:322-331). So an unlabelled destination never collides with a labelled staying replica: `"z:"` is not `"z:lab-a"`. In practice a new node always starts out unlabelled:
- The data plane registers the address only (dataplane/crates/flaminio-node/src/engine.rs:585-593) and starts heartbeating.
- The zone and rack labels come later, from the CSI node plugin's `SyncTopologyLabels`. It waits for that registration, retries every 5 s, runs only in-cluster, and skips labels Kubernetes lacks (control/csi/topology.go:21-51; control/cmd/flaminio-csi/main.go:91-95).

Meanwhile the node is alive, not a witness and not draining, with load 0. The drain's least-loaded choice therefore puts it first (drain.go:76, 157-162), and the service accepts the move (design.md "Accepting a move" steps 2-3). Once the plugin labels the node into the staying replica's zone, two copies of the chunk sit in one zone. Nothing repairs that (intent, Out of scope).

This is the usual way to replace a node: add one, drain the old one. Any node can also clear its own zone to an empty value (merge semantics, service.go:197-200) and so read as a fresh, distinct zone.

The same fail-open applies to a staying replica whose record is missing or does not parse: it is read as "unlabelled" (spec S8; design step 2), so it collides only with unlabelled destinations. The lab plan treats this behaviour as correct: step 3 moves a zone/strict volume onto fl-c, which has no zone. So do the tests: S5-S7 has "a staying replica without a node record ... collides with an unlabelled destination".
**Fix:** for a `strict` volume at `rack`/`zone`, make an unknown domain fail closed in both the service and `choose`:
- refuse a destination with no label (or an empty one) at that level, with `FailedPrecondition`, "destination has no zone label";
- treat a staying replica whose record is missing, does not parse, or has no label at that level as colliding with every destination.

Alternatively, record this explicitly as an accepted risk in decisions.md, with the drain at least skipping unlabelled destinations for strict rack/zone volumes. Under the fix, the lab gives fl-c its own zone (`lab-c`) for step 3. Add tests for an unlabelled destination and for a staying replica whose record does not parse, each refused under strict.

### R1-S2 — Medium
**A metadata-service rollback strips the record from every volume attached during it, silently and for good.**
Q5 (decisions.md) accepts that an older instance's `Attach` drops the two keys. Its only remedy is an ordering rule: every instance is upgraded before a rack/zone volume is created or moved (design.md Docs; spec Q5). A rollback cannot meet that rule, and rollback is supported: ARCHITECTURE §18 tests "N → N-1 rollback before finalize" (docs/ARCHITECTURE.md:940), and §18 already forbids creations during a downgrade for #30's sake (:916-918).

During a rollback, every agent restart (`only_if_target` Attach) and every CSI publish runs through the N-1 instance. That instance marshals the header from the fields it knows (service.go:723-725, `json.Unmarshal`, which keeps no unknown keys). After the re-upgrade, those volumes read as `node`/`strict` (design.md, `Placement`), `get` shows `node`, nothing warns, and the next drain may put a chunk's replicas in one zone.
**Fix:** add to ARCHITECTURE §18 and docs/lab.md that a downgrade below this version loses the level and policy of every volume attached during it. Have operators list rack/zone volumes before a rollback (`flaminio-meta get`), and name Q3(b), the administrator's set command, as the way back. Or say plainly that rollback is unsupported once a rack/zone volume exists. Record the choice under Q5.

### R1-S3 — Medium
**The drain identifies a collision refusal by the gRPC code alone, but #72 and #78 return the same code for other refusals of `MoveReplica`.**
#18's contracts say: "`FailedPrecondition` only for a collision ... The drain tells a refusal by the code alone" (design.md:124-126, :84-85). A refused chunk is counted as blocked and refused, and `Run` keeps going. Two other plans use the same code for other refusals:
- #72 returns `FAILED_PRECONDITION` for a destination with no capacity report or no readable ledger (/tmp/flaminio-plans-2026-11-12/72/docs/issues/72/capacity-contracts.md:115), and `RESOURCE_EXHAUSTED` for a full one. Its drain then drops that destination, picks another, and fails the step when none is left (#72 design.md:105-108; spec S8).
- #78 refuses a draining destination with `FailedPrecondition` (/tmp/flaminio-plans-2026-11-12/78/docs/issues/78/design.md:53-54).

With these combined, #18's drain:
- reports a capacity or drain-mark refusal as "refused by the service, tried again";
- proposes the same destination again at the next step, because `choose` cannot see capacity, and runs to the 30-minute time-out;
- fails the whole step on `RESOURCE_EXHAUSTED` ("Other errors as today").

#72's re-pick ignores #18's allowed set and bestEffort levels, and its "step fails" contradicts S11.
**Fix:** give #18's refusal a distinct marker beyond the code, such as an `errdetails.ErrorInfo` reason `FAILURE_DOMAIN_COLLISION`, or #13's `MoveReplicaResponse.reason`. Specify that any other `FailedPrecondition` or `ResourceExhausted` takes that destination out for the step, and `choose` runs again within the allowed set at the same level. Record the hand-off to #72 and #78 in decisions.md, done by whichever plan lands second.

### R1-S4 — Medium
**#13's hand-off to `choose` is missing, and #18's "blocked" contradicts #13's "waits".**
#13, which is review-clear, assigns the change to #18. Its decisions.md:235-236 reads: "#18: `choose` ... excludes the chunk's `GetReleased()` with its replicas. #18's text needs the change". Its design.md:792-795 says that when only nodes releasing a copy are left, the chunk waits and is not a failure. #18's `choose` excludes only replicas (design.md:76-81), and a chunk with nothing chosen is blocked, so `Run` returns `BlockedError` once only such chunks remain (design.md:82-83, :92-93). The two possible results:
- **Without the exclusion:** `choose` proposes a releasing node at every step. #13's `CheckMove` answers `applied=false` with a reason, which #18 counts as neither blocked nor moving, so the drain runs to its time-out without saying why.
- **With a naive exclusion:** a chunk that will have a destination once the release is confirmed ends the drain with "no destination".

**Fix:** in design.md "The drain", have `choose` exclude `GetReleased()`. A chunk whose allowed set is empty only because of releases waits (counted as a pending move is, with #13's `Held` reason) and is not counted as blocked. Note that this is due in whichever plan lands second.

### R1-S5 — Medium
**`Volume` fields 9 and 10 are already claimed by #79.**
#18 adds `failure_domain = 9` and `failure_domain_policy = 10` (design.md:115-117). #79 takes `Volume.use_profile = 9` and `Volume.initialization = 10`, a message (/tmp/flaminio-plans-2026-11-12/79/docs/issues/79/initialization-contract.md:45-46). #17 already moved to 16 and 17 because "9 and 10 are #79's" (flaminio-17 design.md:530), and #13 lists 9 and 10 as claimed by #17, #18 and #79 (flaminio-13 design.md:928-929). If either plan lands with these numbers, the other's wire types conflict for older readers, including a hand-written `pb.rs` once #79 names tag 9.
**Fix:** move to the lowest pair no plan of the batch claims (18 and 19 by #13's list). Record the choice in decisions.md, and update design.md Contracts and its Tests line ("fields 9 and 10").

### R1-S6 — Low
**A stored level or policy word the drain does not know fails the whole step, against S11.**
The design says "a word they refuse fails the step" (design.md:77-79), so one volume's unknown word stops every other volume's moves. Unknown words do reach the drain: `volumePB` reports them as stored (design.md:40-42), for example a later version's word read by an older CLI, or damage. S11 says the step "still requests every move it can, for that volume and the others".
**Fix:** count such chunks as blocked, with the word in `BlockedVolumes`, so the drain ends with `BlockedError` and not on the first one. Add a drain test.

### R1-S7 — Low
**The Risks entry on labels understates who controls them, now that labels gate every strict move.**
design.md Risks says only "A node can still relabel itself (RegisterNode)". In fact `auth.go:71-72` lets three kinds of identity set any label:
- every administrator identity, for any node: the CSI controller's `admin/csi-controller` certificate (scripts/lab.sh:1004) as well as the operator's;
- the node's `agent`;
- the node's `io`, for its own node, even though the data plane never sends labels (engine.rs:585-593).

A node can attract strict replicas by claiming a new zone or an empty one (see S1), or repel them and block drains by claiming a staying replica's zone. No test covers either.
**Fix:** state the full set of label writers in Risks and decisions.md as the accepted trust model. Alternatively, as a small hardening, refuse topology-label changes from the `io` component in `RegisterNode`, with an `auth_test.go` case; otherwise file it as its own issue.

## Nits
- design.md facts, "a move compares at most 8 keys (5 replicas)": it is 7 at most. The chunk, the header, the destination, and at most 4 staying replicas (5 replicas minus `from`; `CheckMove` refuses while a move is pending).
- design.md "The refusal": say the label value and the ids are formatted with `%q`. A label is a free string a node sets on itself (service.go:197-200). Unquoted, a value with `"` or a newline could forge the `shares zone "lab-a" with replica "fl-a"` text that the lab's step 4 greps, or break the admin's stderr.

Reviewed commit: 12874537d333dc6a0fcd3a2ea4f5abcc86fee85d
