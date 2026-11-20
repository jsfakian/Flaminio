# Issue #67 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`a42901efa10fd85d50e91f07260d3d67ab65d772a12f37142b55512124989d3c`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

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


## Review corrections — 2026-11-20 (proposed, unapproved)

The approval and settlements above remain the historical record. These actual R1 corrections
are not fresh human acceptance, implemented support or executed qualification evidence.

- **R1-C1:** before #83, explicitly label older VM-baseline evidence; after #83, current VMs
  refuse before mutation and successful two-host qualification uses admitted physical hosts.
  #84 physical/minimum-kernel evidence stays separate; no VM override or dependency-plan edit.
- **R1-C2:** require operator-installed NFSv4.1 host client/helper and actual kubelet-path
  preflight on both consumer nodes, with recorded versions and missing-client refusal fixtures.
  Up/test never silently install packages or load modules.
- **R1-C3:** add stable WRITE/COMMIT sync delay/error ordering and a negative skip-wait oracle.
  Process-kill page-cache survival and downstream block conformance cannot replace that test.
- **SEC67-R1-01:** the proposed owned host mount-helper gate records invocation ownership
  before launch. Down durably seals both hosts, waits pre-seal helpers, deletes only the owned
  consumer PVC and proves node/API quiescence before endpoint retirement. Unknown ownership
  holds resources; no timeout/force-token/port-reuse shortcut. This adds a host installation
  prerequisite and may leave down blocked with old service retained, an explicit unapproved
  availability cost; it adds no CSI/controller or gateway API credential.
- **SEC67-R1-02:** require finite per-export Ganesha, tunnel and host-proxy containment categories,
  effective control read-back and slow-handshake/NFS pressure/restart qualification. Exact
  release-specific values remain in the accepted qualification approach; no custom memory proof
  or filesystem quota is introduced.

See [qualification](qualification-contract.md) and [withdrawal](withdrawal-contract.md) contracts
for concrete future checks. Only static source/document/link/whitespace checks ran for this fix.


## R2 review correction — 2026-11-20 (proposed, unapproved)

- **SEC67-R2-01:** each consumer's actual API UID appears in its immutable PV NFS path, after
  Pending PVC creation and before PV/binding/Serving publication. Permanent export UUID,
  Export_Id/FileSystem_ID and server owner/reclaim identity remain separate. Reopening requires
  a new PVC UID; cached old source paths cannot pass against the new Serving record. All stale,
  unknown or malformed reserved owned sources fail closed rather than delegate as unrelated NFS.
  Paused-before-dispatch, partial reopen and old/new source fixtures exercise this distinction.

The prior withdrawal/resource corrections and historical accepted decision remain preserved;
this refinement is unapproved and no helper/API/mount/runtime/lab fixture was executed.
