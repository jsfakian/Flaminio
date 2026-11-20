# Issue #59 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending). These are
historical approvals of that revision, not approvals of the review-round-1 changes below.

## Settled at approval (historical record)

- **The window contract against the intent's "nothing exchanged"** (`spec.md`, Open questions
  and S3; `design.md`, Worker choke point and wire envelope; `adr-proposal.md`, Proposed
  decision): as proposed. No new admission after the observed window closure, plus a declared
  and published bounded tail; literal zero traffic after the boundary is promised only behind a
  qualified external gate and never covers packets already in flight. A platform that is not
  qualified refuses strict window mode.
- **Measurement boundary, burst and tail defaults** (`spec.md`, Open questions; `design.md`, Pure
  transfer policy and Worker choke point and wire envelope): as proposed. The wire cap counts IP
  bytes including TCP and TLS, captured at the matching IP boundary of the run's own namespace;
  the burst defaults to 65536 bytes (4096 to 1048576); the close tail is a measured
  qualification output published per platform, not a constant.
- **Object-store client progress controls** (`spec.md`, Open questions; `design.md`, Worker
  choke point and wire envelope): as proposed. No timeout on a whole upload's age; separate idle
  limits (remote progress 120 s, connect 30 s, headers 60 s, configurable within bounded ranges)
  that advance only while sending could progress; SDK retries, redirects and credential refresh
  go through the one wrapper or are accounted for. The historical text left SDK hook exposure as an implementation gate; review round 1 replaces
  that gate with the concrete proposed transport profile without changing this approval record.
- **Rate-enforcement privilege** (`spec.md`, Open questions; `design.md`, Worker choke point and
  wire envelope; `adr-proposal.md`, Options): as proposed. A trusted setup helper isolates the
  run's sockets in a dedicated network namespace with an exclusive egress shaper; it holds no
  object credentials, the worker drops setup privilege and gets no broad `NET_ADMIN`, and a setup
  that is not qualified (or not Linux) offers only an explicitly named payload cap.
- **The proposed configuration** (`design.md`, Pure transfer policy, "Proposed config"; Files
  touched): as proposed: the upload rate, burst, window, time zone and concurrency flags with
  their limits, `--update-transfer-policy`, and `make wan-backup-test`.
- **The ADR's decision** (`adr-proposal.md`, Proposed decision; `design.md`, Docs, risks and
  ADRs): accepted as proposed, with the plan.
- **#52's object unit, amplification and common transfer choke point** (`spec.md`, Open
  questions; `design.md`, Docs, risks and ADRs): no recommendation in the plan. Settled 2026-11-13
  (user's instruction): see below. The
  design assumes one object-transport factory and whole 4 MiB objects, but leaves the choice to
  #52's ADR, which must settle it first.

## Settled at the user's instruction of 2026-11-13

- **#52's object unit, amplification and common transfer choke point:** as the design assumes
  and #52's plan, accepted 2026-11-13, fixes them. The object unit is #52's whole 4 MiB
  content-addressed encrypted block (pages and roots at most 1 MiB); the amplification is #52's
  (a changed block costs up to a 4 MiB read and upload before compression, and diff false
  positives are counted as extra read work); the one choke point is #52's `ObjectStore` adapter,
  whose `before_attempt` interceptor runs on every HTTP attempt, SDK retries included, and is
  where #59 admits or defers. The SDK hook exposure was historically an implementation gate; the proposed transport profile
  now supplies concrete integration instead of treating that gate as an unresolved mechanism.

## Review round 1 — proposed revisions, unapproved

- **R1-C1/C2/C3/C5:** [transport-profile.md](transport-profile.md) supplies the owned Linux
  networking helper/connector, concrete SDK pins and controls, exact local egress boundary,
  accounting, phase clocks, cleanup and conformance fixtures. #74 provides no implicit networking
  helper. Downstream burst formation prevents a sender from guaranteeing the old remote-arrival
  envelope; this boundary clarification and the wire profile's supported rate floor are changes
  to the historical approval, not newly accepted decisions.
- **R1-C4:** [policy-contracts.md](policy-contracts.md) re-admits after all waits under #53's
  qualified store time, exact-version cover, one re-cover and one-root rules. Policies which
  cannot fit cover/root dispatch in the fixed commit budget fail explicitly with pins kept;
  a root accepted late permanently ends that operation. Automatic retry is conditional on
  compatibility and cannot make a late root complete. #53's settled contract is unchanged.
- **R1-C6:** K in-flight blocks cost at most K*4194384 encoded PUT bytes after one cut; required
  GET readback/revalidation is counted separately against a bounded graph and resume episode,
  including inbound/control bytes and requests. Repeated outages have no finite total bound.
- **R1-C7:** Exact proposed lab grammar/topology, fixture/cleanup and #83 admission ordering are
  in policy contracts. The full write-load campaign is Blocked on current VMs when #83 is active;
  no fake physical file or bypass. An authorized earlier legacy VM run remains exploratory.
- **R1-C8:** Explicit `--transfer-policy wire-v1|payload-v1` activates autonomous outage retry.
  No #59 options preserves #52's exit-1-and-rerun contract. Incidental upload options without an
  explicit profile are usage errors. This default interpretation was unspecified at approval.
- **R1-C9:** Both languages consume one bundled IANA-2025b transition-table artifact, never host
  tzdb. Persisted source/table identity and canonical policy hash bind restart; updates need an
  explicit revision with zero credit. Actual UTC vectors cover DST/midnight/rule mismatch.
- **R1-N1:** Current prose distinguishes these unapproved revisions from the preserved historical
  approvals/settlements. Qualification results remain unknown; absence of measurements is not
  used to defer choosing the mechanism.

## Review round 2 — proposed revisions, unapproved

- **R2-C1:** The transport profile specifies lease/window-bound validated IPv4 fragmentation-needed
  feedback for the owned TCP path, with smaller-MTU routed-endpoint, forged-feedback and closed-gate
  fixtures. This changes only the proposed #59 profile; historical approvals remain intact.
- **R2-C2:** `local_cost(profile,n,c)` and phase clocks branch by profile. Payload-v1 uses its
  optional body rate and reserved credit; no-rate window-only mode has zero known pacing cost,
  with no wire r/Q division or kernel allowance. Root admission still needs fresh dispatch,
  open-window/generation checks and #53's2u allowance. Zero local cost is not a network-service
  guarantee. Low-rate incompatibility, available-credit, closure and remote-backpressure fixtures
  cover both supported payload modes; a conservative preflight cannot erase available credit.
