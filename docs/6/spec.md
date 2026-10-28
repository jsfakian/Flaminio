# Issue #6: a faster first attach after a restart — specification

## Behaviour

**S1. No automatic partition scan** (once libublk accepts the flag; intent, "Dependency"). The
node adds its ublk device with `UBLK_F_NO_AUTO_PART_SCAN` when the running kernel advertises the
flag (`UBLK_U_CMD_GET_FEATURES`). Otherwise it adds the device as today, and logs once that the
kernel scans its partitions. The flag stops the automatic scan when the device starts: a
partition table inside a volume no longer creates partitions on the host when the volume is
attached. An explicit re-read (`blockdev --rereadpt`, `partprobe`) still does. Reading the
device works as before.

**S2. Time limits on an attach.** Kubelet runs NodeStageVolume under a 2-minute deadline, which
reaches the CSI node plugin through the gRPC context. Within it, the node plugin calls:
1. `Meta.Attach` on the metadata service, which tries each endpoint for up to 10 s: up to 30 s
   with three;
2. the data plane's control socket, `attach <volume> <token>`;
3. on a filesystem volume, `blkid`, `mkfs` if the volume is empty, and `mount`.

The data plane must answer (2) in time for its answer to reach kubelet. Today it waits up to
120 s, starting only after (1), so when it fails, kubelet sees only its own deadline, with no
cause. With this issue, the data plane answers every control command within 50 s. An attach
takes these steps:
- **Target start:** up to 30 s (was 60 s) for the metadata service to answer the target's
  start: the engine reads the volume and the nodes.
- **Adding the device:** up to 10 s, plus one more try, while a previous device with the same
  id goes away. This is unchanged.
- **Starting the device:** up to 10 s. libublk waits up to 3 s for the device's character
  device to appear. On a kernel that scans partitions after the device has started, as Linux
  7.0 does, starting does not wait for reads. On one that scans inside the start, the scan's
  first read can wait for a takeover: 5.5 s.

So the socket answers an attach within 50 s. When that limit passes, it answers `error: no
answer within 50 s` and withdraws the request: if the node has not begun it yet, it never
does. An attach already under way may finish later, and a retry then finds its device through
`status`. `delete` answers within 50 s too (was 60 s), and `detach` within 30 s, as now.

**S3. A timed-out attach leaves nothing behind.** When the target's start runs out of time, the
data plane drops the attach in the engine. An answer from the metadata service that comes later
is ignored, whether it carries data or a failure. It neither starts a target nor ends another
attach, so the next attach of the volume works, even when it comes before the late answer. Today such an answer starts a target
with no device behind it, and every later attach of the volume fails with "already attached"
until the node restarts.

**S4. The CSI node plugin's limit.** It waits up to 60 s for a control command (was 150 s),
unless the caller's deadline is earlier. Every call from kubelet has a deadline, so the 60 s is
a backstop for a data plane that never answers. It is also the limit for callers without a
deadline, such as the tombstone sweep's `delete`. It is above the data plane's 50 s, so the data
plane's own answer comes first and names the cause.

The budget, with three metadata endpoints: (1) 30 s, plus (2) 50 s, leaves 40 s of kubelet's
2 minutes for (3).

**S5. Unchanged.** How long an attach takes when nothing fails: under 0.1 s on the lab, from the
metadata service's attach to the control socket's answer. How long the first reads wait
afterwards. And `scripts/checks/reconcile.sh`, which passes.

## Tests

- **Go** (`control/csi/dataplane_test.go`):
  - With `Timeout` 50 ms and a socket that never answers, `Attach` fails within a second, and
    its error says no answer came.
  - With `Timeout` 10 s and a context deadline of 50 ms, it fails within a second too.
  - With `Timeout` zero, as `main.go` leaves it, a socket that answers `ok /dev/ublkb3` after
    200 ms gives `/dev/ublkb3`. The effective limit for zero is `DefaultControlTimeout`, which
    the test asserts is above 50 s.
- **Rust:**
  - **Control socket** (`control.rs`): with a control channel whose receiver is kept but never
    answers, and a limit of 100 ms, `attach` answers `error: no answer within 0.1 s`. The
    request it sent is then marked withdrawn.
  - **Limits:** a test asserts that `ATTACH_LIMIT` is `METADATA_LIMIT + ADD_DEVICE_LIMIT +
    START_LIMIT`, and is below the CSI node plugin's 60 s.
- **Engine** (`flaminio-node/tests/engine.rs`):
  - An attach is cancelled before the metadata service answers. The late answers start no
    target and give no `Attached`. A new attach of the volume then completes.
  - The same, with the new attach made before the late answers arrive, at a newer epoch: it
    completes once, at the newer epoch, and nothing fails.
  - The same again, with the dropped attach's late answers failures: the new attach still
    completes, and no `AttachFailed` comes.
- **Lab:** `scripts/checks/reconcile.sh` with zero copy passes. A timed attach, as in the
  design's measurements, with both replicas up and with one replica's node stopped, answers in
  under a second.
- **S1, once it lands:** a lab check saves a volume's first 4 KiB (its logical block) and
  writes a prepared 512-byte MBR, with one partition, over its start with `dd`. It detaches the
  volume, attaches it again, and checks that no `/dev/ublkbNp1` appears while the kernel
  advertises the flag. It then writes the saved 4 KiB back.
