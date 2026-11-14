# Issue #70: CSI volume expansion and XFS support — intent

[Issue #70](https://github.com/jsfakian/Flaminio/issues/70), architecture §17.1.
Accepted by the user on 2026-11-13 ([decisions](decisions.md)); not implemented.

## Why

- **The advertised roadmap has no implementation.** `control/csi/controller.go:88–99` omits
  controller expansion; `node.go:41–52` omits node expansion; embedded unimplemented servers
  handle both RPCs. `controller.go:104–128` accepts filesystem volumes only as ext4.
  `deploy/csi/driver.yaml` has no external resizer and sets `allowVolumeExpansion: false` (`:235`).
- **Capacity is fixed at attach.** `flaminio-node/src/engine.rs:1596–1679` builds one target
  with a fixed size and chunk vector; `flaminio-repl/src/target.rs:2510` indexes it with every
  chunk of a metadata view, so a longer view panics a running target.
  `flaminio-io/src/ublk.rs:948–958` sets device capacity only at creation.
- **Replicas also assume one size.** `flaminio-node/src/replica/mod.rs:213–220` refuses a
  changed size before the epoch check. Recovery takes a volume's size from the first chunk
  descriptor (`:137–144`); descriptors keep creation-time `volume_size` (`flaminio-store/src/store.rs:1189–1218`).
- **Metadata has creation machinery but no growth transition.** `meta/service/service.go:372–476`
  places and writes a whole new volume; nothing appends chunks to an existing one.
- **A live append does not fit the session model.** A replica creates chunks only on `Hello`,
  and a new Hello replaces the target's session, which the target counts as a member failure
  for every chunk it holds there (`target.rs:1214–1309`). Growing a running target would drop
  its prefix replicas from their ISRs. Linux 6.6 also lacks ublk `UPDATE_SIZE`.

## This step

This plan is the offline step for raw block and ext4 volumes: issue steps 1 and 2 with the
offline path, step 3 for ext4, and step 4's tests and lab check for raw block and ext4.

- **Done when, here:** block and ext4 PVCs grow from a resize request while unpublished, keep
  their data across restart and failover, use the added space, retry idempotently, and refuse
  to shrink. The plugin advertises offline expansion only, and the docs say so.
- **Stays open (follow-ups):** online resize (ublk `UPDATE_SIZE`, and a running target taking
  new chunks without a member-failure Hello); XFS stage, publish and growth (an XFS profile in
  #79's initialization contract, and `xfsprogs` in the image). The issue closes only when XFS
  is done. Changed after approval (rounds 4 to 6): charging #72's capacity ledger and #73's
  quota is no longer a follow-up; each growth step charges both, and this plan lands with or
  after #72 (spec S2).

## What we want

1. Retry-safe metadata growth that appends whole chunks placed by the volume's own rules,
   never changes an existing chunk, never shrinks, and exposes only complete committed sizes.
2. PVC-driven controller and node expansion for raw block and ext4, offline: the device takes
   the new size at its next attach, and ext4 grows after it.
3. New chunks are served only by a target attached after they exist, through the existing
   fencing, ISR and reconciliation rules. Growth adds no metadata round trip to I/O.
4. Tests and a lab check proving old and added data for both modes.

## Out of scope

- Shrinking; changing chunk size, RF or minISR; #72's ledger and #73's quota themselves (growth
  charges them); snapshots and clones; disk-only metadata recovery (#54).
- Online resize and XFS (later steps of this issue, above).
- Replacing #18's recorded placement, #79's mount ownership, #80's geometry admission, #77's
  coherent reads or #78's placement gate: this step uses them as prerequisites.
- Operator implementation (#43); running shared lab or privileged checks during drafting.
