# Issue #65: RDMA transport — intent

[Issue #65](https://github.com/jsfakian/Flaminio/issues/65), README roadmap phase 6,
ARCHITECTURE §1.1 and §16.1. This change comes before the issue's step 1: the measurement its
ADR needs, of what a replicated write's transport costs today and what another could save. It
builds no transport. Steps 1 to 3 wait for it: issue #65 stays open after this change.

## Why

The issue says that on fast networks the TCP and TLS stack is most of a replicated write's
latency and CPU cost. Nothing measures that, and the code shows other costs on the same path.

1. **No profile of a replicated write exists.** The RF 2 campaigns kept throughput, latency
   and the network's figures, not the CPU per thread or the profile that measurement rule 6
   asks for (`docs/performance-plan.md` §2; `docs/benchmarks/data/2026-10-17-writes-rf2-w41/`
   holds neither).
2. **In the lab the transport cannot be seen.** RF 2 over mutual TLS
   ([results](../../benchmarks/2026-10-17-writes-rf2-w41.md)):
   - small writes cost 5–8% against RF 1; 4K QD1 at p50 2.44 ms against raw's 2.10 ms;
   - a durable 4K write takes about 2 ms, the overlay's round trip 1.5–2.0 ms (0.44 ms
     between the hosts), and 1M runs at the 1 GbE link's 106 MiB/s;
   - a stack that costs tens of microseconds disappears in these. §16.2's target (round trip
     + device write + 30 µs) has never been measured, and the performance plan leaves RF 2
     against raw to "a faster interconnect (25 GbE or more, phase 6)".
3. **The transport as built** is `flaminio-net`: frames over TLS 1.3 by rustls in user space
   (`tls.rs:67-90`), on one network thread per node (`flaminio-node/src/net.rs:103-107`).
   kTLS, which §15.2 and ADR 0003 count on for the data path, is used nowhere in `dataplane/`.
4. **Much of the path is neither TCP nor TLS, and no transport removes it.** For one write:
   - **the sender** copies the payload into the encoded frame (`frame.rs:235`), and writes
     and flushes every frame alone (`session.rs:59-62`);
   - **the receiver** fills a zeroed `Vec` per frame (`session.rs:81-82`), copies the payload
     again when decoding (`frame.rs:604`), and a third time into an aligned buffer for
     `O_DIRECT` (`flaminio-uring/src/uring.rs:90-93`);
   - **a write and its reply change thread four times,** twice on each node: from the engine
     to the network thread through two channels (`net.rs:124`, `153-156`), and back through a
     mutex, a channel and an eventfd (`flaminio-io/src/node.rs:301-304`);
   - **all sessions of a node share that one thread,** and their encryption with it.

   §16.1's zero-copy send, receive into registered buffers and per-core connections are not
   built. RDMA needs registered buffers too: the same rework of the buffer path.
5. **Nothing separates the session from TLS over TCP.**
   - **What is free of it:** the frames, `check_request` and the token check. `Conn<S>` needs
     only a byte stream (`session.rs:42-85`).
   - **What is not:** `accept` and `connect` take a TLS acceptor and connector, and the peer's
     identity is read from the TLS session's certificates (`session.rs:103-113`, `183-202`).
     Identity exists only as the result of a TLS handshake.
   - **The network thread** takes a `TcpListener`, a `SocketAddr` and a `TcpStream`
     (`net.rs:53-70`, `205`, `266`), and a node registers one `ip:port`
     (`engine.rs:170-171`; `address`, `meta.proto`). A target cannot learn that a peer speaks
     a second transport.
   - **The refusal tests** (five, `flaminio-net/tests/session.rs`) all run through TLS over
     loopback TCP.
6. **RDMA has no TLS on the wire.** ARCHITECTURE §2 principle 5 (secure by default; turning
   protection off is explicit, and visible in status and metrics) and §15.1 (a network
   attacker on the storage network) apply. No ADR covers transport security yet.
7. **An RDMA library is new, unsafe code.** The workspace forbids `unsafe`
   (`dataplane/Cargo.toml:40`); only `flaminio-uring` lowers that. A binding is FFI to
   `rdma-core`, a C library at build and run time, beside a decoder of untrusted frames.
8. **The lab cannot show RDMA's gain.**
   - **Hardware:** two servers on a 1 GbE uplink (`eno1`), VMs on virtio behind NAT, joined
     by a WireGuard overlay (`docs/lab.md`, `scripts/lab.conf`). No RDMA device is recorded;
     the servers were not inspected for this intent.
   - **Soft-RoCE:** `rdma_rxe` (and soft-iWARP, `siw`) are in mainline well before ADR 0003's
     6.6 baseline; the guests run 7.0. Whether the guest image ships the modules is
     unchecked. RoCE runs over Ethernet and wg0 is a layer-3 tunnel, so it is likely to
     connect only VMs of one server (fl-a, fl-c, fl-w): also unchecked.
   - **What it proves:** correctness only. Soft-RoCE is slower than TCP, so the issue's "done
     when" cannot be met in the lab.
9. **kTLS competes for the same gain** with far less change (§15.2): it keeps TLS on the wire,
   the identities and every refusal test. Which of the two is worth building first is a
   question of numbers.

## What we want

1. **What a replicated write costs on today's transport,** on the target and on the peer: CPU
   per I/O (§16.2's unit), per thread, and its share in TLS encryption, the kernel's TCP path,
   Flaminio's own copies and allocations, and the thread hand-offs. For 4K at QD1 and QD32,
   and 1M.
2. **The same with the link and the disk taken out:** the transport alone, on one machine,
   repeatable without the lab. Against today's transport, two bounds: without encryption
   (the most kTLS with offload could reach), and without the kernel's network path (the most
   any transport could reach). What remains is Flaminio's own, and no transport saves it.
3. **A lab profile of an RF 2 write campaign,** by the performance plan's rules, with the CPU
   per thread and the profile of the target and of a peer kept with the results.
4. **A verdict, recorded** in `docs/benchmarks/` and the performance plan: whether the issue's
   "Why" holds, and how the possible saving divides between Flaminio's own path, kTLS and
   RDMA, in CPU per I/O and in latency at QD1. The issue's "Why" is corrected to match.
5. **The issue's dependency answered,** in `docs/lab.md`: whether either server has an
   RDMA-capable device, and whether soft-RoCE or soft-iWARP loads in the guests, and between
   which VMs it connects.

## Out of scope

- **Step 1: the ADR.** The RDMA library; how peers are authenticated and data protected
  without TLS on the wire; `unsafe` and a C library in the transport. It needs this change's
  numbers, and decisions that are a human's: the spec lists them under Open questions.
- **Step 2: a second transport** under the framing, with the same identity and token checks,
  and the refusal tests on both. It needs the seam of Why 5, and a way for a node to
  advertise the transport.
- **Step 3: fallback to TCP** per peer.
- **The issue's "done when":** latency and CPU per I/O on RDMA-capable hardware against TCP
  with TLS. It needs hardware the lab does not have.
- **kTLS on the data path** (§15.2). This change measures its bound; building it is not this
  issue's.
- **Fixing what the measurement finds:** the copies, the per-frame writes, the hand-offs,
  per-core connections (§16.1). Each is a change of its own, judged on these numbers.
- **The session's rules and the wire format:** unchanged. No product behaviour changes.
- **A faster lab network.** The lab's link stays 1 GbE.
