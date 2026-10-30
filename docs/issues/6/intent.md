# Issue #6: a faster first attach after a restart — intent

[Issue #6](https://github.com/jsfakian/Flaminio/issues/6), follow-ups §10.

## Why

Reconciling a volume after its replicas restarted used to take minutes: 160 s for 16 GiB on the
lab. Two measures were sized for that:
- **Partition scan:** the kernel's partition scan of a new ublk device, and udev's probe, read
  the device as soon as it exists. Those reads waited for the reconciliation, so turning the
  scan off was meant to stop it holding up the attach.
- **Time limits:** an attach through the CSI node plugin may take 150 s (`controlTimeout`), and
  the data plane gives one 120 s.

Follow-up item 2 fixed the cause: the same reconciliation now takes about 1 s. The lab shows the
attach no longer waits for it at all (design, "Measurements").
- **The attach answers in under 0.1 s.** The data plane answers once the target has started and
  the device is up. The partition scan no longer delays it on Linux 7.0, even when the first
  read has to wait 5.8 s for a takeover.
- **The limits now bound only failures,** such as a metadata service that does not answer, and
  they are in the wrong place for those. The data plane's 120 s starts only after the node
  plugin's own call to the metadata service, so a failing attach never answered within
  kubelet's 2-minute deadline on the CSI call. Kubelet saw only its deadline, with no cause,
  and the node plugin's lock, which serialises every volume operation on the node, was held
  until then.
- **A metadata answer that comes too late breaks the volume on that node.** When the data plane
  gives up on the metadata service, the engine still holds the attach. An answer that comes
  later starts a target with no device behind it, and every later attach of the volume there
  fails until the node restarts.
- **Partition scanning still matters, for another reason.** With it on, a partition table that
  a tenant writes inside a volume shows up on the host as partitions of the ublk device, and
  udev probes them. Turning off the automatic scan keeps them off the host when the volume is
  attached.

## What we want

1. Volumes are attached with partition scanning off, where the kernel supports it.
2. The time limits on an attach are sized for what an attach does now. A stuck attach fails
   within kubelet's deadline, and says why.
3. An attach that timed out leaves nothing behind, so kubelet's retry works.
4. `scripts/checks/reconcile.sh` still passes.

## Out of scope

- How long the first reads wait after an attach: for a takeover when a replica is
  unreachable, and for the reconciliation of the chunks they read. That is follow-ups §2 and
  issue #11.
- The CSI sidecars' timeouts (provisioner, attacher: 60 s). Those calls go to the metadata
  service, not to the node.

## Dependency

The data plane creates its ublk device through libublk. libublk 0.4.8, the latest release,
refuses `UBLK_F_NO_AUTO_PART_SCAN`, as does its main branch, so (1) needs a change upstream.
That change is
[ublk-org/libublk-rs#72](https://github.com/ublk-org/libublk-rs/pull/72), opened for this issue.
Until a libublk release includes it, this issue lands (2) and (3), and stays open for (1).
