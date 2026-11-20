# Plan review: issue #44, round 7 (incremental), design-code and design-security
Reviewed: git diff f2655aa..aa0457b (design.md S12/S13 test row only).
- R6 Nit fixed: the row now says the watcher "still exits non-zero at its limit and the run is invalid (S11)".
- Agreement: S11's last bullet (not in every ISR within the limit, or watch lost: invalid run) and the design's watcher-limit text (lines 141-168: bounded by the watcher's own limit, a run over it fails as invalid) say the same.
- No findings at Low or worse. No new safety rule, wire or format change.
