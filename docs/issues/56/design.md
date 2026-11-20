# Issue #56: Two-data-node contract and checks — design

[spec](spec.md) · [decisions](decisions.md)

Historical approval: 2026-11-13; review revisions below are unapproved. Not implemented.
Existing intent's first increment. Changes tests/scripts/docs only, with no protocol fix,
new topology installer or ADR0006 default change. Lab results below are planned, not measured.

## The facts it starts from

`flaminio-repl/src/sim/mod.rs:130` randomizes3–4 data nodes; `:202` scripted Action already
supports Attach/Cut/Heal/Slow/Read/Write. Metadata is one linearizable model endpoint (`:340`),
not a distributed etcd election. `sim/tests.rs:229` validates clean seed before accepting a
mutation catch. `scripts/guest/partition.sh:13` drops all wg0 with a shared tag, not one pair.
`control/failover/failover.go:83` treats absence as out-of-service; #82 confirms this unsafe
for arbitrary node shutdown inference. Current clock source is unqualified for suspend/VM
pause (#83). #81 owns >64 ISR changes; baseline scoped lab uses exactly64 chunks.

## Fault table (S1–S5)

Add `docs/edge-two-node-contract.md`. Initial target=A, all ISR={A,B}, connected etcd A/B/W,
RF2; swap A/B for symmetric cases. Every row states initial committed ISR, not only topology.

| Fault | minISR1 outcome | minISR2 outcome |
|---|---|---|
| B lost, A+W quorum | A commits removal then continues; resync B before join | writes needing B wait; reads only under valid confirmations/lease |
| A lost, B+W quorum | new epoch at B; fence/reconcile/takeover wait then permitted removal of unreachable A and writes | no write ACK without both; no stale-successful read |
| W lost, A/B connected | existing I/O continues; etcd majority survives; lab Kubernetes controls unavailable | same for data I/O; no weakened all-ISR ACK |
| A↔B cut, both↔W | A may commit singleton ISR and serve; B stays nonauthoritative | writes wait; read lease/readiness still gate reads |
| same cut, attach B after singleton A | new epoch may depose A, but B cannot adopt out-of-ISR local data; both may wait until heal | new target needs normal readiness, no alternate data authority |
| A↔W only cut; A↔B↔W connected | A/B still form metadata majority; no membership loss solely from this edge | same, if all data/session paths available |
| A isolated from B/W, initial ISR A/B | B+W may take over safely; old A stops successful reads after its old confirmation expires | writes remain blocked without2; no unsafe minimum relaxation |
| A isolated after ISR already A | B cannot reconstruct acknowledged singleton history; A may continue with local confirmations; no other writer | not a reachable ordinary minISR2 membership |
| two voters lost | no new metadata decisions; already-ready I/O depends on current ISR/confirmations, not quorum liveness alone | no new target without quorum; both-member write rule remains |
| sole authoritative data disk permanently lost | accepted single-copy writes may be unrecoverable; no stale-disk automatic promotion | any claimed no-loss recovery needs all-ISR evidence, not epoch/sequence guess |

A metadata outage does not itself expire a lease: current ISR confirmations can remain valid.
An unreachable required member does. New target must obey the stricter current protocol,
not ADR0005's outdated immediate-read sentence (audit Proposed0007). Distinguish inability to
schedule a pod from inability of an existing target to serve. No power-off timing is a safety
proof. Double faults enumerate actual surviving voter/data sets and pre-fault ISR; don't claim
a two-loss guarantee from the word witness.

## Simulator and mutation coverage (S6, S7)

Add `Config::two_node(seed,min_isr)` which starts from the same sampled timing/workload values
then sets nodes2,rf2,min_isr1|2 and move_pct0; validate max/geometry bounds. It does not change
Config::random or existing seeds. Add debug bounded campaign tests and release20,000-seed
campaigns per minimum through current make sim conventions. Preserve seed/config in failures.

Create deterministic `sim/edge_tests.rs` with model-only `EdgeStep` controls: Crash/Restart,
LoseDisk(crashed_node), MetaAvailable(bool), Attach, labeled Probe(node,chunk,block,read|write), AdvanceTo and Assert.
Keep existing Action/seeds unchanged. Attach refuses when modeled metadata authority is down
or its caller cannot reach META; it must not call Meta::handle through today's script bypass.
Probe records Issued(op_id) or Rejected(reason); missing/busy/deposed targets fail a required
issued-probe assertion rather than silently skipping. `AdvanceTo` drives events without healing;
phase assertions inspect committed ISR, target readiness/deposition, probe completion/value and
history linearizability before EndChaos. Forbidden-success assertions refer to actually issued
probes and a stated bounded phase, with enough simulated lease/takeover time elapsed.

Each fault-table row has a named case and swapped-target variant for both minimums: replica
crash, target crash+attach, witness unavailable with A/B majority, A/B cut before/after singleton
and attach-other, each A/W or B/W cut, target isolated with full/singleton ISR, each two-voter
loss and sole-authority disk loss. Witness/voter availability is a test input translated into
META availability plus per-node META links from a three-voter reachability matrix; document
that this grants/refuses linearizable metadata authority, not an etcd election simulation.
Crash/Restart uses existing model incarnation rules; LoseDisk is test-only permanent loss of a
crashed node's stored replica state. Disk-loss cases assert unavailable recovery of accepted
singleton history, never manufacture stale promotion. Require successful post-fault survivor
writes where allowed, blocked issued writes for minimum2, expired old-target reads forbidden,
serve-nowhere after attach-other and resync before admission after explicit Heal/Restart.
Only then permit EndChaos and existing final convergence checks. Harness regressions prove
skipped probes and Attach with metadata down cannot satisfy a phase; no production rule changes.

Add a checked coverage table listing every existing mutation and exact two-node scenario or
seed witness with clean counterpart. Move-only mutations are structurally inapplicable because
RF2 uses both available data nodes. The two mutations the intent search did not catch remain
open until a witness or actual structural proof exists;320k unsuccessful seeds is not proof.
Existing generic mutation tests remain required. If a clean seed fails, report a defect and
stop that contract's clear claim; never filter it out as noise or loosen linearizability.

## Lab history and faults (S8–S10)

Proposed `make edge-two-node-test MIN_ISR=1|2` through lab.sh/checks/edge-two-node.sh. Runs an
explicit owned test volume with64chunks and both data nodes in every initial ISR. Record exact
binary/dependency versions. With #83 enforcement active, validate physical-boottime-v1 first;
VM/unknown provenance returns Blocked before creating data or mutating faults. No force flag,
fake physical contract or startup bypass. An explicitly selected legacy pre-#83 binary may run
`EVIDENCE=legacy-exploratory` after separate lab authorization; its overall production verdict
is Blocked regardless of measurements. Qualification needs supported physical hardware.

Before any legacy fault, exclusively hold and stop the known failover Deployment using the
conditional procedure in [lab-contracts.md](lab-contracts.md). Confirm no controller process or
owned Pod remains and no preexisting out-of-service mark; absent/incomplete ownership refuses.
Manual ordinary metadata attach may exercise target takeover without Kubernetes node recovery.
Current #82 manual-lockout-v1 is physical-only, holds restart exclusion through old-UID terminal
retirement and refuses VM/sample-power providers. Therefore these VM checks never mark nodes
out-of-service or claim full pod recovery. A sampled power-off confirms fault effect only.

Use #49's complete4096-byte value encoding and bounded register search, with the separate
`FLEDGE01` coordinator stream and helper/adapter specified in [lab-contracts.md](lab-contracts.md).
One management monotonic clock and durable coordinator journal define all operation envelopes;
VM times are diagnostics only. This format supports overlapping old/new endpoint probes and
powered-off helpers without a same-boot End. #49 FLHST001/FLRUN001 remain unchanged. The adapter
handles unknown replies and later-epoch optional-write deadlines explicitly; no causal data
is read from a contextual manifest. Exact caps/checkpoints fail Undecided, never pass by dropping
pending operations. Preserve every acknowledged write, including a validated late response.

Alternate/overlap controlled probes where needed; record acquired epoch and normal attachment
readiness, then require post-fault writes on the permitted survivor. For a wait outcome, require
issued probes and no forbidden successful response during the bounded observation. Availability
is that observation, not proof of an infinite wait. No fio job overwrites a history block.

Extend partition.sh with `pair-on RUN PEER_IP|pair-off RUN PEER_IP|pair-status RUN PEER_IP`,
validated run nonce and allowlisted overlay IP from inventory. Install exact INPUT/OUTPUT rules
with run+peer comments, on both endpoints where needed; preserve management addresses. Check
iptables rule counters plus positive negative-path probe and unchanged control path; rule
existence alone is insufficient. Heal deletes only exact owned rules, idempotently. No root
qdisc replacement. Persist expected rules before mutation and actual results after each step.

Legacy VM-loss cases confirm libvirt power-off plus service disappearance; suspend/pause is
not equivalent. Before mutation persist strict typed owned-action records, using the bounded
schema/validation/healer procedure in [lab-contracts.md](lab-contracts.md). Traps handle normal
failures; management loss requires independent recovery, never an unconditional self-heal claim.
The local watchdog bounds waits and attempts cleanup; final unhealed state fails. Witness loss
uses management/libvirt because Kubernetes API on W is then unavailable. Keep the unsafe
controller stopped while VMs restart; restore its prior state only after all nodes/services,
quorum, full ISR and absence of new out-of-service marks are confirmed. Do not clear a mark.

Observe ISR through coherent #77 GetVolume views: `(epoch, [(chunk,isr_ver,ISR)])` with each
sample's management invocation/return envelope. Neither public etcd revisions nor watches are
assumed. Record first observed singleton and full-ISR samples; elapsed bounds include sampling
gaps/metadata outage. Changed epoch/view tokens or missed transitions remain explicit unknowns.
No condition/metric/alert implementation is smuggled into this test increment (#71 later).
After each heal require full ISR, data verification, node/service/quorum recovery and no owned
rules. Stop before next case if any restore step is uncertain. No disk format or etcd wipe.

## Contracts and files touched

No production API/wire/on-disk/control grammar changes. New simulator/test helpers only; existing
readiness and ISR rules unchanged. Add contract doc, sim edge tests/coverage table, scripts/checks/
edge-two-node.sh, guest pair-rule helper, Go coordinator/helper under pkg/blockhistory and
control/cmd/flaminio-edge-history, Rust sim/history adapter/check_edge_history example and Make
help. [lab-contracts.md](lab-contracts.md) fixes test stream/helper/owned-healer seams. Proposed
result JSON schema_version1: run/topology/min_isr/sourceHEAD,
case/faultproof/initialISR/epochs/history_verdict/availability_observed/singlecopy_bounds,
clock_profile/cleanup/evidence_paths. Verdict Pass|Fail|Undecided|Blocked; only Pass is clear.
Exit0all requested cases pass,1failed/undecided/blocked,2arguments,255management SSH loss.

## Tests

| Rules | Concrete planned checks |
|---|---|
| S1–S6 | edge model scripts/campaigns, both minimums/swapped targets; each row asserts fault-phase outcomes before healing; skipped probe and metadata-down Attach harness regressions |
| S7 | clean counterpart for each mutation witness; excluded moves with structural explanation; unresolved mutations fail coverage goal |
| S8 | lab-contracts fixtures: overlap, powered-off helper, late reply, optional-write resurfacing, corrupt stream, caps/checkpoint carry; clean device passes and stale/lost device fails |
| S9 | fake iptables/libvirt/SSH/API: no-effect cut, hostile/replayed/truncated inventory, foreign rules/marks, interrupted controller hold, changed UID/spec refusal, resumed heal and no disk mutation |
| S10 | coherent epoch/isr_ver/ISR samples, metadata gaps and missed singleton transitions; exposure bounds unknown rather than zero |
| S11 | integrated #83 VM run Blocked before any action; legacy exploration never production Pass; VM power samples never call #82 recovery; generic seeds unchanged |

Run `make lint test sim contracts` for tests/scripts and standard PR race/hygiene. No need to
run destructive lab commands merely to draft. Simulator checks are future implementation tests.

## Lab, docs, risks and ADRs

Authorize this specific power-cycle/partition campaign before execution. It deletes only its
own test workload/data on cleanup; preserve unrelated PVCs/services. In legacy checks restore VMs,
remove owned pair rules, recover services and full ISR before declaring clean. Retain evidence
and exact manual healer instructions in docs/lab.md. No raw disk reformat. Record known lab W
role and shared-physical-host limitations. ADR0006 accepted single-copy-loss risk remains;
its visibility/backups/resync priority and #44 region-only resync are not implemented here.
No new architectural ADR: tests refine current protocol, not a new risk acceptance. New clean
counterexample opens/links the owning correctness issue and blocks affected contract clearance.

## Parallelizable?

Yes: simulator cases/coverage and owned lab fault/history harness can be built separately
against the stated table/result contract. Final evidence requires coherent dependency versions.
