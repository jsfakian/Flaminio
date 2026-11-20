# Issue #14 — plan review, round 1, focus: design-code

Scope: docs/issues/14/{intent,spec,design,decisions}.md at HEAD. #14's step 2 is already
merged (PR #31, c062c37). #30 (PR #34), #10 (PR #32) and #11 (PR #38) merged after it. So the
plan was checked against the code as it is now: meta/cmd/flaminio-meta/main.go,
meta/service/{service,auth}.go, scripts/lab.sh, scripts/guest/{node,confirm,common}.sh, and the
tests named in the plan. #13's plan (/home/jsfakian/Documents/src/flaminio-13/docs/issues/13/)
was read where it builds on #14.

Contracts: no protobuf change; ConfirmDeletion makes a single conditional write
(TxnRev, compare on ModRevision), well within MaxTxnOps; no new RPC, so no new auth.go rule.
No new replication safety rule, so no simulator check is needed.

### R1-C1 — Medium
**The plan's account of what a wrong confirmation does, and the shipped usage text, predate
#30's merged code.**
- intent.md:43-48 (§6) says a new target's `hello` "creates only the chunks a replica lacks,
  keeping those it has". It concludes that leftovers on a confirmed node become the new
  volume's chunks ("it serves the old volume's data"), or depose it.
- intent.md:39-40 (§5) quotes `delete_wbench`'s old comment, "a new wbench (epoch 1 again) would
  be deposed…". At HEAD the comment says the opposite (lab.sh:1872-1875).
- design.md:15-17 says the control socket's delete "deletes under epoch `u64::MAX`".
- The code shipped for S2 repeats the old consequence in its doc comment and usage text: "the
  new volume then serves the old one's data there, or is deposed by them"
  (meta/cmd/flaminio-meta/main.go:28-30, 214-218).

At HEAD a re-created volume starts at its claim's revision, above every earlier epoch. At its
first Hello on a node, the replica drops every chunk of the name fenced below that epoch
(commit fb99440). The control socket's delete is now `Store::delete_volume(uuid, None)`, "every
chunk, whatever its fence" (flaminio-node/src/replica/mod.rs:813-822). docs/lab.md:244-248 and
followups §14 were updated for this; the plan and the usage text were not. The conclusion
("worse than a blocked id") still holds, but for another reason: a deleter that has yet to run
(the CSI sweep, `node.sh delete`) ignores fences and wipes the new volume's replicas. #30's
residual cases add to that: old tokens on nodes not yet reached, and a volume created by an
old metadata service.

**Fix:**
- Rewrite intent §5-6 and the design's facts bullet to describe the code at HEAD.
- Say in S2 what the usage text must state: the wipe hazard and #30's residual cases, not
  adoption or deposition.
- Correct main.go:25-30 and 214-218 to match. TestUsage (main_test.go:366) asserts only "keeps
  them".

### R1-C2 — Low
**#14's plan contradicts #13's settled plan about step 1.**
- design.md:139 says "Issue #13's sweep unit, when it comes, joins this list" (the no-sweep
  check in `confirm.sh`).
- intent.md:66-67 and decisions.md:22 place step 1 "in the node agent".

#13 puts the sweep inside the Rust node daemon, `flaminio-io node` (#13 intent.md:4-5, a user
decision of 2026-11-05). It moves tombstone listing and confirmation to the node's data plane
identity (#13 spec.md:77-79, S12). It removes `vol-confirm`, `confirm.sh` and
`flaminio-meta confirm` (#13 spec.md:81, S13; decisions.md:254-258; design.md:560-561). There
is no unit to add to `no_sweep`. The sweep would also run in the very `flaminio-io` process
that `confirm.sh`'s `data_plane` requires to be running (confirm.sh:71-77). So a `pgrep` or unit
check could never see it, and following design.md:139 would leave two confirmers on fl-c.

**Fix:** replace design.md:139 with a line saying that `vol-confirm`, `confirm.sh` and `confirm`
go in the same change as #13's sweep and must not outlive it (#13 S13). Name the node daemon,
not "the node agent", in intent.md:66 and decisions.md:22.

### R1-C3 — Low
**S7.8 claims a check the code does not make.**
- spec.md:94-95: "the confirmation finds the volume still pending: otherwise another caller
  confirmed, and it fails".
- docs/lab.md:243 makes the same claim.

The code checks only `flaminio-meta confirm`'s read, the `ListTombstones` made just before
`ConfirmDeletion` (main.go:478-494). `ConfirmDeletion` answers with the same empty success in
three cases: the node was newly confirmed, it was already confirmed (service.go:652-669), or
the tombstone is gone (service.go:631-633). A confirmation by another caller between the read
and the call is therefore reported as vol-confirm's own success. decisions.md:26 already says
"the empty response cannot say more". The rule is worded as if the service's answer could.

**Fix:**
- Word S7.8, design step 7 (design.md:150-152) and lab.md:243 as "the read just before the
  call shows the volume still pending".
- Name the read-to-call window among the accepted risks, next to the existing "between
  `vol-confirm`'s checks and its confirm" entry. Telling the cases apart is the incarnation
  follow-up's job.

### R1-C4 — Low
**"RF 1 on fl-b never does [stop]" is false when `$VOL` has a replica on fl-c.**
- spec.md:80-81 (S6) says it, and docs/lab.md:226-227 repeats it.

At every campaign start, `write_start` deletes `$VOL` wherever it is, through `delete_volume`
(lab.sh:2155-2158). If `$VOL` (RF 2, placed by `vol create`) has a replica on fl-c, the campaign
stops for fl-c whatever its own RF and node. The lab check recorded exactly this: "A campaign
whose `$VOL` (volb) was on fl-c stopped at its start" (followups.md:297).

**Fix:** say that an RF 1 campaign on fl-b never stops for wbench, and stops once at its start
when `$VOL` has a replica on fl-c.

### R1-C5 — Low
**Several statements in the design differ from the code that shipped. Each would mislead #13,
which edits these functions.**
- design.md:125-127 says `confirm.sh` "takes the paths of `flaminio-meta`, `flaminio-agent` and
  the disk tool … as variables". The code fixes them and honours `META_BIN`, `AGENT_BIN` and
  `DISK_TOOL` only under `FLAMINIO_SCRIPT_TEST=1` (confirm.sh:21-26). That tightening matters
  for security and the design omits it.
- design.md:137-138 says the k3s check uses `systemctl is-active` and `is-enabled`, and accepts
  `inactive`, `disabled` or `not-found`. The code reads `systemctl show` LoadState, ActiveState
  and UnitFileState, and also accepts `masked*`. Its comment says why: systemd 249's
  `is-enabled` prints nothing for a missing unit (confirm.sh:41-50).
- design.md:150-151: `confirm.sh confirm` also runs `no_sweep` again (confirm.sh:105); the
  design omits it.
- design.md:132-133: `vol-confirm` also refuses a node that is not in `data_vms`, meaning fl-c's
  `created` flag is missing (lab.sh:1819). The design omits this check.
- design.md:97 says "A failed poll is retried". The code retries only ssh's 255 and the
  transient codes; any other error stops at once (lab.sh:1787-1789).
- design.md:72 lists `cmd_vol` detach among the callers that get `>/dev/null`. In the code it
  captures the status line and checks it (lab.sh:721-725).

**Fix:** bring these lines in design.md into line with the code.

### R1-C6 — Low
**The intent's references to spec rules are broken.**
- intent.md:70 puts the log line in "spec, S5". It is S4; S5 is the deletion.
- intent.md:74, 78, 83, 85 and 89 say "Spec S13 records it" five times: for the incarnation
  field, the gone-for-good node, the other deleters, the lab lock and fl-c's keys.

The spec ends at S10. Those items are in decisions.md "Accepted risks" and followups §14
"Later".

**Fix:** point to S4, and to decisions.md and followups §14 instead of S13.

## Nits
- spec.md:51-53 (S4) says "each confirmation that newly confirms a node logs, once", with no
  exception. decisions.md:76 and design.md:59-60 accept that the line can be missing after etcd
  loses the answer to the committing write. When that write was the last confirmation, the
  retry finds no tombstone (service.go:631-633), so there is no line in that case either. State
  the exception in S4.

Reviewed commit: 4024388accbb93666ecee338be9d3ea3d9d7126c
