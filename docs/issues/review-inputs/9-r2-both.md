# Plan review: issue #9, round 2 (incremental), focuses: design-code and design-security

Scope: `git diff e0c91cd..d5392c8 -- docs/issues/9/ ':!docs/issues/9/report.md'`, and the code
and documents those lines rely on. I checked every quoted figure against
docs/benchmarks/2026-10-29-reads-qd1-queues.md and the four `zero-copy-*-summary.txt`. They
match: 84.7, 87.9, 88.5, 90.2, 98, 86 and 100%; 76.4, 73.5 and 71.9 µs; gaps of 4.5 and 1.6 µs;
gains of 3.8 and 2.3 points; and about 89% for the prototype. The one exception is N1.

## Earlier findings

- R1-C1 (figures): fixed. The re-derived ranking is new finding R2-C1, and N1 is a rounding slip.
- R1-C2 (what closes #9): fixed. The new condition is under-specified: R2-C2.
- R1-C3 (untested S2 and S5): fixed. The SPAN×2 stop gives "the traced read … failed" whether
  fio clamps to the device or fails (bench-reads.sh:74-76). A misspelt tracepoint fails in
  bpftrace, and the step prints the tail and "the trace … failed" (:68-72). The `parse` test
  fits node.rs's `tests` (node.rs:475-495, default at :140).
- R1-C4 (followups): fixed.
- R1-C5 (the build measured): fixed. The provenance sentence guesses: N2. #15's plan
  contradicts the requirement: R2-B3.
- R1-S1 (the bench writes any volume): fixed by S7. The new separate volume breaks the lab's
  one-volume assumption: R2-B1 and R2-B2.
- R1-S2 (VOL unvalidated and unquoted): fixed. Moving `valid_id` is safe: lab.sh sources lib.sh
  at :5, and its callers (lab.sh:1735, :1814) keep it. No other script defines it, and
  lab-deletion-test.sh:21 then finds lib.sh's copy.
- R1-S3 (replicas instead of ISR): fixed. `chunks_in_isr` matches the target's source rule
  (target.rs:1644-1652). `chunks_on` stays for drain-test (lab.sh:1272, :1283).
- R1-N1, R1-N2, R1-N3: fixed.

### R2-B1 — Medium
**The bench's own volume clashes with the lab's single test volume. After a bench run, other
lab checks stop, and vice versa.**
- A node serves one attached volume at a time. The control socket answers "X is attached here;
  one volume at a time" (control.rs:94-95; followups.md item 5).
- `release_lab_volume` (lab.sh:1083-1088, "the lab's test volume lets go") and `cmd_vol attach`
  (:707-718) detach only `$VOL`.
- `cmd_bench_reads` never detaches its volume at the end (lab.sh:1491-1590).

Until now the bench and the lab checks shared `$VOL` (vol1). Under S7 they differ by default:
`bench-reads` against lab.conf's vol1.
- **After `make bench-reads` (NODE=b),** everything that attaches vol1, or a CSI volume, to fl-b
  stops at the attach. That covers node-fault (lab.sh:772), partition (:824-827), the csi checks
  and bench-pod (:1105, :1145, :1348), and chaos when its target is fl-b (:1431, :1456).
- **The other way round:** with vol1 attached on fl-b (after node-fault, for one), the bench
  stops at its own attach.
- **The spec's own sequence:** its S7 lab check `VOL=bench-qd1 make bench-reads` (spec.md:106)
  leaves bench-qd1 attached. A later default run, or #15's or #75's re-measurement on another
  `bench-` name, then fails.
- **Writes:** `make bench-writes` deletes only `$VOL` and wbench. It stops on a `bench-reads`
  volume with replicas on fl-b: "fl-b holds replicas of bench-reads, and the campaign formats
  its disk" (write_start, lab.sh:2139-2154).

Each case stops loudly, and no data is at risk. But the design (design.md:126-165) gives
neither the bench volume's lifecycle nor any of these interactions.

**Fix:** in the design's next step:
- the bench detaches its volume when it ends (a trap, so an interrupted run also lets go);
- before attaching, it asks NODE for `status`. If another volume is attached there, it detaches
  it when that volume is lab.conf's test volume or `bench-*`, and otherwise stops, naming it;
- write_start deletes `bench-*` volumes as it deletes `$VOL`, or the docs give
  `VOL=bench-reads make vol-delete` before a campaign.

Add a lab check: `make bench-reads`, then `make node-fault`, both pass.

### R2-B2 — Low
**Creating the volume trusts the preferred node without the checks write_volume needs.**
- `domain.Place` drops `prefer` when NODE is not a candidate (domain.go:366-368). A candidate is
  alive, not a witness and not draining (service.go:398-400).
- The design creates the volume right after a possible restart (`cmd_node up` when the flags
  differ) and `ensure_nodes`, neither of which waits for NODE's heartbeat (lab.sh:781-786). An
  interrupted drain-test also leaves fl-b draining.
- write_volume waits for `alive` and then checks that every chunk has a replica on NODE, for
  this very reason (lab.sh:1878-1901).

In #9's design, the volume would be created on fl-a and fl-c. The ISR check would then refuse
with the wrong cause ("a replica resyncing, or an ISR a restore left elsewhere"). Every later
run would refuse too, because the misplaced volume exists and is never created again.
design.md:137-139 and spec.md:71 ("so that every chunk has a replica there") state the
preference as a guarantee.

**Fix:**
- before `create`, wait for NODE to be alive and not draining, as write_volume does;
- after it, refuse unless every chunk's `replicas` hold NODE, saying that the volume was placed
  without NODE and must be deleted (`VOL=… make vol-delete`).

Add a script-test vector for the post-create check.

### R2-C1 — Low
**The re-derived ranking's criterion contradicts its own numbers.** design.md:290-300 ranks
the candidates "by what each leaves of the gap", says "None closes the gap alone", and has
trimming "leave the least". But:
- **Item 3** works on the kernel's 4.8 µs, of which raw pays only 1.1 µs (design.md:297-299).
  So up to about 3.7 µs is within its reach, more than the 4-queue gap of 1.6 µs. On that
  criterion it would rank first, and could close the 4-queue gap alone. (Batched commands
  batch nothing at QD1. BPF completion skips both wake-ups and the user space.)
- **With one queue,** `none` alone (1.2 µs) leaves 3.3 µs and trimming (1 µs) leaves 3.5 µs, so
  trimming leaves the least only at 4 queues.

The same claims are in intent.md:109-111 and decisions.md:35-38.

**Fix:** rank by what is measured and how much work each costs. Say that item 3's saving is
unmeasured and could exceed the 4-queue gap. Drop "None closes the gap alone", and limit
"leaves the least" to 4 queues.

### R2-C2 — Low
**"What closes #9" (intent.md:59-74) does not say how the 90% is read, and it can close on a
run whose QD32 misses the plan's goal.**
- **Rounding:** `bench_summary` prints the share with `%6.0f%%` (lab.sh:1620), so 89.5–89.9%
  shows as "90%". The plan's path is close to that boundary: trimming leaves about 0.6 µs at
  4 queues, about 89.2%.
- **Stability:** a run flagged `UNSTABLE (spread > 10%)` (:1619) still "records" a share.
  Neither the intent nor the issue amendment (design.md:352-353) excludes it.
- **QD32:** a `QUEUES=4` run would close #9 with QD32 at 86% (zero-copy-q4-summary.txt).
  performance-plan.md:224 sets the goal as QD32 and 1M at 90% or more, "with QD1 kept at
  90%". performance-plan.md:31 still says "QD1 does not by default". "The run's own queue
  count" is decided (decisions.md), but those two lines then contradict a closed #9.

**Fix:** define the closing value as the medians' ratio, unrounded, ≥ 0.900. Require no
UNSTABLE flag and a passed end-to-end check. Either require the same run's QD32 and 1M rows
to be ≥ 90%, or have the Documents amend performance-plan.md:31 and :224 to say that QD1 may
meet the goal at a queue count that costs QD32.

### R2-B3 — Low
**Cross-plan: #15's measurement reads on vol1, which #9 now forbids.** design.md:308-309 says
#15 "re-measures … on a `bench-` volume (S7)". #15's S12 reads "first, on `vol1`, as the
write campaign reformats fl-b" (flaminio-15 design.md:950-951). Two outcomes:
- if `bench-ab` reuses `cmd_bench_reads`, S7 refuses vol1;
- if it does not, it detaches and attaches vol1 with no S7 guard.

Moving it to a `bench-` volume then trips write_start (R2-B1). #9's report acknowledges that
"those plans need that change", but nothing records it where #15 will see it.

**Fix:** put the requirement in #15's plan, or in a comment on #15, together with the
write_start exemption from R2-B1. In #9's "What is left", say that it is pending there.

## Nits
- **N1:** design.md:233 says "the bench's runs differ by 2.9 µs". 1e6/13 097 − 1e6/13 602 is
  76.35 − 73.52 = 2.8 µs. The 2.9 comes from subtracting the rounded values.
- **N2:** design.md:325-326, "They ran with #28's bench (186ef09, 7afa35d)", is the kind of guess
  the same sentence rules out. 7afa35d changed bench-reads.sh, lab.sh and lib.sh in the very
  commit that recorded the results, so which bench produced them is not known. Say "186ef09's
  bench, with or without the fixes 7afa35d made".
- **N3:** spec.md:94-102 keeps `VOL=volb make bench-reads` as S1's lab check, and S7 refuses that
  command. Add the runnable form: a `bench-` volume, with the range its `regions.txt` shows, raw
  from that range's start, and a spread of 10% or less.
- **N4:** design.md:162-163 has lab.sh's usage line say `VOL (bench-reads)`. Only the Makefile
  sets that default (design.md:148). `scripts/lab.sh bench-reads` gets lab.conf's vol1 and is
  refused. The usage line should say that `make` supplies the default.
- **N5:** intent.md:56 says "cannot move or overwrite a volume it did not create". S7 checks
  the name only (spec.md:67-69), so a `bench-…` volume made by hand is taken. Say "a volume not
  named `bench-…`".

Reviewed commit: a01f2fb0b1178fb46b3b36092d0350a25f43c151
