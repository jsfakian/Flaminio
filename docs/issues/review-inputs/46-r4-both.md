# #46 plan review, round 4 (design-code + design-security), diff be8f4dd..eac81e3

Round-3 findings
- R3-B1 (unsafe in flaminio-io): FIXED. UringDevice holds `file: File` (uring.rs:38), so
  `self.file.as_fd().try_clone_to_owned()` (or `File::try_clone` then `OwnedFd::from`) is safe
  and implementable; `is_direct()` exists (uring.rs:165); AlignedBuf is 4096-aligned and has
  DerefMut (aligned.rs:81), so `read_at(&mut buf, off)` gets an aligned buffer. dup shares the
  file description (offset, O_DIRECT) but `pread` does not use the offset. One remaining gap: R4-1.
- R3-B2 (lint's own lines, CI): FIXED in substance (renamed lint, two path exemptions, CI step).
  Opens R4-2.
- R3-B3 (measurement first step): FIXED. `scrub-build` passing only on the usage error
  (control.rs:109) fails closed on a lab-tools build, a dead socket or no answer.
- R3-N1, R3-N2: fixed. R2-N3 left on purpose.

New findings
- R4-1 Low (design.md "The read"): `read_at` is one `pread`; it may return short (EINTR, EOF
  at the device end) and the design says nothing of that, nor of the O_DIRECT requirement
  that offset and len be multiples of the logical block size (the AlignedBuf's pointer is
  aligned, its `len` is whatever is asked). A short read taken as a full block would CRC a
  partly zeroed buffer and report a false mismatch (a false "corrupt" feeding S-rules), and
  EINTR would abort a pass. Fix: say the reader uses `read_exact_at` (retries EINTR; short
  read = I/O error, not a CRC result), asserts offset/len 4096-aligned, and add a test for a
  read past the end of the file returning an error.
- R4-2 Low (design.md "CI"; docs/ci.md:5-13): ci.md says every CI job is a `make` target so a
  failure reproduces locally. The new step is a second raw `cargo test` in `rust-test`
  (ci.yml:40 already is one, but the design widens the exception), runs the whole workspace
  suite twice (job timeout 40 min), and docs/ci.md is not in the list of files to change.
  Fix: add a make target (for example `test-fixture`) that both `make test` and the CI step
  call, or at minimum list docs/ci.md as updated and run only the packages with
  `lab-tools` code (`-p flaminio-io -p flaminio-node -p flaminio-store`).
- R4-3 Low (lint exemption): the exemption is "by path" for `scripts/tools/scrub-test.sh`, a
  whole file, so any real build line added there goes unlinted; the lint is also a script
  scanning `scripts/`. Fix: exempt only lines between marker comments, or have the script test
  build its doctored lines at run time (printf into a temp tree) so no file needs exempting
  but the lint; add a script-test case that a `lab-tools` line in any other
  `scripts/tools/*.sh` fails.
