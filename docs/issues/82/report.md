# Issue #82 — report

## Review rounds

### Plan review round 1 — 883c3178b106ab9ba7a6eeb64208aa888b83f49c

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r82c1 (design-code), r82s1 (design-security).

- **Review82-R1-C1 — High:** Production authority/config/identity/held-fence/return mechanism unspecified, leaving S4/S7 fake-only Evidence: design.md45–47,85–96,109–111,164–181.

Changes/resolution: Both fresh reviewers independently confirmed C1 with no other comments. Added concrete Proposed manual-lockout-v1: bounded immutable Ed25519 operator assertion and inventory bound to cluster/NodeUID/physical serial, explicit physical restart lock held through old-UID retirement, precise signer/config/file/API checks, default refusal and terminal fresh-NodeName/PKI/clean-OS return. Preserve every replica disk/key and #13/#14/#61 obligations; no software claim to measure isolation, no same-identity automatic Clear. Added real crypto/file/API/crash/retirement test requirements, VM refusal and physical qualification boundaries; all changes remain draft/unapproved.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — f2506fb13dacad30ec6c74c8ab7c9a8d951c69c8

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r82c2 (incremental design-code), r82s2 (incremental design-security).

- **Review82-R2-N1 — Nit:** Authority configuration called Node flag despite belonging to flaminio-csi failover Evidence: manual-lockout-contract.md16; main.go69–76.

Changes/resolution: Both fresh reviewers confirmed prior HighC1 resolved and independently reported only this Nit. Named exact failover-mode CLI and failover deployment wiring. No other comments or runtime/lab testing/approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 7c33526dbe40a4650bf3b0c8edc9886c47da35fc

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r82c3 (incremental design-code), r82s3 (incremental design-security).

- **Review82-R3-N1 — Nit:** New wiring wording uses nonexistent deployment flaminio-csi-failover Evidence: manual-lockout-contract.md102; driver.yaml180.

Changes/resolution: Both fresh reviewers confirmed prior mode Nit fixed and caught this new deployment-name typo. Corrected to existing flaminio-failover. No other findings, implementation/runtime/lab tests or approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 6c858756ec7aa9ceabb4530aba3400f8acb62016

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r82c4 (incremental design-code), r82s4 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed deployment-name Nit resolved against actual driver.yaml and reported zero comments including Nits. Earlier High production authority finding was confirmed resolved in R2; R2 mode Nit confirmed fixed in R3. No implementation/runtime/lab tests or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.
