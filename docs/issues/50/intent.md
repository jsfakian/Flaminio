# Issue #50: Encryption at rest with a KMS (Vault, KMIP) — intent

[Issue #50](https://github.com/jsfakian/Flaminio/issues/50), README roadmap phase 3,
ARCHITECTURE §12, §15.1, §15.4 and §20.4. This change is the issue's step 1 (the cipher in the
target), with step 3 (the key only in the target's memory) and the part of step 6 that any keyed
attach needs (fail closed without the key). The KMS providers, rekey, crypto-erase and
authenticated checksums come after it: issue #50 stays open after this change.

## Why

ARCHITECTURE describes what is wanted. The code has none of it.

1. **Nothing encrypts.** No cipher, key or KMS code in `dataplane/crates`, `meta`, `control` or
   `pkg`. A volume's record has no encryption field (`Header`, `meta/domain/domain.go:37`;
   `Volume` and `CreateVolumeRequest`, `meta.proto:117`, `:129`). The CSI controller refuses
   the README's `encryption: "true"` as an unknown parameter (`parseParams`,
   `control/csi/controller.go:157`). The data plane's one crypto library is `ring`, under
   rustls; it has no XTS.
2. **Replicas store what the application wrote.** A write's data is copied once from the ublk
   request (`ublk.rs:1263`, `:1444`), handed to the engine as `Io::Write`
   (`flaminio-node/src/engine.rs:181`), sliced per chunk (`engine.rs:1739`), and the same bytes
   go to the local store and to every replica (`Frame::Write`, `flaminio-net/src/frame.rs:42`).
   - **Checksums are each replica's, of the bytes it receives** (`block_checksums`,
     `flaminio-store/src/store.rs:1781`, `:1795`, `:1815`); the write frame carries none. §12
     has the target compute them: it does not.
   - So data encrypted before `Io::Write` is ciphertext to everything below: stores, checksums,
     digests, repairs, resync and moves. Repair and resync copies go from replica to replica
     inside the engine and never reach the application.
3. **Reads reach the application three ways, and one never passes through the process.**
   - The copy path reads the local replica into the tag's buffer and verifies it there
     (`fast_read_copy`, `ublk.rs:1629`; `local_read`, `ublk.rs:1293`). Pieces from other
     replicas are copied in (`ublk.rs:1244`, `:1400`). Both can be decrypted in place.
   - With `--zero-copy`, a per-node flag (`node.rs:58`), the disk reads land in the request's
     pages (`ReadFixed`, `zc_read_plan`, `ublk.rs:1508`): "the data never passes through this
     process" (`ublk.rs:1464`). On an encrypted volume the application would read ciphertext.
   - A way through the process exists: the staged read, disk to staging slot to request
     (`stage_device_read`, `ublk.rs:1559`). It costs the copy that zero copy removed. On fl-b,
     4 vCPUs, the copying, verifying path reads 1M sequential at 58% of raw and 4K QD32 at 87%;
     zero copy reads them at 100% and 98% (`docs/performance-plan.md`, status table).
     `make bench-reads` runs zero copy by default.
4. **An unwritten block is stored as zeros, and only its content says so.** Unmapped ranges
   read as zeros (`store.rs:1865`, `:1980`), and their digest is the zero block's CRC
   (`store.rs:1284`, follow-ups §2). The same zeros are stored as data in three cases: the rest
   of a cluster written in part (`store.rs:1800`), a repair of an unwritten block
   (replication-protocol §6, step 3), and a resync's `Discard` of a hole. AES-XTS decrypts a
   zero block to noise. The lab checks that unwritten ranges read zeros (follow-ups §3), and
   the CSI node plugin probes a new volume with `blkid` before `mkfs` (`control/csi/mount.go:40`).
   - Application discards do not exist yet: the engine's device advertises none
     (`ublk.rs:950`) and completes one as a no-op (`ublk.rs:1279`, `:1454`).
5. **No path carries a key, and the record is readable by all.** An attach gives a target its
   epoch and capability token (`Attach`, `meta/service/service.go:695`). The CSI node plugin
   makes the call as the node's agent and passes the token to the data plane as a line on the
   control socket (`control/csi/node.go:70`, `flaminio-io/src/control.rs:5`). `GetVolume` and
   `ListVolumes` answer any authenticated identity (`meta/service/auth.go:79`); the data
   plane's own identity may otherwise call only `RegisterNode`, `NodeHeartbeat` and `ChangeIsr`.
   §15.1's claim, that a replica node sees only ciphertext, holds only if a node that is not
   the target cannot get the key.
6. **A key in memory dies with the process.** A target never restarts under its epoch: after a
   data-plane restart the node plugin attaches again (`OnlyIfTarget`, `node.go:70`), and a
   failover is an attach on another node (`control/csi/controller.go:281`). Each needs the key
   again. §1.1: whatever gives the key is sometimes unreachable.
7. **Nothing protects memory.** The workspace forbids `unsafe` (`dataplane/Cargo.toml:40`),
   which `mlock` and `MADV_DONTDUMP` need. No core-dump limit is set by the data plane or by
   its lab unit (`scripts/guest/node.sh:65`).
8. **The cost is unknown.** CRC-32C verification runs at about 8.5 GB/s per core on fl-b and
   took about 44% of the queue thread on 1M reads; checksums and the copy limit a thread to
   about 4 GB/s (`docs/performance-plan.md`). AES-256-XTS adds to that on every read and write.
   Nothing here has measured it, nor whether the lab guests see the hosts' AES instructions.
9. **The lab has no key server:** no Vault, no KMIP, and no step that would start one.

## What we want

1. An ADR, accepted before the design, for what this step fixes for the later ones: the cipher
   and its library; where a volume's key is kept, who releases it and to whom; the key-source
   interface that Vault and KMIP will implement.
2. A volume is created encrypted or not, by a StorageClass parameter or `flaminio-meta create`.
   Its record says which, and never changes. Unencrypted volumes behave and perform as today.
3. The target of an encrypted volume encrypts every application write before it leaves the
   process, and decrypts every read: AES-256-XTS, a key per volume, the volume-logical block
   number as tweak. Every replica stores the same ciphertext. Checksums, digests,
   reconciliation, resync and moves work on it unchanged, and a target on another node reads
   the same data.
4. A block never written reads as zeros through an encrypted volume, and its digest stays the
   zero block's on every replica.
5. No node setting makes an encrypted volume return ciphertext or store plaintext. Under
   `--zero-copy` its local reads pass through the process, and the node says so.
6. The key reaches only the data plane of the node the metadata service names the volume's
   target. There it lives in memory only: locked, zeroized when the volume detaches, out of
   core dumps, never logged. It is in the clear in no record, on no disk and on no command line.
7. An attach of an encrypted volume without its key fails closed, and says why: at the first
   attach, at a failover and after a data-plane restart. A key that is not the volume's is
   refused, not used. An attached volume makes no key call on its I/O path: it keeps serving
   while the key source is away.
8. A first key source that needs no server outside the cluster, flagged as the weaker kind
   (§15.4), so that the step runs in the lab and in tests.
9. A lab check, and the cost measured. A known pattern written through an encrypted volume is
   found on no replica disk; the reconcile and resync checks pass on it; with the key source
   away an attach fails, and works again when it returns. Reads and writes are measured
   against an unencrypted volume by the performance plan's rules.

## Out of scope

- **Step 2: the KMS providers.** Vault Transit and KMIP behind the ADR's interface, the
  `KMSProvider` resource, the Kubernetes Secret KEK. The lab then needs a Vault-compatible
  server and a KMIP server on a VM, and a check that stops them.
- **Step 4: rekey and DEK rotation.** Re-wrapping a key; rewriting a volume under a new one.
- **Step 5: crypto-erase** on volume deletion and on disk release. Deleting an encrypted volume
  works as today: replicas deleted, tombstone confirmed.
- **Step 7: authenticated checksums** (§12, §20.4; format incompat bit 1 is reserved). Until
  then a compromised replica node can alter or replay ciphertext blocks unseen: CRC-32C is not
  keyed. This step keeps data from being read, not from being changed.
- **Encrypting an existing volume,** or decrypting one.
- **Zero-copy reads of encrypted volumes.** They need the request's pages mapped, or the
  cipher in the kernel. Until then encrypted reads take the copying path's cost (Why 3).
- **Application discards.** When the device advertises them, a discarded block follows
  outcome 4.
- **Keys beyond the live volume:** snapshots and backups as ciphertext with their key
  reference (§14), the on-disk volume descriptor (page type 7, reserved), metadata disaster
  recovery (§11.5).
- **Audit events for key operations** (§15.3) in #73's event chain: a follow-up, recorded in
  `docs/followups.md` as #73 asks; until then each release is logged (S7). #73's policy on
  unencrypted volumes is #73's; this step only gives it the volume's choice (S1).
- **Encryption on by default** (§2, principle 5). It waits for a KMS provider.
- **TPM-sealed keys** (phase 4), **cloud KMS** (phase 5), **OPAL drives** (phase 6).
- **The MVP's `serve` mode** (`volume.rs`): unencrypted only.
