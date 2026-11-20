Reviewed e22ebd7e1181dfafab41a48760bd0518657657be incrementally from b2b76b7. Reviewer /root/remaining_plan_reviews/review56_r3_code.

R2-C1, R2-C2 and R2-S1 fixed.

- R3-C1 — Medium: lab-contracts.md:184 allows conflict rereads without refreshing/validating the saved restoration snapshot. An ownerless concurrent scale/spec update can leave replicas/generation/spec stale before another hold attempt. Simplest fix: abort conflict without retry/fault injection. Alternative: validate and durably replace entire intent before retry. Fixture: ownerless scale/spec conflict preserves foreign update.

No Nits. No files changed.
