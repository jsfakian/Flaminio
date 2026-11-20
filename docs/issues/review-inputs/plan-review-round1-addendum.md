# Round-1 addendum — second phase of the batch (from 2026-11-14)

Read after `plan-review-brief.md`. It corrects what has changed since that brief was written.

- **The plans are approved.** Every plan records the user's approval of 2026-11-13, and answers
  settled at the user's instruction (`decisions.md`: "Settled at approval", "Settled at the user's
  instruction of 2026-11-13"). Those are decisions, not findings by themselves. A settled answer is
  a finding if the code contradicts it or it leaves a mechanism unspecified. Review the plan as it
  stands at HEAD.
- **Report the reviewed commit** as `git -C <the plan's checkout> rev-parse HEAD`.
- **Review-clear plans are fixed points.** 37 plans are review-clear and approved: #6, #9,
  #12–#18, #43–#53, #70–#86. Where this plan touches what one of them defines, the clear plan
  wins: report the contradiction as a finding against this plan. Their folders:
  - on `origin/main` (read with `git show origin/main:docs/issues/<n>/<file>`, or in the main
    checkout): #12–#18, #43, #44, #70–#86 as merged by PR #87;
  - newer than `main`: #6 and #9 in `/home/jsfakian/Documents/src/flaminio/docs/issues/<n>/`;
    #45 to #53 in `/home/jsfakian/Documents/src/flaminio-<n>/docs/issues/<n>/`.
- **Contracts already claimed** by clear plans:
  - `Volume` proto fields 9–10 (#79), 15 (#13), 16–17 (#17), 18–19 (#18), 20 (#50), 30–31 (#51), 40 (#75);
  - `MoveReplicaResponse.reason` = 3 (#13);
  - tag 72 on `NodeHeartbeatRequest.capacity` and `Node.capacity` (#72);
  - Proposed ADR numbers: 0007–0011 are the audit's, 0012 (#12), 0013 (#16), 0014 (#17);
  - etcd transactions hold at most 120 operations (`meta/kv/etcd.go:91-92`).
- **Cross-plan hand-offs** other plans have queued for this one are in
  `/home/jsfakian/Documents/src/flaminio/docs/issues/batch-2026-11-12.md` (search `#<n>`). Each
  one this plan does not yet take is a finding.
- **Scripts and code** are described as they are on `origin/main` now.
- **Nits (the user's rule from 2026-11-16):** a plan now clears when a round reports nothing of
  Low or worse. Nits no longer hold a plan open: report one only when it is real, keep it to one
  line, and never raise the severity of a wording point to keep it alive. This replaces the brief's
  "zero comments, Nits included".
