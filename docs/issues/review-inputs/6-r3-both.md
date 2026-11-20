# Issue #6 plan review, round 3 (incremental), design-code and design-security

## Earlier findings
- R2-C1: fixed. `RunEpochs<K>` is called by both `Engine::start_target` and the simulator's. Wiring matches `target::Bugs` (`#[cfg(test)]`, `sim::Bugs` field, set in `start_target` under `cfg(test)`). The shipped build has no bug field.
- R2-B1: fixed for the token, the 5 s poll and the S1 re-read clause. The control's direct-mode claim is not checked: R3-B1.
- R2-C2: fixed. `take_attach` with `serve(AttachRequest, &mpsc::Sender<String>)` matches `attach_and_serve(..., &reply, request)` (ublk.rs:809, 840) and the error reply at ublk.rs:814.
- R2-C3: fixed. See Nit 1.
- R2-N1: fixed.

### R3-B1 — Low
**`partscan.sh` does not verify direct mode, and its positive control can fail for other reasons.**
Evidence: design.md "Lab check", spec.md S1 test.
- `PRIVSEP=0` exists nowhere in `scripts/` (grep finds no `privsep`): #74 is not built. The check
  does not read how the node runs. Under `--privsep` `blockdev --rereadpt` fails, so the check
  fails at the control step. The design says the control shows direct mode, but it does not say
  what the failure message is. A reader would take "no p1 after re-read" for an invalid table or
  a broken S1.
- `blockdev --rereadpt` returns EBUSY while another process holds the device open. udev's
  probe holds it open just after the add, so the one call can fail on a correct node.

**Fix:** retry `--rereadpt` for the same 5 s, and run `udevadm settle` first. On failure, say:
"the re-read created no p1: the table is invalid, the node runs --privsep, or the device was
busy". Better, once #74 lands, read the node unit's command line for `--privsep` and fail up
front, naming the mode.

## Nits
1. docs/replication-protocol.md §9 (design Docs bullet): "Scripted scenarios ... reproduce the
   two split-brain cases" and "Only a failure that breaks linearizability or convergence counts"
   stay true only as written. The new scenario is neither a split-brain nor a lin/convergence
   catch. The bullet adds a sentence but does not say to reword those two. Add "reword the
   scenarios' intro and the mutation checks' counting rule" to it.

Reviewed commit: 8fde0909e4177fd7ceca4e4ce4dea45642b5315b
