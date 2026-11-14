# Issue #16: Short-lived certificates, rotated — intent

[Issue #16](https://github.com/jsfakian/Flaminio/issues/16), follow-ups §7. This change is the
issue's step 2 (rotation without a restart), in every long-lived process, and what the lab needs
to prove it: certificates that last hours, renewed with nobody at the keyboard. Step 1 (the
operator holds the CA and issues on CSR after a TokenReview) waits for the operator, which does
not exist yet: issue #16 stays open after this change.

## Why

Every connection is mutual TLS, and ARCHITECTURE §15.2 wants 24-hour certificates, rotated.
Today a certificate lasts 30 days (`CERT_DAYS`, `scripts/lab.conf:104`), an operator reissues it
(`make pki-issue`), and no Flaminio process notices the new file.

1. **Every Flaminio process reads its certificate once, at start.**
   - **Go** (`pkg/pki/pki.go:285-310`): `ServerConfig` and `ClientConfig` load the pair and put
     it in `tls.Config.Certificates`. Their callers: the metadata service's listener and its
     etcd client (`meta/cmd/flaminio-meta/main.go:116`, `:121`), and the CSI driver's three
     roles (`control/csi/meta.go:33`).
   - **Rust:** the replication transport builds its two configurations once
     (`flaminio-net/src/tls.rs:77` `with_single_cert`, `:88` `with_client_auth_cert`;
     `flaminio-io/src/node.rs:295`, `:298`). The metadata client reads the PEM into strings and
     keeps one identity for its life (`node.rs:311`, `flaminio-metaclient/src/lib.rs:85-87`).
2. **A new file would not reach some of them anyway.**
   - The metadata service gets its files as systemd credentials, copied when the unit starts
     (`scripts/guest/meta.sh:21-27`).
   - The CSI node plugin mounts each file alone from the host (`deploy/csi/driver.yaml:169-173`):
     a file replaced by a rename stays the old one inside the pod.
   - The CSI controller and the failover controller read a Secret that `csi deploy` creates only
     when it is missing (`scripts/lab.sh:1003`). Nothing reissues it: after 30 days it needs a
     hand.
3. **etcd already reloads.** Its listener and its peers load the certificate and key from their
   files at every handshake (`go.etcd.io/etcd/client/pkg/v3/transport/listener.go:474`, `:493`,
   v3.7.3). It needs no code, only files that are never half written. Today they can be:
   `pki.sh install` writes the certificate in place (`scripts/guest/pki.sh:22`).
4. **Expiry is checked at the handshake only.** A connection that is open stays open. So with
   24-hour certificates and no reload, a cluster keeps running until something reconnects: a
   peer's restart, a partition, a failover. Then every new handshake of a process that started
   more than a day ago is refused, exactly when the cluster needs to recover.
5. **A restart is not a way to rotate.** A restarted data plane's replicas report whole chunks
   as uncertain (follow-ups §2), and it restarts only after its targets have moved away
   (follow-ups §9). Once a day, on every node, that is a drain and a resync per node.
6. **The key never changes.** `pki.sh csr` makes a key only if there is none
   (`scripts/guest/pki.sh:15`). A certificate is not secret: a server sends it to whoever
   connects. So whoever copied a key once uses every later certificate, and a short life bounds
   nothing.
7. **In the lab, almost nothing could present a service account token.** etcd, the metadata
   service and the data plane are systemd units on the VMs (`scripts/guest/etcd.sh`, `meta.sh`,
   `node.sh`); only the CSI driver runs in pods, and fl-c has no kubelet at all (docs/issues/14).
   Step 1's TokenReview covers none of them until the data plane runs in Kubernetes (issue #12).
   What the operator's CA does for a process outside Kubernetes is not written anywhere.
8. **There is no operator.** `control/` holds `agent`, `csi`, `drain` and `failover`.
   ARCHITECTURE names the operator (§3, §15.2); follow-ups §7 says step 1 is "blocked on the
   operator" and that `make pki-issue` is the path until then. Issue #43 (the operator, opened
   2026-11-12) plans it. Its issue reads: "2. CA bootstrap: the operator holds the internal CA.
   Issuing on CSR is #16." So issuing on CSR stays this issue's step 1, once that CA exists.
9. **The lab's CA cannot issue for less than a day.** `flaminio-ca sign --days` is a whole number
   (`control/cmd/flaminio-ca/main.go:85`). No test covers an expired certificate, in Go or in
   Rust.

## What we want

1. Every long-lived process uses a renewed certificate, on both sides of its connections,
   without a restart: the metadata service (its listener and its etcd client), the data plane
   (replication, both ways, and its metadata client), the CSI driver's three roles. etcd already
   does.
2. Replacing the files is all a renewal has to do, whoever does it: the lab today, the
   operator's CA later, cert-manager or SPIRE (§15.2). The files reach the process that reads
   them.
3. A rotation costs nothing: no connection closed, no session lost, no ISR or epoch change,
   nothing added to the I/O path. A bad or late renewal only ever refuses connections; a process
   recovers from an expired certificate without a restart.
4. The lab issues certificates that last hours or minutes, and renews all of them unattended,
   each with a new key made in its VM.
5. A lab check: many rotations under load with no restart; then, when asked, the week on
   24-hour certificates.
6. The metadata clients check that they reached the metadata service: its `meta` identity, not
   only the CA and the address. #74 hands this follow-up to this change: the `io` certificate
   carries the node's address and the server usage, so today `io.key` can answer as the
   metadata service at that address.

## Out of scope

- **Step 1: issuing on CSR** with the operator's CA, after a TokenReview. It waits for that CA,
  #43's step 2. The spec's open questions record what it must settle: where the CA comes from
  (#43's step 2), and how a node outside Kubernetes gets its first certificate and renews.
- **The trust anchors.** The CA bundles and the token key (`token.pub`) are still read once, at
  start; rotating a CA or the token key is another change. The lab's CAs last five years.
- **Revocation.** A short life is the revocation. Whether a connection may outlive its peer's
  certificate is an open question of the spec.
- **cert-manager and SPIRE.** This change keeps to what they deliver, files replaced in place,
  and wires neither.
- **Which identities exist,** their names, SANs and usages, the two CAs, and the caller rules
  (`meta/service/auth.go`). Only the metadata clients' check of the server's `meta` identity is
  added (What we want, 6).
- **What each VM holds beyond its own identities** (the token key, an etcd client certificate on
  fl-c): docs/issues/14 records it.
- **Other keys:** k3s's own certificates, WireGuard keys, encryption at rest.
- **kTLS** on the data path (§15.2) and a metrics endpoint: rotations are logged.
