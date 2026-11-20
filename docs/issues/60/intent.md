# Issue #60: Cloud KMS providers — intent

[Issue #60](https://github.com/jsfakian/Flaminio/issues/60), README roadmap phase 5,
ARCHITECTURE §1.1, §15.4 and §17.2. It builds wholly on
[issue #50](https://github.com/jsfakian/Flaminio/issues/50) (encryption at rest), which is
planned, not built. Nothing here starts before #50's ADR is accepted and its key-source
interface built (#50's step 1). A cloud provider also waits for #50's step 2, which defines what
a provider is: its configuration, its KEK reference, the `KMSProvider` resource.

This change is the issue's step 3, as far as it can be checked without a cloud: the conformance
test every key source passes, and a source in the tests that fails as a cloud KMS does. To retain
outcome 4's bounded work, #60 also proposes an eight-call production admission gate shared by
Wrap and Unwrap in each metadata service; #50 supplies only the one-second call context. It builds
no cloud provider. Steps 1 and 2 come after it: issue #60 stays open after this change, and
cannot close without an account in each cloud.

## Why

1. **Nothing exists, on either side.** No cipher, key or KMS code (#50, Why 1). The CSI
   controller refuses the README's `kmsProvider` as an unknown parameter (`parseParams`,
   `control/csi/controller.go:136`, `:162`). `go.mod` has seven direct dependencies and no cloud
   SDK (`go.mod:7`).
2. **Who calls the KMS is #50's open decision 1, and it fixes what a provider is.**
   - **(a) The metadata service** holds the wrapped volume key and releases the key only to the
     node it names target (#50's recommendation). A provider is Go code in the metadata
     service. One cloud identity, the service's, calls for every volume.
   - **(b) Each node's data plane** calls the KMS. A provider is Rust in the data plane, whose
     TLS trusts the cluster's CA alone (`flaminio-net/src/tls.rs:54`) and which has no HTTPS
     client. Every node that may become a target needs its own cloud identity and the right to
     unwrap.
   - This intent follows (a), as #57's does. The decision is #50's ADR's. Under (b) the test of
     this step is Rust, and the later steps move to the data plane.
3. **The three services differ where #50's rules must not.** Read from their documentation on
   2026-11-12; the design confirms each.
   - **Binding a wrapped key to its volume.** #50 refuses a key that is not the volume's. AWS
     KMS and GCP KMS authenticate extra data with the ciphertext (encryption context,
     additional authenticated data). Azure Key Vault's `wrapKey` does so only with its
     authenticated algorithms, not with RSA-OAEP. A provider cannot rely on the service for
     the binding.
   - **The KEK's version.** AWS and GCP find it from the ciphertext. Azure's call names the
     version in its path: the wrapped form must record it, or a rotated KEK locks volumes out.
   - **Deletion is delayed, then final.** AWS: 7 to 30 days, cancellable, and the key refuses
     every operation meanwhile. GCP: a version is scheduled for destruction, 30 days by
     default, restorable until then. Azure: soft-deleted for a retention period, then purged.
     A KEK disabled or pending deletion fails every later attach of every volume under it,
     while attached volumes keep serving: the outage shows at the next failover.
   - **Rate limits** are per account, project or vault, and shared with the cloud's other
     callers.
4. **A key call sits on the attach path, never on the I/O path** (#50, outcome 7). The key is
   needed at the first attach, at a failover and after a data-plane restart (#50, Why 6). §1.1:
   a KMS is sometimes unreachable.
   - Today `Attach` is one read and one conditional etcd transaction
     (`meta/service/service.go:696`). A KMS call adds a network round trip of unknown length to
     it, and to §11.2's failover.
   - A caller gives one metadata instance 10 s, then asks the next (`attemptTimeout`,
     `metaCall`, `control/csi/meta.go:21`, `:64`). A slow KMS makes each instance in turn call
     it again.
   - A restarted data plane attaches all its volumes again: one unwrap each, at once, against
     the rate limit.
5. **"Workload identity" presumes the service runs where the cloud's identity reaches it.**
   The issue means the cloud's identity, not §15.2's certificates, which a cloud does not know.
   - The metadata service is a host unit, on fl-a, fl-b and the witness (`cmd_meta`,
     `scripts/lab.sh:509`). Its secrets are files given as systemd credentials
     (`scripts/guest/meta.sh:21`), and `serve` takes paths on its command line
     (`meta/cmd/flaminio-meta/main.go:84`).
   - **As a pod,** the identity is a projected service account token. The metadata service in
     Kubernetes is a later step of #43.
   - **On a VM,** the identity is the instance's, read from the instance metadata endpoint. The
     unit may reach it (`RestrictAddressFamilies`, `meta.sh:43`; no address filter). But it is
     then the identity of every process on that VM, and two of the lab's three instances share
     their VM with a data node.
   - The cloud's audit log names that identity. Under (a) it is the same for every volume: the
     log tells volumes apart only if each call carries the volume. Flaminio's own audit log
     (§15.3) does not exist.
6. **`KMSProvider` kinds have nothing to carry them.** §17.2 makes it a cluster-scoped resource,
   admin-only (§15.3). No resource type exists (`api/` holds `proto` and `gen`), and no
   operator: #43's first step leaves the other resources out. #50's step 2 brings the resource.
7. **Each provider is a large new dependency.** One SDK per cloud, or one thin HTTPS client
   per cloud with its own request signing and token exchange. ADR 0001 names the Kubernetes
   and CSI libraries, no cloud SDK. `govulncheck` and Dependabot would cover either.
8. **The project has no cloud.** The lab is KVM VMs on two servers (`docs/lab.md`), CI runs on
   a server beside them (`docs/ci.md`), and ADR 0004 puts cloud third. No account, no KEK, no
   credentials for CI.
   - **Emulators** exist for some of the three, none from the cloud itself as far as known.
     They prove the shape of the calls. They do not prove the identity, the access policy, the
     rate limits, the key states and their timing, or the audit log.
   - **A fake written here** proves less: our reading of each API.
   - What needs no cloud is the contract itself, and how the service behaves when a key
     source fails as a cloud KMS does.

## What we want

1. One conformance test states what every key source must do, in #50's terms:
   - a wrapped volume key unwraps to the same key, and the wrapped form does not hold it;
   - a wrapped key presented for another volume, or under another KEK, is refused, not
     unwrapped to something else;
   - a key wrapped before its KEK was rotated still unwraps;
   - failures are told apart: away or rate-limited (try later), refused (the identity may not
     use the KEK), the KEK disabled or pending deletion (still reversible, and it says so), a
     damaged wrapped form;
   - every call ends within a bound, and no error or log line holds a key.
2. It runs on every key source that exists: #50's static key, then Vault Transit and KMIP as
   #50's step 2 brings them. A new source is added to it in one place, and a source that is
   not in it cannot be configured.
3. A source in the tests fails on command as a cloud KMS does: away, slow, rate-limited,
   refusing, its KEK disabled, pending deletion, or rotated. It passes the test itself.
4. With that source, #50's attach rules hold under each failure. An attach fails closed, says
   which failure, changes no record, and works again when the source does. An attached volume
   makes no key call. A slow source is not called again by every metadata instance in turn,
   and many attaches at once make a bounded number of calls.
5. All of it runs in `make test` and `make race`: no cloud account, no emulator and no external
   network. Owned loopback metadata mTLS, embedded-etcd client/peer traffic and replica TLS
   fixtures are allowed.
6. The same test can be pointed at a live service by configuration, for the later steps. Not
   configured, it reports that it was skipped; it never passes unrun.

## Out of scope

- **An ADR before the first provider:** an SDK or a thin client per cloud; which identity
  calls, and what the audit log then shows; where the metadata service runs in a cloud.
- **Step 1: the three providers,** one at a time behind #50's interface: AWS KMS, GCP KMS,
  Azure Key Vault. Each passes the test against an emulator where one is good enough, and
  against its cloud.
- **Step 1: the `KMSProvider` kinds.** They need #50's step 2 for the resource and #43 for
  whoever reconciles it.
- **Step 2: workload identity.** It needs the metadata service where the identity reaches it:
  a pod (#43's later step), or a VM whose instance identity is the service's alone.
- **The issue's "done when":** each provider in its cloud, with workload identity. It needs an
  account, a KEK and CI credentials per cloud. Nothing in this step shows it.
- **Emulators** in CI or in the lab.
- **Reading (b),** and a node's sealed cache of released keys, to attach while the KMS is away
  (#57, Why 2 (c)).
- **Rekey and crypto-erase** (#50's steps 4 and 5). Rotating a KEK in the cloud, and what a
  delayed deletion means for crypto-erase, are theirs. Flaminio never deletes a KEK.
- **Vault Transit and KMIP themselves** (#50's step 2). This step only runs the test on them.
- **The audit log** (§15.3), and exporting the cloud's.
- **HSM-backed and external key stores,** keys shared across regions or accounts, private
  endpoints.
- **The rest of phase 5:** instance-store NVMe, zone-cost-aware placement, autoscaler
  integration.
