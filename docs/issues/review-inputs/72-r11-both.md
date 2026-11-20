# #72 round 11 (incremental, 5966eff..89fb28e)

1. Medium (drift). design.md:145-146, Files list: "`meta/service/expand.go`, `expand_test.go` (#70's): the growth charge, by whichever of #70 and #72 lands second (S3)." Not touched by the commit; contradicts design.md:89-91, spec S3 and #70's Charges (#70 lands with or after #72 and adds the charge). Fix: "the growth charge, added by #70 (S3)".
2. Low (drift). decisions.md:138-140, "Growth (cross-plan with #70)": still says "whichever of #70 and #72 lands second adds it and its test", with no "changed" note, unlike the entry at :147-150. Fix: reword as #70 adds it, or mark it superseded by the :147 entry.

Everything else in the diff (intent 5, spec S3, design growth bullet and S3 row) agrees with #70's design Charges (design.md:76-80) and its ordering prerequisite. No step-1 refusal is described anywhere else in #72.
