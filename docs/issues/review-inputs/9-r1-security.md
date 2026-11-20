# Plan review: issue #9, round 1, focus design-security

Scope: docs/issues/9/ (decisions.md, intent.md, spec.md, design.md). I checked the plan against
the code already merged by PR #28 (186ef09, 7afa35d). The plan changes no data-plane code:
node.rs gets usage text only, there is no new unsafe, and the fast-read path (lease, deposed,
ISR-source and lock checks in flaminio-repl/src/target.rs:662-701) is untouched. So no read can
go stale, skip its lease or fencing check, or use a buffer the kernel still owns. Nothing
contradicts #85 (flaminio-uring batch lifetimes; #9 does not touch that code) or #84. The
findings below are all in the bench harness.

### R1-S1 — Medium
**The bench overwrites, and moves the target of, any volume the caller names, with no guard.**
The plan makes `VOL` a documented knob for any volume: the design's harness section ("They also
say `VOL` (default vol1)"), the Makefile help (Makefile:287) and docs/lab.md:424. Commit 186ef09
says "VOL can be any volume". But `cmd_bench_reads` does three things to that volume:
- it detaches the volume from every node (`release_lab_volume`, lab.sh:1511);
- it attaches it to NODE at a new epoch, which deposes its current target (lab.sh:1519,
  `cmd_vol attach`);
- it writes all of it with fio whenever the marker file `~/.flaminio-bench-filled-$VOL` is
  missing on that node (lab.sh:1535-1538, `bench-reads.sh fill`). The marker is kept per node,
  so it is missing on a new NODE, a recreated VM, or any volume the bench never saw.

So `VOL=<a CSI PVC> make bench-reads` cuts off the pod's I/O and silently overwrites the
volume's data. Neither the help text nor lab.md says that the bench writes VOL. Two of the
plan's own S2 refusal messages also tell the operator to rewrite VOL (FILL=1: lib.sh:237 and
lib.sh:243). #84's spec S7 sets the opposite rule for a lab check that writes: it "creates the
volumes it writes, accepts no volume or device from its caller".

**Fix:** write VOL only when `FILL=1` is set. With no marker, stop and say that FILL=1 overwrites
VOL. Or refuse any volume the bench did not create (for example, one that has a target and no
marker). Make the help text and lab.md say that the bench attaches VOL to NODE and that FILL=1
overwrites it. Add the no-marker refusal to a script test.

### R1-S2 — Low
**`VOL` reaches remote shell commands with no validation and no quoting.** AGENTS.md: "Ids
reaching shell commands or socket lines are validated and quoted."
- lab.sh:1535 and lab.sh:1538 run `vssh "$node" "test -f ~/.flaminio-bench-filled-$VOL"` and
  `"touch …-$VOL"`.
- `release_lab_volume` (lab.sh:1086) and `cmd_vol attach` (lab.sh:713, 718) pass
  `ENV_VARS="VOLUME=$VOL"`. That string becomes `env VOLUME=$VOL bash -c …`, which the remote
  shell parses (lib.sh:157).
- `release_lab_volume` runs on every VM before anything checks VOL. The first check is
  `metactl get`, at lab.sh:1523.

The plan widens VOL to any volume (R1-S1), but it never validates it. `valid_id` already exists
at lab.sh:1633. **Fix:** in the design's harness section, and as the first line of
`cmd_bench_reads`, add `valid_id "$VOL" || die`, and quote the remote uses with `printf %q`.
Add a script-test vector for an invalid VOL.

### R1-S3 — Low
**The local-read precondition checks replica placement, not ISR membership. The under-90%
refusal then gives the wrong cause and suggests an overwrite.**
- `chunks_on` (lab.sh:1257-1258) counts chunks whose `.replicas` contain the node. The message
  at lab.sh:1532 says "need local ISR".
- The target reads only from an ISR source (target.rs:670, `self.source(i, None) !=
  Some(self.node)`). So if a local replica is outside the ISR (it is resyncing, or a campaign
  restore left the ISR elsewhere), the volume is read over the wire.
- The trace then finds less than 90%. S2 and lib.sh:237 blame that on "not on this disk, or
  never written (FILL=1 writes it)".

**Fix:** before the trace, check that every chunk's `.isr` contains the node, and stop with that
cause. Name this cause in S2 for the under-90% refusal.

Reviewed commit: e0c91cdcf0a0c89e432546ba290239683ba143a5
