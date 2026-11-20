# Issue #57 — report

## Review rounds

### Plan review round 1 — eb38cba71d7e2b9f4dd2b47129c79b80e2d9a80e

Scope: full intent/spec/design/decisions/local ADR proposal against source, current dependencies and primary TPM/Linux/systemd sources.
Reviewers: /root/remaining_plan_reviews/review57_r1_code (design-code/spec correctness), /root/remaining_plan_reviews/review57_r1_security (design-security/spec security).

- **R1-C1 — Medium:** design31–34,83–100 permits configurable source ID/version and128-byte UTF8 identities.
- **R1-C2 — Low:** design41–42 incorrectly attributes8-call bound and detail-based retry to #50.
- **R1-C3 — Medium:** design83–89 canary envelope lacks Binding.VolumeID/FirstEpoch required by #50 AAD.
- **R1-C4 — Medium:** design56–65 unspecified hierarchy/parent template/child attributes/policy/session recipe determine reproducible loading.
- **R1-C5 — Medium:** design44–51,120–123 lacks executable unit/protected-memory route.
- **R1-N1 — Nit:** adr-proposal says Accepted and No choice Accepted; design stale Proposed wording.
- **R1-S1 — High:** design36–38,160–161 promises5s unseal and no leftover worker by closing transport.
- **R1-S2 — Medium:** spec34–37 preserve prior bundle on failure contradicted by design106–109 exclusive create then rename/dirfsync; replacement already occurs on fsync error, concurrent activation ownership absent.

Full evidence/fixes and primary links are retained in audit `docs/issues/review-inputs/57-r1-code.md` and `57-r1-security.md`. Dependency HEADs: #50 `e8f0fe21abb0ebd47c1e5f80ea932d73fab96baa`, #53 `105dac041ccfca18ba1bcd4984e84115efceb200`, #74 `5a1423d9ed403aae74561d29e58fa86a8acfc1b3`.

Resolution: all eight findings addressed by the planner/designer. The concrete appendix defines exact fixed/derived #50 identity, complete canary binding, code-based retry, rsa-owner-pcr-v1 policy/session recipe, protected parent/CLI/owner preflight and executable unit/device/bundle delivery. A single TPM owner uses bounded caller result acceptance, binary IPC, a per-boot lifetime record plus lock and honest timeout quarantine. Fresh immutable candidates preserve previous bundles and require explicit validated selection. Historical approvals/acceptance preserved, revisions explicitly unapproved. No runtime/dependency/lab edits.

Validation: git diff --check and issue-document relative links pass. All TPM, process, unit and fault tests remain planned implementation checks.

Status: needs_incremental_review.

### Plan review round 2 — 18f04de30bdc8295fdbde6effde3fad771ccb242

Scope: incremental `eb38cba..18f04de`, excluding report, and affected paths.
Reviewers: /root/remaining_plan_reviews/review57_r2_code (incremental design-code), /root/remaining_plan_reviews/review57_r2_security (incremental design-security).

Security reports zero comments including Nits. Earlier R1 findings are fixed; correctness reports:

- **R2-C1 — Medium:** The new recipe accepts nonempty owner hierarchy authorization but unattended startup has no protected source for it. Evidence: tpm-contracts.md:31–36,63–66,197–205; design.md:28–38. The local seal/recover pipe carries owner_auth, while serve exposes only bundle/profile and intentionally persists no hierarchy secret. Recreating the owner primary requires that hierarchy authorization.
- **R2-C2 — Medium:** Required isolated local swtpm coverage has no route through the new fixed production device, owner-state and protected-host contracts. Evidence: design.md:28–29,121–124; tpm-contracts.md:76–103,125–172. Production owner always opens /dev/tpmrm0 and fixed root-owned /run state, but make tpm-test promises private swtpm sockets and no physical TPM access. No isolated transport/record/profile injection or sandbox setup is specified.
- **R2-N1 — Nit:** Common preflight scope says any secret read, but failed preflight still starts ordinary TLS/token-authenticated metadata RPCs. Evidence: design.md:34–39; tpm-contracts.md:148–152. Existing identity/token secrets are explicitly outside #57 intent scope.

Full real reports/primary links: audit checkout `review-inputs/57-r2-code.json` and `57-r2-security.json`. Dependency #50 `e8f0fe21abb0ebd47c1e5f80ea932d73fab96baa`.

Resolution: all three findings addressed. The recipe requires empty existing owner auth and rejects extra/legacy auth input without mutation; isolated test-binary-only constructors exercise the real owner protocol/private swtpm transport/state under explicit unit/memory/permission limits without a shipping bypass; preflight wording covers TPM-source material while preserving baseline TLS/token startup. Historical approvals preserved, revisions unapproved. No runtime, dependency or lab changes.

Validation: document whitespace/relative links pass. All described regression fixtures remain future implementation work.

Status: needs_incremental_review.

### Plan review round 3 — 9c6549ce06757620a324777ec09c416e7d7f98f6

Scope: incremental `18f04de..9c6549c`, excluding report, and affected test/unit/transport paths.
Reviewers: /root/remaining_plan_reviews/review57_r3_code (incremental design-code), /root/remaining_plan_reviews/review57_r3_security (incremental design-security).

Security reports zero comments including Nits; earlier R2 findings are fixed.

- **R3-C1 — Medium:** The new direct Unix-stream TPM adapter does not provide complete TPM response framing. Evidence: tpm-contracts.md:240–242 selects net.DialUnix followed directly by transport.FromReadWriteCloser. In go-tpm v0.9.8, that adapter calls tpmutil.RunCommandRaw, which performs one Read into a4096-byte buffer; a Unix byte-stream Read may return a partial response. A fragmented valid response can fail parsing or leave bytes for the following command.
- **R3-N1 — Nit:** The new emulator invocation says swtpm2 instance without selecting the required socket interface. Evidence: tpm-contracts.md:255–257 lists socket flags after “start an owned swtpm2 instance”; the pinned executable is swtpm and the interface is selected by its socket subcommand.

Full real reports/primary links: audit `review-inputs/57-r3-code.json` and `57-r3-security.json`.

Resolution: test-only TPM Send now serializes complete command/reply pairs, completes writes, reads the10-byte header and exact validated bounded body, closes failed streams, and obeys the owner deadline. Fragmented/short/EOF/overlarge/tag/timeout/concurrency fixtures are explicit. The emulator invocation is now exactly swtpm socket --tpm2. Historical approvals preserved, revisions unapproved. No runtime/dependency/lab changes.

Validation: document whitespace/relative links pass. All test-route checks remain future implementation requirements.

Status: needs_incremental_review.

### Plan review round 4 — 55dff2b45c7cb6751d98ad65df58864f88a85913

Scope: incremental `9c6549c..55dff2b`, excluding report, and affected test transport.
Reviewers: /root/remaining_plan_reviews/review57_r4_code (incremental design-code), /root/remaining_plan_reviews/review57_r4_security (incremental design-security).

Both confirm R3-C1 and R3-N1 fixed and report **zero comments including Nits**. Full reports: audit `review-inputs/57-r4-code.json` and `57-r4-security.json`.

Status: clear. Technical snapshot `55dff2b45c7cb6751d98ad65df58864f88a85913`. Earlier findings were resolved in prior rounds. Historical approvals/local ADR acceptance remain; subsequent revisions remain explicitly unapproved. No runtime tests, TPM access, lab, dependency edits or new issue. Document whitespace and relative links pass. #57 remains open for deferred KMSProvider/offline edge/fleet stages and actual hardware evidence.
