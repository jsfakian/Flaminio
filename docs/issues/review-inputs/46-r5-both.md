# #46 plan review, round 5 (design-code + design-security), diff 8a39a89..ee3aa88

Round-4 findings
- R4-1: FIXED in substance (read_exact_at, alignment check, past-the-end test, spec line). Opens R5-2.
- R4-2: FIXED. `-p flaminio-store -p flaminio-node -p flaminio-io` with `--features flaminio-io/lab-tools`
  is valid (resolver 3, flaminio-io is among the selected packages); `$(MAKE) test-fixture` fits
  Makefile `test` (it already calls `$(MAKE) fuzz-regressions`); ci.yml:40 and docs/ci.md's CI row are
  in the files list. (ci.md "every job is a make target" is already untrue for line 40; the design
  narrows it, does not widen it.)
- R4-3: FIXED for the string `lab-tools`. Opens R5-1.

New findings
- R5-1 Low (design.md "Build lines"; tests table S8/S11): the script test is "linted as any
  script" and has "no line with the string", but the string it avoids is only `lab-tools`. The
  lint also fails `--all-features` on a main-workspace build line, and the test must write such a
  doctored line (and `cargo build` lines for the Makefile/script/workflow cases) with `printf`.
  Those printf lines contain a literal `cargo build ... --all-features`, so the lint fails on the
  script test. Fix: say the script test composes `--all-features` (and the build command words) from
  parts too, or define the lint's match so a line inside a printf/quoted fixture is not a build
  line; add to the "passes on the tree, script test included" case.
- R5-2 Low (design.md "Aligned"): "asserts" the offset/length are 4,096 multiples, yet "a release
  build answers it as a read error and does not read". That reads as `debug_assert!` + a release
  check: in a debug build (what `make test` runs) the new test "an offset or a length that is not
  a multiple of 4,096 is answered as an error with nothing read" would panic the reader thread
  (the daemon), not return an error. Fix: say plainly it is a checked condition returning an I/O
  error (`InvalidInput`) in every build, no assert/panic; the test then holds in debug.
