# Issue #5: zero-copy local reads fail with EINVAL — intent

[Issue #5](https://github.com/jsfakian/Flaminio/issues/5), follow-ups §12.

## Why

With `--zero-copy`, the frontend reads the local replica straight into the pages of the ublk
request. Some of those reads fail with EINVAL. A failed local read gets no reply, so the target
waits for its request timeout (3 s), drops the local replica from the ISR, and resyncs it. On
`vol1`, right after an attach, one block of every chunk was readable only after 7.1 s, against
0.9 s without zero copy.

The lab shows what the request's pages have to do with it (design, "Root cause"). The ublk
device accepts direct I/O into buffers aligned to only 4 bytes. The disk under the store,
opened with `O_DIRECT`, needs 512. With zero copy, the request's pages go to the disk as they
are. The disk then refuses a read that the ublk device accepted. The reader in the lab check
was `dd iflag=direct` from Ubuntu's uutils coreutils, whose buffer is aligned only to 16 bytes.
That `dd` fails the same way on the raw disk. Attaching has nothing to do with it: any such
read fails, at any time, and every one costs a 3-second stall and a resync.

## What we want

1. A read that the ublk device accepts is served the same with zero copy as without: the same
   data, no failure, no stall. Zero copy is an optimisation. It must not change which requests
   the device can serve.
2. A local read that fails for any other reason costs the client no timeout. The read goes at
   once to another member of the chunk's ISR, and the local replica stays in the ISR.
3. The lab check for reconciliation proves it. With zero copy, it logs no failed local read, and
   every chunk is readable as fast as without zero copy.

## Out of scope

- Repairing a replica's bad extent in place, after a checksum mismatch or a device read error.
  Dropping the replica never did that either: a resync copies only the writes it missed.
- Remote replicas reporting a failed read. That needs a new protocol response; today a remote
  replica whose read fails gets dropped after the request timeout, as before.
- Verifying checksums with zero copy. Zero copy does not verify today, and still does not.
- Making the ublk device refuse misaligned direct I/O, so that it behaves like the raw disk.
  Design, "Decisions", explains why not.
