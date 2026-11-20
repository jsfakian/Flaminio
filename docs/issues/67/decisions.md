# Issue #67 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **Server and version** (`spec.md`, Open questions; `design.md`, Gateway and durable state;
  `adr-proposal.md`, Proposed decision and Options): as recommended, one NFS-Ganesha VFS server,
  NFSv4.1 over TCP only. The exact release is pinned by digest in the required
  `qualification.md` fixture, and no export is enabled before it qualifies.
- **Capability and seccomp profile** (`spec.md`, Open questions; `design.md`, Privileges and
  lifecycle; `adr-proposal.md`, Consequences): as proposed. A root Ganesha process with a
  measured, minimal capability set and seccomp profile, never `privileged: true`; the tunnel
  process runs nonroot with every capability dropped. The exact rules are recorded in
  `qualification.md`; a candidate that fails the fixture returns for design review.
- **Authenticated transport** (`spec.md`, Open questions and S5; `design.md`, Authenticated
  transport and claim isolation; `adr-proposal.md`, Proposed decision): as recommended,
  per-export node-local mutually authenticated TLS tunnels under a dedicated NFS CA, with the
  plaintext listener on the pod's loopback and no plaintext fallback. The tunnel implementation
  and version, revocation profile and port allocation are fixed by qualification.
- **The trust boundary** (`spec.md`, Open questions; `adr-proposal.md`, Consequences): as the
  plan states. NFS uid and gid give no per-user tenant authentication; one export is one trust
  boundary, shared under the documented Unix permission model.
- **The lab commands** (`design.md`, Privileges and lifecycle and Lab, "Proposed"): as proposed,
  `make gateway-{up,status,down,test} GATEWAY=<id> NAMESPACE=<ns> CLAIM=<name>` through
  `scripts/lab.sh` to `scripts/checks/gateway.sh`.
- **The ADR's decision** (`adr-proposal.md`, Proposed decision; `design.md`, Docs, risks and
  ADRs): accepted as proposed, with the plan.
