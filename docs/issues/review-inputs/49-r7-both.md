# Plan review: issue #49 (Chaos test suite), round 7, design-code and design-security

Scope: `git diff a08bad2..2ff6fa4 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. Checked against `origin/main`'s `scripts/lib.sh`
(`run_detached`, :163-173: the unit runs as `--uid=$VM_USER`, `mkdir -p ~/.flaminio-lab`),
`scripts/guest/write-load.sh` (:9, `sudo fio`), `scripts/guest/chaos.sh`, and contracts.md
(:9 header prefix 8 + u32 + payload <= 4096 + u32 = 4,112; :20 every frame exactly 136 bytes).

Counts: Low 2, Nit 1.

## Earlier findings

- R6-B1: fixed. The client runs as `sudo "$HOME/flaminio-history" run` (design.md:69-74), the gate
  accepts uid 0 only (:276-278), `end`/`status`/helper reads/signals/collection are all `sudo`
  (:117, :245, :251-256, :299, :319-320), and the script test and main_test.go cover both
  (:337). The uid-0 argument holds: rename needs write on the source directory, so a root file
  the lab user can move is one in that user's own directories; hard links are limited by
  `protected_hardlinks`; device nodes need `CAP_MKNOD`; the remaining swap of another
  root-owned run file is named (decisions.md:447-450) and bounded to kill-or-refuse.
- R6-S1: fixed. `openat2` with `O_RDONLY|O_NONBLOCK|O_NOCTTY|O_CLOEXEC` and
  `RESOLVE_NO_SYMLINKS` (design.md:274-276) is a valid `how` (no `O_CREAT`, mode 0), exists as
  `unix.Openat2` in `x/sys`, covers every component, and a FIFO opens at once and is refused
  by `fstat`; tests at :337.
- R6-C1: fixed (design.md:26-29, test at :337).
- R6-C2: fixed (design.md:58-62; recorder_test.go case at :335).
- R6-C3: fixed (design.md:334).
- Nit 1: fixed. 481 x 136 = 65,416; frames are fixed-size (contracts.md:20), so
  `header_end + 136k` lands on frame boundaries; with at most 16 operations in flight and a
  drain per burst, an outstanding write's Issued frame and any later Return are both inside the
  window. Header refusals at design.md:283-284.
- Nit 2: fixed (design.md:271-273).
- Nit 3: fixed (design.md:264; decisions.md:366-367).

### R7-C1 — Low
**Nothing says who creates the segment directory, and with the client now root it matters.**
design.md:72 and decisions.md:404-405 assert "a segment directory the lab user owns", and the
uid-0 argument (:277-282) and the helper's own `kill-mark` write (:294, run as the lab user via
`chaos.sh`) depend on it; but no step creates `~/.flaminio-lab/chaos-<h>/<segment>/`
(`run_detached` makes only `~/.flaminio-lab`, lib.sh:170). If `run --history PATH` creates its
parent (the natural `MkdirAll`), it now does so as root: the directory is root's 0755, the
helper's `kill-mark` write fails, and every kill round fails; a `sudo mkdir` in the harness does
the same.
**Fix:** state that the harness creates the segment directory as the lab user, without `sudo`
(e.g. `mkdir -m 0700`), before the client's unit starts, and that `run` creates only its files
(exclusive creation) and never a directory, refusing a missing parent. Script test: the
directory is made without `sudo` before the unit; workload_test.go or main_test.go: `run` with a
missing parent directory refused.

### R7-S1 — Low
**Two of the new `SUDO_UID` refusals have no test.** design.md:272-273 refuses a `SUDO_UID`
that is missing, not a number, 0, or with no passwd entry; the main_test.go list (:337) names
only "missing or 0". A `SUDO_UID` such as `1000x` or `-1`, or one with no entry, would go
unchecked: an implementation that parses leniently or falls back to `$HOME` (root's) would pass
the tests.
**Fix:** add to main_test.go: a `SUDO_UID` that is not a strict decimal (`1000x`, `-1`, empty)
and one with no passwd entry (the lookup injected), each refused before the open.

## Nits

1. design.md:338: the blank line added after the S9/S10 row ends the Markdown table, so the
   S11–S13 to S17 rows render as plain text with literal pipes. Remove the blank line.

Reviewed commit: 1e3fba0f751db95b55cc543d4919aadff3bd0268
