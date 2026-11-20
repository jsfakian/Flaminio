# Issue #57: TPM-sealed keys — intent

[Issue #57](https://github.com/jsfakian/Flaminio/issues/57), README roadmap phase 4,
ARCHITECTURE §1.2, §15.1 and §15.4. It builds on
[issue #50](https://github.com/jsfakian/Flaminio/issues/50) (encryption at rest), which is
planned, not built: this intent assumes #50's first step done, and its ADR accepted. This change
is the issue's step 1 on the key source #50 builds first: that source's key sealed in a TPM,
with the refusals and the way back that make a sealed key safe to use. The `KMSProvider` kind,
step 2 on an edge cluster and step 3's planned procedures come after it: issue #57 stays open
after this change.

## Why

1. **#50's first key source gives the key to whoever has the node's disk.** It is to be a
   static key in a file on the node, flagged as weaker (#50, outcome 8). It already needs no
   network at boot. What it lacks is a binding to the machine: §15.1's stolen disk is covered
   only while the stolen disk is a replica's. On a 1-node edge cluster (§1.2) the key file,
   the metadata store (`/var/lib/flaminio-etcd`, `scripts/guest/etcd.sh:11`) and the replicas
   are in one box, "physically exposed" (§1.1).
2. **What "sealed in the node's TPM" means follows #50's current plan decision: who releases a
   volume's key.**
   - **(a) The metadata service releases it to the target** (#50's recommendation). The key
     that wraps volume keys is the metadata service's. "The node" is each node that runs a
     metadata instance, and each seals its own copy of the same key. A failover gets the
     volume's key from any instance; a data node needs no TPM.
   - **(b) Each data plane asks the key source itself.** The wrapping key is the data node's
     own. A volume's key is then wrapped once per node that may become its target, and a
     failover to a node without a copy cannot unlock the volume.
   - **(c) A cache, under either.** A node seals the volume keys it was once given, to attach
     again while a remote KMS is away. The KMS stays the authority; a failover to a node that
     never attached the volume still needs it, and rekey and crypto-erase must reach every
     cache.
   - This intent follows (a), now specified by #50's reviewed static-source contract. Under (b) the step moves to the
     data plane and outcome 6 becomes per-node copies; (c) is a later step under either.
3. **Every long-lived secret is a plain file, guarded by its mode.** Identity keys are made in
   the VM as unencrypted PKCS #8 in a root-only directory (`scripts/guest/pki.sh:10`, `:16`).
   The token key is copied to every VM (`scripts/lab.sh:579`), the WireGuard key is a file
   (`scripts/guest/wg.sh:14`). The metadata service gets its keys as systemd credentials that
   are copies of those files (`LoadCredential=`, `scripts/guest/meta.sh:21`) and reads them
   by path (`loadEd25519`, `meta/cmd/flaminio-meta/main.go:165`). The data plane reads
   `io.key` as root (`flaminio-io/src/node.rs:280`); the CSI node plugin mounts `agent.key`
   from the host (`deploy/csi/driver.yaml:171`). A TPM-sealed key would be the first secret
   bound to hardware.
4. **Nothing can reach a TPM.** No TPM code or library in Go or Rust.
   - The metadata service's unit hides devices (`PrivateDevices=yes`, `meta.sh:41`), and its
     binary is built without cgo (`scripts/lab.sh:504`): a binding to the C TPM stack would
     change both.
   - The Rust workspace forbids `unsafe` outside `flaminio-uring` (`dataplane/Cargo.toml:40`),
     and §15.5 confines it to io_uring, ublk and crypto FFI. That bears on (b) and (c).
   - Either way a TPM library is a new dependency, under the dependency policy
     (`dataplane/deny.toml`, `govulncheck`).
5. **A sealed key fails where a file does not,** and nothing planned gives a way back.
   - **A PCR change:** a firmware or kernel update changes what the boot measures. The key no
     longer opens.
   - **Another machine:** a replaced board or TPM, or the disks moved to a spare node.
   - **A dead node:** its sealed copy dies with it. With several metadata instances the others
     hold the key. A 1-node cluster has one copy.
   - #50 makes an attach without its key fail closed. So each of these is an outage of every
     encrypted volume, and on one node a permanent one, unless a copy exists off the node.
6. **A key that was once on the disk may still be there.** The lab's one way to deliver a
   secret writes it to the VM's root disk (`put_file`, `scripts/lab.sh:556`, `pki.sh:27`).
   Sealing a key and deleting its file leaves the blocks.
7. **The lab has no TPM.** The VMs' definition has none, and no UEFI firmware or Secure Boot
   (`scripts/templates/libvirt-domain.xml`). `host-prep` installs no software TPM
   (`scripts/host/prep.sh:13`), and no script gives the CI runner one.
   - A software TPM (`swtpm`) attached to a VM proves the flow: seal, unseal at boot, refuse.
     It does not prove the binding: its state is a file on the VM's host.
   - What the lab VMs' firmware and boot loader measure into which PCRs is unknown.
   - `vm-redefine` keeps a VM's UUID (`cmd_redefine`, `scripts/lab.sh:89`), so a device can
     be added to the lab's VMs without creating them again.

## What we want

1. An ADR proposal, with historical plan approval recorded separately from review revisions: who talks to the TPM and with what
   library; the PCRs a key is bound to by default; the sealed form and where it rests; the
   form of the recovery copy.
2. On a node with a TPM 2.0, an operator seals the key of #50's static source into that TPM,
   bound to chosen PCRs. The node then keeps only the sealed form. The key is never written in
   the clear to the node's disks, to a command line or to a log.
3. The holder unseals the key at start, with no call outside the node, and then serves as #50's
   static source does. No attach and no I/O waits on the TPM.
4. The sealed form opens only in the TPM that sealed it, and only while the bound PCRs hold the
   values they had at sealing. Copied to another machine with the node's disks, it gives
   nothing.
5. A holder that cannot unseal releases no key, and says why: no TPM, another TPM, which PCR
   differs, a damaged sealed form. It never falls back to a key file. Attaches of encrypted
   volumes then fail as #50 specifies; unencrypted volumes and volumes already attached are
   not affected.
6. With several holders, each seals its own copy of the same key. A volume's key can be had
   through any of them that can unseal, so a failover does not depend on one node's TPM. A
   holder that cannot unseal answers Unavailable so callers move on to the next (`metaCall`,
   `control/csi/meta.go:64`), and changes no record.
7. Sealing is never the only way back to the key. A recovery copy exists off the node before
   the key is used, and sealing again from it ends every lockout of outcome 5: a PCR change, a
   replaced TPM, a replaced node. The refusal names the procedure. A cluster with one holder
   is told that the recovery copy is its only other one.
8. The node reports its key source as TPM-sealed, with the PCRs it is bound to. The unsealed
   file source stays flagged as weaker.
9. Tests with a software TPM that need no lab, and a lab check on VMs with one: the key
   unseals after a reboot and an encrypted volume attaches; under another TPM, and after a
   changed PCR, it does not, and sealing again from the recovery copy restores it; the key is
   absent from the known secret write paths; scans do not certify historical media erasure.

## Out of scope

- **The rest of step 1: the `KMSProvider` kind.** #50's step 2 brings the resource. Until then
  the sealed source is chosen as #50's static source is.
- **Step 2 on an edge cluster.** The issue's "done when" needs a 1-node cluster (issue #56):
  in the lab's three-member cluster a node rebooted without a network has no metadata quorum,
  so it attaches nothing, whatever keys it holds. This step shows only that unsealing calls
  nothing outside the node.
- **Step 3, a planned PCR change.** Sealing ahead of a firmware or kernel update to the values
  the next boot will measure, or a signed policy that accepts them. Until then every such
  update is a lockout, ended by outcome 7.
- **Step 3, recovery for a fleet.** A recovery copy wrapped for a central site, and its
  rotation (§1.2, fleet operation). Here the recovery copy is the operator's to keep.
- **Readings (b) and (c)** of Why 2: per-node wrapped volume keys, and a sealed cache of keys
  released by a remote KMS.
- **Which instances hold the key with 2 nodes and a witness,** above all a witness reached
  over the WAN: issue #56.
- **A stolen whole node.** A key bound to PCRs alone opens for anyone who boots the machine
  unchanged. What then guards it is the operating system's access control, and Secure Boot
  keeping the boot unchanged. A PIN, or a release bound to the network, contradicts an
  unattended boot. The issue's "done when" asks only that a moved disk cannot be read.
- **Remote attestation:** a node proving its boot state to the cluster before it is given
  anything.
- **The node's other secrets.** The token key, the identity keys, the WireGuard key and etcd's
  data stay plain files. A stolen disk still yields the node's identities.
- **The binding on real hardware.** A hardware TPM on an edge board, and arm64: the lab's
  software TPM proves neither.
- **Rekey** (#50's step 4): a new key sealed again on every holder.
- **TPM 1.2,** and disk-level encryption by the operating system or the drive (OPAL, phase 6).
