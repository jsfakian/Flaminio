# Plan review: issue #9, round 6 (incremental), design-code + design-security
Reviewed commit: beb2a640f7270994c42e5d9aa451545a61c3f9e7 (diff a3bb571..210e7e3)

## Round-5 findings
- R5-B1: fixed. `bench_attached` accepts `not running`; the caller passes 0 and 3, stops on 255 (lost) and other exits; a vector and a lab check (node down b, bench-reads) were added. cmd_bench_reads step 4 does start a stopped node (`! is-active` -> `cmd_node up`; `systemctl show ExecStart` works on a stopped unit). `release_lab_volume` ignores the exit 3 (`|| true`). Residual: R6-A.
- R5-B2: fixed. A one-blank-line `answers` file gives `['']`, so the fake server answers an empty line (node-scripts-test.sh:79), not `error: no answer`. `fresh` writes answers first (line 112), as the design now says.
- R5-B3: fixed. `pvc-` volumes get "delete the pod" and no `vol detach`; others get "unmount it if mounted" first, as node.sh `down` (lines 22-27) advises.
- R5-N1: fixed. `vms_of` accepts the VM name (lib.sh:40-41) and `vol detach` calls `vms_of "${2:-a}"` (lab.sh:721), so `vol detach fl-b` runs.

## New findings

### R6-A - Low: the caller's exit mapping has no test except exit 3
The 0/3/255/other mapping sits in `cmd_bench_reads` (lab.sh), outside every script test; only the lab check covers exit 3. Exit 255 and "other exit stops" are untested, and the design gives no message for "other exit" (a `die` in node.sh exits 1, with its text on stderr). Also "exit 3 with `not running`" is ambiguous when exit 3 comes with other output (it then refuses as unexpected, which is right but unsaid).
Fix: put the mapping in a small lib.sh function (or fold it into `bench_attached NODE VOL RC`) so a vector covers rc 3 + `not running`, rc 3 + other text, rc 1, and rc 255. Or say in the design that only the lab check covers it, and name the "other exit" message.

### R6-N1 - Nit: case of the checked string
The spec and design vector check for `unmount it if it is mounted`, but the design's message begins "Unmount it if it is mounted, then run ...". A case-sensitive check would fail. Make the vector string match the message (or check with `grep -i`).
