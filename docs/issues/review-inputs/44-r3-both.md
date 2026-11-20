# Plan review: issue #44, round 3 (incremental), design-code and design-security

Reviewed: git diff 0dc56f2..093f431 -- docs/issues/44/ (report.md excluded).

## Round-2 findings
- L1 fixed. Design refusal 2 uses (.alive // false), (.draining // false), (.labels // {} | has("flaminio.io/witness")); client prints EmitUnpopulated:false (main.go:501); proto has bool alive/draining; lab.sh:614 uses the same labels expression; fixtures are required in that shape (S12 row).
- L2 fixed. Matches status replies: "ok none" / "ok {v} /dev/ublkb{id}" (control.rs:104-107); valid_id exists (lab.sh:1633); malformed answer refuses, test row has "no metactl get recorded".
- L3 fixed. Refuse if id exists (NotFound as lab.sh:1757-1760), trap only after create success; test row "AlreadyExists: no delete, no detach". Nothing the trap restores (stopping MEMBER, attach) happens before create, so the order is sound.
- L4 fixed. T=$VM_A, MEMBER=$VM_B; key on fl-a (lab.sh:529); spec S12, design, decisions agree; "T=fl-a MEMBER=fl-b" no longer appears.
- L5 fixed. Spec names witnesses and the lapsed/draining node; design refusals 2 and 3 say what each covers.
- N1 fixed: Makefile lint list after :94 (lab-deletion-test is line 94), SHELL_SCRIPTS globs scripts/*/*.sh so shellcheck takes the new script. N2 fixed: Discard is 1+8+4+(4+4)=21 body + 4 prefix = 25 (frame.rs:1,279-284,165-168). N3 fixed: Digests are 1+4 bytes per Some (frame.rs:306-318); plan is intersected with source-mapped blocks (target.rs:1455-1465); 4160 MiB x 256 = 1 064 960 blocks, 5.08 MiB; 16384-4160 = 12 224 MiB of holes; 12 224 x 25 B = 0.29 MiB; x150 B = 1.8 MB, under 2 MiB. N4 fixed (design "Lab and docs"). N5 fixed: 300 s in spec S11, design, decisions; 8 + 2 + 22 = 32 MiB adds up.
- Figures agree in intent, spec, design, decisions: no stale 21 bytes, 1 048 576, 1800 s or "mapped block" remains (only intent.md:24, "every mapped block of every chunk", which is the whole-plan description and is correct).

## Findings of the revision

### L1 - Low: the check's helpers are not in lib.sh
The design says the script uses "lib.sh and existing lab command helpers", but valid_id, data_vms and metactl are lab.sh functions, and there is no scripts/checks/lib.sh (lib.sh is scripts/lib.sh). The other checks pull single functions out with `source <(sed -n '/^metactl()/,/^}/p' ... lab.sh)` (reconcile.sh:15, resync.sh:12). Fix: say dirty-resync.sh extracts metactl, valid_id and data_vms (and what they use, e.g. vmvar) the same way; the script test must exercise that extraction, since a missing helper fails only at run time.

### L2 - Low: refusal 3 does not say what a failing `metactl get V` does
A V that passes valid_id but whose `metactl get` errors (NotFound after a delete, or a transport error) is neither "has neither T nor MEMBER" nor a malformed status. Fix: any error from that get refuses; one test row.

## Nits
- N1: design "Lab and docs" ("Add lab procedure/help") and S14 ("command help matches the actual script") read as if lab.sh/make help change; the Files list says they do not. Say "lab.md procedure" only.
- N2: a `create` whose answer is lost "stops with the id named, deletes nothing": the volume may exist, so S12's "leaves the lab as found" is knowingly unmet. Say the message prints the `metactl delete`/cleanup command for an operator, as the fl-c case does.
- N3: say where the 300 s starts (member's data plane start, or the watcher's first line), so the script test has one boundary.

No new safety rule, wire, token or on-disk change; no injected bug owed.
