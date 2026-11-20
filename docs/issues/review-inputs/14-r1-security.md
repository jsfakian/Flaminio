# Issue #14 — plan review, round 1, design-security

Scope: docs/issues/14/ (intent, spec, design, decisions), checked against the code at HEAD.
#14 step 2 is merged (PR #31, c062c37); #30 is merged after it (fb99440, 32d7475) and changed
the code that the plan's hazard model describes.

Checked and sound (no finding): ConfirmDeletion and ListTombstones authorization (auth.go:75-78,
unchanged; agent bound to its own node); the CLI's identity check before any call
(main.go:253-286, as pki.PeerIdentity); read-first confirm; the one-confirmer guard on fl-c
(vol-confirm refuses cluster members, checks CSI/agent/k3s, re-checks before confirming); a
tombstone pending for a node blocks re-creation (service.go:375-378, claim compares the
tombstone at Rev 0) and is never overwritten while pending; ids reaching remote commands and
socket lines are validated (lab.sh valid_id, node.sh volume_id, confirm.sh:19-20) or %q-quoted
(metactl); the S4 log line is conditional on a TxnRev write and bounded.

### R1-S1 — Medium
**The plan's hazard model is pre-#30. The code at HEAD contradicts it, and the plan never names
the hazard that remains.**

Evidence:
- intent.md:43-48 (Why 6) says a re-created volume's `hello` keeps an old volume's chunks and
  "serves the old volume's data", or is deposed by chunks fenced later. At HEAD:
  - a volume's first epoch is its claim's revision (service.go:458-462);
  - `hello` drops every chunk of the name fenced below it (replica/mod.rs:193-211).
- These lines repeat the stale claim:
  - spec.md:23-24, the invariant: "keeps them, and a volume created again with that name meets
    them";
  - S2 makes the usage text "say what a wrong confirmation does". The code prints "serves their
    data there, or is deposed by them" (main.go:25-30, 214-218), and TestUsage pins it
    (main_test.go:366).
- decisions.md:30-31 justifies refusing administrators with "true of any stopped node that holds
  replicas". With #30, confirming such a node no longer harms a re-created volume.
- design.md:15-17 says the control socket's delete runs "under epoch `u64::MAX`". It is now
  `Store::delete_volume(uuid, None)` (replica/mod.rs:813-821). #30 D13 made that change because
  u64::MAX kept a chunk fenced at u64::MAX yet answered success, so the tombstone got confirmed.
- docs/lab.md:248-252, updated by #30, already says the opposite of the usage text.
- What remains, and the plan names nowhere: every delete of the name ignores the fence. One that
  runs after the id is freed wipes the re-created volume's replicas on that node; with RF 1, its
  data. Such a delete comes from a second confirmer in the middle of a sweep, or from the
  harness (R1-S2). This is the hazard the tombstone still guards (service.go:373-374).

Fix:
- Restate intent Why 6, the spec's invariant, S2's usage text, and the reason of decisions.md:30
  for HEAD:
  - #30 drops leftovers at the first session;
  - leftovers still hold space, and old tokens still open sessions on them;
  - a volume created by an old metadata service is still deposed;
  - a fence-blind delete that runs after the id is freed wipes the new volume's replicas.
- Keep the decisions themselves (they are now more conservative than needed), and record that
  #30 changed their reason.
- Correct main.go's doc comment, its usage text and TestUsage to match.
- Correct the design's u64::MAX fact.
- Record in the incarnation follow-up that a delete bounded below the deleted volume's epoch
  (`Store::delete_volume(uuid, Some(..))` already exists) would close the remaining hazard.

### R1-S2 — Low
**The harness's deletes ignore fences and can run after the id is freed. Step 4 cannot see a
creation in progress. The invariant "every hazard needs two confirmers" overstates.**

Evidence:
- delete_volume (design.md:79-92; lab.sh:1733-1766):
  1. deletes the metadata;
  2. starts stopped nodes (up to minutes; a FORMAT=1 start included);
  3. checks `get` = NotFound;
  4. sends `delete` to every data node. That delete removes every chunk of the name, whatever
     its fence.
- The first fl-b CSI sweep, within 5 s, frees the id of an RF 1 wbench on fl-b (tombstone
  {fl-b}). Two gaps follow:
  - `get` answers NotFound for a header still marked Creating (service.go:286-288), so the
    "re-check" (design.md:89-90: "anything else (a creation meanwhile): exit 1") misses a
    creation in progress;
  - the per-node deletes after it are not covered at all.
- In that window, a concurrent `make vol-create`/`vol attach` of the same name loses its
  replicas on every node. No second confirmer is involved.
- The accepted risk names only "two harness runs at once" (decisions.md:70). The spec's
  invariant (spec.md:18-21) claims every hazard needs two confirmers.

Fix, one of:
- State the residual exactly: the harness deletes on every node outside the one-confirmer rule;
  step 4 sees only finished creations. Widen the accepted risk to any concurrent creation of the
  name during a deletion, and make the invariant "one deleter-and-confirmer per node, plus the
  harness while no other lab command runs".
- Or close the window: delete the replicas on every data node after the detach and before the
  metadata delete. The live header then blocks re-creation. After that, only wait for the
  tombstone.

### R1-S3 — Low
**The design's no-sweep check on fl-c is weaker than the code's and names a mechanism the code
rejected.**

Evidence:
- design.md:136-139 asks for "`k3s` and `k3s-agent` each `inactive`, `disabled` or `not-found`
  (`systemctl is-active`, `is-enabled`)". That reads as any one of the three, which accepts an
  active but disabled k3s-agent: a running CSI sweep, so a second confirmer for fl-c.
- confirm.sh:41-50 uses `systemctl show` (LoadState, ActiveState, UnitFileState), because
  `is-enabled` prints nothing for a missing unit on systemd 249. It requires `inactive` and
  (`disabled` or `masked*`), unless the unit is not found.
- design.md:150-152 (step 7) re-checks only the unit, the process and the delete. The code also
  re-runs the sweep check before confirming (confirm.sh:105, tested at
  node-scripts-test.sh:231-237). That re-check narrows the accepted risk "a sweep started on
  fl-c between the checks and the confirm" (decisions.md:66-67).

Fix:
- Describe the check as confirm.sh does: `systemctl show`; inactive and disabled or masked, or
  not found.
- Add the sweep re-check to step 7.

### Nits
- intent.md:74, 78, 83, 85, 89 cite "spec S13", but the spec ends at S10; those items are in
  decisions.md's accepted risks and follow-ups §14. intent.md:70 cites "spec, S5" for the log
  line, which is S4.
- Two design statements no longer match the code:
  - design.md:18, "`node.sh delete` today succeeds silently when the node is not running":
    node.sh:94-99 and 120-128 now print `not running` and exit 3.
  - design.md:125-127, confirm.sh "takes the paths of `flaminio-meta`, `flaminio-agent` and the
    disk tool … as variables": confirm.sh:21-26 honours them only under `FLAMINIO_SCRIPT_TEST=1`.
    Followed literally, the design would let ENV_VARS choose the binary run under sudo.

Reviewed commit: 24877eb4186721c81960bdf917665bbf89bbcdc0
