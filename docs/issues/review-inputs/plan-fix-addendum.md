# Fix addendum — second phase of the batch (from 2026-11-14)

Read after `plan-fix-brief.md`. It corrects what has changed since that brief was written.

- **The plan is approved** (2026-11-13; see its `decisions.md`). The brief's "Nothing is approved"
  is outdated.
  - Record each change in `decisions.md` as made after approval, in this round of the batch's
    review, with the finding id.
  - Add one line under Approvals: "Plan review round <N> (<date>) fixes, made after approval: see
    below."
  - A change must not narrow the intent or a spec rule unless a finding shows the rule cannot hold
    as stated. Then say so in `decisions.md` and in your final message.
  - A risk a finding names is recorded as "named, not accepted". Accept nothing on the user's
    behalf, and invent no results, measurements or approvals.
- **Nits (the user's rule from 2026-11-16):** fix a Nit only when the fix is easy, a small change
  of wording or a reference. Leave any other Nit unfixed, list it in the report entry under
  "Nits left", and say so in your final message. Every finding of Low or worse is still fixed.
- **Take the reviewer's recommended fix.** The user asked for unattended work that takes the
  recommendations. Where a finding offers options, take the simplest safe one ("cut before
  adding") and say why. Where two reports overlap, fix the root cause once and name every finding
  id it closes.
- **Review-clear plans are fixed points:** #6, #9, #12–#18, #43–#53, #70–#86. Where this plan
  touches what one of them defines, follow the clear plan and don't edit it. If a clear plan needs
  a change, describe it in your final message; it is made by whichever plan lands second.
  - On `origin/main`: #12–#18, #43, #44, #70–#86 (`git show origin/main:docs/issues/<n>/<file>`).
  - Newer than `main`: #6 and #9 in `/home/jsfakian/Documents/src/flaminio/docs/issues/<n>/`; #45
    to #53 in `/home/jsfakian/Documents/src/flaminio-<n>/docs/issues/<n>/`.
- **Outward-facing steps are not taken:** no GitHub issue is filed, nothing is pushed. Where a fix
  calls for a new issue, write "to be filed" and list it in your final message.
- **Scripts and code** are described as they are on `origin/main` now
  (`git show origin/main:<path>`). Lab scripts and code are not edited: the design states the
  change.
- **Safety rules:** a new or changed replication, membership or recovery safety rule keeps its
  simulator check and an injected bug the simulator must catch, in `flaminio-repl` (AGENTS.md).
- **The report entry** goes to the scratchpad path your prompt names, never to `report.md`. Its
  last line is "Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes
  were made after approval."
- **The commit:** one new commit, never an amend; only files under the plan's folder (plus the
  plan's own Proposed ADR file where your prompt says so); staged by name and committed with
  `-- <paths>`; the last line `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`. Check
  `git diff --check` and line lengths (100 columns for prose) before committing, so no second
  commit is needed.
- **Final message:** the commit hash, one line per finding id, every rule narrowed, every risk
  named, and anything another plan or the user needs.
