# Round 19 (incremental), #17, design-code + design-security
Reviewed commit: a3a3bcd26e0c2c7944b27b1b8ea89b3f0324f926 (worktree HEAD; fix diff c3e6ac5..d95e16e)
R18-L1 fixed (design.md:762-767, decisions item 94: reached-gate signal, socket gate opened only after it).
R18-L2 fixed (decisions item 90 marked: gate position -> 93, "after the interval" -> 92/94, done() -> 91).
R18-L3 fixed (design.md:766-773: "entry ... still starting (held)", the failed-held-attach outcome named as #15's accepted cost).
No stale "after the interval" or "running only" wording remains in design.md.
## Findings
1. Low. The test needs to know the socket client's reserve has happened before it opens the startup gate (socket-first), and the startup client's reserve before opening the socket gate (startup-first). The text relies on the held metadata phase but does not say the test waits for it to be reached. Without that, the loser could reserve first and win. Fix: say the test waits for the winner's held metadata phase to be reached before opening the loser's gate.
## Security
No new finding. The signal is cfg(test) only; no production surface changes.
