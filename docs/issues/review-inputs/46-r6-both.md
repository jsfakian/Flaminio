# #46 plan review, round 6 (design-code + design-security), diff af5aedd..d3cf93d

Round-5 findings
- R5-1: FIXED. design.md "Exempt" and the S8/S11 row say the script test has neither the feature's string
  nor `--all-features` on any line, composes both at run time, and that the lint passes on the tree with
  it included. Consistent with the lint's build-lines rule (design.md ~288-292).
- R5-2: FIXED. design.md "Aligned" says every build answers a misaligned request as a read error
  (InvalidInput), nothing read, no panic. The S1/S3/S4 row's test ("answered as an error with nothing read")
  now holds in debug. decisions.md R4-1 text updated to "checks".

Broken or opened by the revision: nothing found.
- No remaining "assert" for the alignment check in design.md or spec.md (grep: only the "not an assertion"
  line itself). spec.md has no alignment sentence to drift.
- No other passage describes the script test as composing only the feature's string; the three places
  (Exempt bullet, S8/S11 row, decisions.md R5-1) agree.

Findings: none. Nits: none.
