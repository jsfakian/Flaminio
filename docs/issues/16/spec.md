# Issue #16: Short-lived certificates, rotated — specification

Step 2 of the issue, and the lab's renewal ([intent](intent.md)). Settled choices and accepted
risks: [decisions](decisions.md). How: [design](design.md).

## Terms

- **Pair:** a certificate (with its chain) and its private key, in two files.
- **Identity:** the pair's SPIFFE ID, for example `spiffe://flaminio-lab/flaminio/node/fl-b/io`.
- **Long-lived process:** etcd; `flaminio-meta serve`; `flaminio-io node`; `flaminio-csi` as
  controller, node plugin and failover controller. Commands that run and exit (`flaminio-meta`'s
  client, `etcdctl`) read their files at each run, and are not concerned.
- **Life:** from a certificate's `NotBefore` to its `NotAfter`. **Renewal:** a new pair for the
  same identity. **Installed:** in place at the paths the process reads.
- **Renewer:** whatever installs renewed pairs. In this step the lab's renewal (S10); later the
  operator's CA, cert-manager or SPIRE.
- **Picking up:** from then on, every handshake the process starts or answers presents the new
  pair.

## The invariant

At every handshake, a long-lived process presents the newest usable pair installed for the
identity it started with, and verifies its peer exactly as it would after a restart. A rotation
never changes who a process is or whom it accepts. A renewal that fails, is late or is wrong can
only make handshakes fail; it never makes one succeed that would have failed. Write safety stays
where it is: epochs, fencing and capability tokens (ARCHITECTURE §2, §15.2).

## Behaviour

### Picking up a renewed pair

**S1. Every long-lived process picks up a renewed pair without a restart,** within a minute of
its installation. Replacing the files is enough: no signal, no command, no knowledge of who
renewed.
- **The metadata service:** its listener, and its client of etcd.
- **The data plane:** replication, accepting and connecting, and its client of the metadata
  service. **The CSI driver,** in its three roles: its client of the metadata service.
- **etcd:** as today; it reads its pair at every handshake.

**S2. Both sides, reconnections included.** A client that reconnects, after its peer restarted or
a partition healed, presents the pair installed by then, not the one it started with.

**S3. A rotation disturbs nothing.**
- No open connection or replication session is closed, and none is opened, because of it.
- No ISR, epoch or lease changes; no I/O waits.
- Nothing is added to the steady-state I/O path: no lock, no file read, no metadata round trip.

**S4. A pair that cannot be used is not used.** The process keeps the pair it has, says why once
for each such pair, and tries again. It never exits for it. A certificate and its key are always
taken together. Unusable:
- a certificate and a key that do not match, or either one missing, unreadable or cut short;
- a certificate that is not valid yet, or has expired;
- a certificate with another identity than the one the process started with.

**S5. An expired pair.** A process whose pair expired with no renewal keeps running and keeps its
open connections. Its new handshakes fail, at its peers. Once a usable pair is installed, it
recovers without a restart (S1).

**S6. Checks are unchanged,** on every handshake, before and after any rotation: TLS 1.3, a
client certificate both ways, the CA bundle, the trust domain, each metadata method's caller
rule, each replication session's capability token. No failed reload falls back to a weaker mode.
A peer's expired certificate is refused, by the Go and the Rust sides alike. The CA bundles and
the token key are read at start, as today. **No TLS session is resumed,** by either side, in Go
or in Rust: a resumed handshake sends no certificate, so it would carry its first handshake's
pair past a renewal (S2) and, in rustls, past its expiry (S5).

**S7. Starting is unchanged.** A process with no readable pair at start does not start.

**S8. Each process logs** every pair it picks up (identity, serial, `NotAfter`), every refusal of
S4, and a warning while less than a third of the life of the pair in use remains.

### Installing

**S9. The files reach the process, whole.**
- **Never half a file.** The two files change together; a reader that still catches one of each
  refuses them (S4) and reads again.
- **No copy made at start:** the metadata service reads files that a renewal replaces. It stays
  sandboxed and unprivileged, and its keys stay unreadable to other users.
- **No mount that keeps the old file:** the CSI node plugin sees a replaced file; it still sees
  its node's agent identity only.
- **The CSI controller's and the failover controller's pair,** kept in a Secret, is renewed there.
- **A renewed key keeps what its reader needs:** the owner, group and mode of the key it
  replaces (its directory's default for a first one), set before it is in place. **So does its
  directory:** an existing one keeps its owner, group and mode; a directory gets its default
  only when it is created.

### The lab's renewal

It stands in for step 1's issuer until the operator exists.

**S10. Renewing unattended.** A lab command renews every identity that `pki issue` and
`csi deploy` issue: on fl-a, fl-b, fl-w and fl-c, the administrator's and the CSI controller's
included.
- **When:** a pair with less than half its life left is renewed; any other is left alone.
- **On a schedule,** with nobody at the keyboard. The CAs' keys stay where they are (question 4).
- **A new key each time,** made in the VM and never leaving it. The old key is gone once the new
  pair is installed.
- **It restarts nothing:** no service, no pod.
- **A VM it cannot reach** does not stop the others. The run then fails, naming the VM and the
  identities, and the next run tries again.

**S11. A life in hours or minutes.** The lab's CA signs for a life given in hours or minutes (at
least 10 minutes), as well as days. The default stays 30 days (question 5).

**S12. Nodes outside Kubernetes.** Nothing in S1–S9 depends on Kubernetes, nor on who issued a
pair. In the lab, etcd, the metadata service and the data plane are host services on every VM,
and fl-c has no kubelet: the lab's renewal serves them all alike. Step 1's answer: question 2.

**S13. Unchanged.** `make pki-issue` by hand, after which nothing needs a restart any more; the
identities, their SANs and usages, the two CAs, and which VM holds which identity; the metadata
API, the replication protocol, the on-disk format.

**S14. Docs.** `docs/lab.md`: the renewal and its schedule, the life setting, what a process logs at
a rotation, what to do about an expired certificate (renew; no restart). Follow-ups §7: step 2 done,
with the lab check's result; the settled answers to questions 1 and 2; step 1 open. #43's issue
reads "2. CA bootstrap: the operator holds the internal CA. Issuing on CSR is #16.": #43's step 2
bootstraps the CA, and issuing on CSR, with its TokenReview and question 2's answer, stays this
issue's step 1, built once that CA exists. ARCHITECTURE §15.2: a renewer replaces files and every
process picks them up, with no TLS session resumed; a metadata client checks the server's `meta`
identity (S15); and question 2's answer: an administrator issues a node's first pairs by hand, the
node renews by presenting its current, valid certificate, a node down for longer than a life is
issued by hand again, and the CA stops renewing a removed node. The Makefile's help.

### The metadata service's identity

**S15. A metadata client checks that it reached the metadata service** (#74's follow-up). The
data plane's client and the Go clients (the CSI driver's three roles, `flaminio-meta`'s client)
accept a server only if its leaf's single URI SAN is a node's `meta` identity,
`spiffe://<td>/flaminio/node/<n>/meta`, besides the CA and the address as today. `<td>` is the
client's trust domain: the data plane's `--trust-domain`; for the Go clients, the trust domain of
their own certificate (`flaminio-meta`'s `--trust-domain` serves `confirm` only, and the check
does not use it). Any node's `meta`: a client does not know which
node is at an address. A server with another identity, a node's `io` certificate at the right
address for one, is refused, and the call goes to the next endpoint.

## Tests

- **S1, S2, S4–S7, Go:** over real mutual TLS on loopback (`pkg/pki`): a renewal of each side;
  every unusable pair of S4 keeps the old one; an expired peer is refused, and accepted once
  renewed; each side, against a peer that resumes, makes a full handshake when it reconnects. The
  metadata service's authorization test, across a renewal of the server and of the caller. Its
  etcd client, against an embedded etcd that requires client certificates, across a renewal and
  a reconnection.
- **S1, S2, S4–S7, Rust:** the same cases on loopback in `flaminio-net`, and reconnections of each
  side against a peer that resumes: after a renewal, a full handshake showing the new pair; after
  the peer's certificate expired, refused. In `flaminio-node`: a session opened before a renewal
  goes on, and one opened after it presents the new certificate. In `flaminio-metaclient`: a
  reconnection after a renewal, forced by the server's restart, presents the new pair.
- **S15:** in Go (`DialMeta`, `flaminio-meta`'s client) and in Rust (`flaminio-metaclient`): a
  server with an `io` certificate at the right address is refused; one with a `meta`
  certificate is answered.
- **S3:** in the tests above, no connection closes at a renewal. The I/O path, by review: no
  handshake happens in steady state.
- **S8:** the log lines, once per event.
- **S9–S11:** script tests with the VMs faked (`scripts/tools/`, in `make lint`): the half-life
  rule, a new key each time, no file written in place, each key's owner, group and mode kept,
  an existing directory's owner, group and mode kept,
  two renewals at once, a renewal cut off midway, a name it does not know refused, an
  unreachable VM, nothing restarted, the Secret renewed. A unit test of the CA tool's lives,
  under 10 minutes refused.
- **The lab, a later step, on request:**
  - **Many rotations:** a life of 30 minutes for six hours (at least ten renewals of every
    identity), a pod writing verified data on a PVC. During it: a replica's node restarted
    (`node-fault`), a partition healed, `csi-test`, `meta-smoke`. No service's start time and no
    pod's restart count changes; at the end every handshake shows a certificate less than 30
    minutes old; no I/O error.
  - **Expiry:** the renewal stopped for fl-c until its pairs expire. Its open sessions go on;
    after a partition of fl-c its new ones are refused, and so is a new metadata connection from
    it; one renewal later it is back in every ISR, with no restart.
  - **The week** on 24-hour certificates, the issue's "done when" (question 5).

## Open questions

1. **What this issue builds without an operator.** There is none, and no issue for one.
   - **(a) Recommended:** step 2 and the lab's renewal, as specified here. Step 1 waits, on a
     new issue for the operator's CA.
   - **(b)** Also a small issuing service now (the CA behind an RPC, with TokenReview), for the
     operator to host later: a new component holding the CA's key on the network, before the
     operator's shape is known.
   - Settled at approval, 2026-11-13: (a), step 2 and the lab's renewal; step 1 waits on a new issue (see decisions.md, Settled at approval).
   - After approval, plan review rounds 1 and 2: no new issue is filed. #43's issue reads "2. CA
     bootstrap: the operator holds the internal CA. Issuing on CSR is #16." So the CA is #43's
     step 2, and issuing on CSR, with its TokenReview and question 2's answer, stays this
     issue's step 1, built once that CA exists (decisions.md, rounds 1 and 2).
2. **Step 1, for a process outside Kubernetes.** TokenReview needs a service account token; host
   services and fl-c have none. Not built here, but step 1 cannot be specified without it.
   - **(a) Recommended:** an administrator issues a node's first pairs by hand; the node renews
     by presenting its current, valid certificate; a node down for longer than a life is issued
     by hand again; the CA stops renewing a removed node. It amends §15.2.
   - **(b)** Only pods get certificates: every component moves into Kubernetes first (issue #12),
     and a data node without a kubelet is not supported.
   - Settled at approval, 2026-11-13: (a), first pairs by hand, renewal by the current certificate; not built here (see decisions.md, Settled at approval).
3. **May a connection outlive its peer's certificate?** Today, and under S3 and S5, yes: a node
   no longer renewed keeps its open sessions until they break. Closing them at expiry costs a
   new handshake of every replication session each day; its effect on ISRs and I/O is not
   measured. Recommended: not in this step; record it. A certificate alone grants nothing (§15.2).
   Settled at approval, 2026-11-13: yes in this step: connections are not closed at expiry, and that is recorded (see decisions.md, Settled at approval).
4. **Where the lab's CAs live for an unattended run.** Recommended: where they are, on the
   workstation (`.lab/pki`), with the schedule there: nothing new holds a CA key, but the
   workstation stays on for the week. On the lab runner, any pushed code could read them
   (docs/ci.md); on fl-w, the CA sits beside what it certifies.
   Settled at approval, 2026-11-13: on the workstation (`.lab/pki`), with the schedule there (see decisions.md, Settled at approval).
5. **The default life, and the week.** Recommended: 30 days stays the default until step 1 (a lab
   left alone for a day would otherwise come back expired); the checks set a shorter life. The
   week with the lab's renewal proves step 2; the issue closes only when the operator's CA issues.
   Settled at approval, 2026-11-13: 30 days stays the default until step 1; the issue closes when the operator's CA issues (see decisions.md, Settled at approval).
