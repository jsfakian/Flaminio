# Issue #15: authoritative device lifetime profile for CSI (#79)

Accepted with the plan, 2026-11-13: cross-issue R1 correction; not implemented. Historical legacy wire formats
stay unchanged. #79 must depend on this contract rather than infer it from current source.
Changed after approval, plan review round 1 (2026-11-13): DEVICE must be a ublk disk the kernel
names (R1-S3), the identity under #74's `--privsep` (R1-S1), and how tools act on the held object
(R1-C3); see decisions.md. #15 ships the daemon side and the plugin's device check; #79 the
conditional-detach client and its ownership (design, S9).

**Revision 5.** Revision 1 is commit `8af7aeb`, as accepted; revision 2 is `0020e98` (#15's plan
review round 1); revision 3 is `0a2b1fe` (round 2: #74's `checkUblkDevice` kept as #74 defines
it, #15's comparison with the reply beside it, one code for a device that fails the check);
revision 4 is `e46dd6b` (round 3: a second reply that differs is a change of state, not
`codes.Internal`); revision 5 is `95a1dba` (round 4: that state's code worded without "when first
seen"). #79 must pin `95a1dba` (R2-B7, R3-N2, R4-N1, R5-N1). Round 5 changed only this line.

## Additive local control commands

```text
device-info VOLUME
  -> ok none
  -> ok VOLUME starting BOOT_UUID INSTANCE GEN FIRST_EPOCH
  -> ok VOLUME running|stopping BOOT_UUID INSTANCE GEN FIRST_EPOCH DEVICE MAJ:MIN DISKSEQ
detach-device VOLUME BOOT_UUID INSTANCE GEN FIRST_EPOCH MAJ:MIN DISKSEQ
  -> ok detached | ok absent | error: VOLUME: identity: stale attachment
```

UUID is canonical host boot UUID; INSTANCE/GEN/FIRST_EPOCH/DISKSEQ are checked nonzero u64
values. DEVICE is `/dev/ublkb<N>`, matching `^/dev/ublkb(0|[1-9][0-9]{0,6})$` (#74 S6), and
`/sys/class/block/ublkb<N>/dev` must equal both MAJ:MIN and the opened fd's rdev: a reply that
names any other block device is refused before effects. Parse exact fields/arity without suffix
acceptance; no `|` is a literal field. Malformed/legacy identity fails before effects.
The tuple is `(volume,first_epoch,boot_uuid,instance,generation,rdev,diskseq)`; path is diagnostic.

The daemon caches FirstEpoch from its verified attach claims and obtains rdev/diskseq by fstat/
BLKGETDISKSEQ of its own held device fd, opened `O_RDONLY|O_NOFOLLOW|O_CLOEXEC`, before publishing
running; it checks `S_ISBLK` and that rdev equals `/sys/class/block/ublkb<N>/dev`. It keeps that
fd until this entry's final detach, including stopping. Under #74's `--privsep` the I/O process
opens no device path: it reads MAJ:MIN and DISKSEQ from `/sys/class/block/ublkb<N>/{dev,diskseq}`
after `start`, before publishing running; the plugin's own checks against the kernel make the
identity root's. Device-info reads one entry/lifetime under its lock;
it never reopens a path and reports the replacement as the old volume. Unknown legacy FirstEpoch
or unsupported diskseq refuses the strict profile. Compare boot/instance against this daemon.

Before effects, CSI requests device-info and runs #74's `checkUblkDevice(path)` as #74 defines it
(the path grammar, `Lstat` a block device, sysfs holding its rdev). Then, its own comparison: it
opens no-follow, and requires the reply's MAJ:MIN to equal both `/sys/class/block/ublkb<N>/dev`
and the fd's rdev (`fstat`, a block device), and the fd's diskseq to equal THAT expected reply's;
then it repeats device-info and compares the whole tuple. A failure of `checkUblkDevice` or of
the comparison is `codes.Internal`, the code #74 gives a device that is not the ublk disk named.
A second reply that differs is a change of state, with the state's code: stopping
`FailedPrecondition`; none, starting or another lifetime `Unavailable`. It retains the fd
through probing/formatting/mount/bind. Any missing/different/stopping identity refuses; an fd
opened on W at V's recycled path cannot become expected evidence for V. CSI's fd and #15's
held-device/no-reuse rules protect the validated lifetime; tools must operate on that held
object: by its path while the fd holds the device's ublk id, so that the path names it or nothing
(#15), or by the inherited fd, with conformance tested by #79. No fallback to plain status is
allowed for these effects once #79 is in; until then #15's plugin keeps one volume's legacy path
on a data plane without `device-info` (design, S9).

Detach-device under registry -> entry lock order finds the current entry and atomically compares
EVERY tuple field before changing its detach flag/routing its generation. Missing V returns
absent with no engine input. A replacement (including a newer V) returns stale and is unchanged.
A matched action captures that generation and waits only for it; delayed execution/ack cannot
look up and detach a later generation. Lost reply retry yields detached/absent for the old entry
or stale for a replacement; it never sends legacy detach as a retry fallback.

## Grammar and tests

Daemon dispatch validates every volume argument before registry/engine work: attach, legacy
and conditional detach, delete, status VOLUME and device-info. Use the exact metadata ID grammar
`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`; extra words/invalid bytes/overflow fail. Share Go/Rust ID
vectors and test direct sockets, not only the Go wrapper. Plain status/list have no volume arg.

Tests vary each tuple field and exercise V dying -> W reusing path, V reattach with new generation,
server state changing between the two replies, unsupported legacy profile and stale detach held
until replacement, and a reply naming another block device, such as a loop device, with that
device's own MAJ:MIN and diskseq. Require zero format/probe/mount/detach effects on mismatches.
Server tests assert replacement's state/target/device unchanged and no wrongly routed engine
input; successful exact detach and repeat-after-lost-reply complete. The plugin's refusals of a
device are asserted as `codes.Internal`, and a changed second reply as its state's code.
Runtime/kernel tests establish held-fd lifetime and #79's full disposable ext4/image lifecycle.
No such checks ran while drafting this contract.
