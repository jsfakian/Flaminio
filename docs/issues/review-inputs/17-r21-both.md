# Round 21 (incremental), #17, design-code + design-security
Reviewed commit: bbbe40749b866ac188a0a904b1699dd594c5bd47 (worktree HEAD; fix is 43d1ec2, HEAD adds only the report)
R20-L1: fixed in both orders. The sentence (design.md:765-768) precedes "Both orders run". Both winners reserve device entries (socket client attach, startup attach), so the registry's device entry `starting` is the right signal. Before the winner reserves, the volume has no entry (the maintenance entry was removed), so `starting` cannot be a stale value. The winner's metadata phase is held, so the entry stays `starting`.
Reading it: the node test already reads entry states and the place count through Node's wrappers (design.md:731-732, 183, 292). The state is read under the registry's lock, so there is no new access path. The wait is bounded and cfg(test).
New opening: none. Decisions item 96 matches design.md.
## Findings
None.
## Security
No new finding. Test-only change, no production surface.
