# Plan review: issue #9, round 1, focus: design-code

Scope: docs/issues/9/{intent,spec,design,decisions}.md at HEAD. The plan is byte-identical to
docs/9/ at 7afa35d (PR #28), so it was written before the bench runs it specifies. The code
matches the design: bench-reads.sh:41-87, lib.sh:220-246, bench-reference-test.sh (run: ok),
lab.sh:1523-1581, node.rs:49-55, provision.sh:10, Makefile:287, lab.md:424, and libublk 0.4.8's
set_thread_affinity. What disagrees is the text: some figures were never updated to the
recorded results.

### R1-C1 — Medium
**The plan's QD1 figures contradict the recorded bench results, and its reasoning rests on the
old ones.** The plan quotes preliminary hand measurements against 15.35k. The bench runs of S4,
recorded in docs/benchmarks/2026-10-29-reads-qd1-queues.md and its data/*-summary.txt, give
different values:
- **What 90% needs with 4 queues.** design.md:196-198 and intent.md:80-81 say 1.0–1.4 µs (72.4 µs
  per read), "so trimming would come close". The results say 1.6 µs (71.9 µs from a raw of
  about 15.45k; results:115-117), and so does the issue's comment.
- **Polling on one queue.** intent.md:57 says it gains "about 5 (84.7% to 89.9%)", and
  intent.md:37-38 and design.md:224 say 89.9%. The bench measured 13 677 against 15 458, which is
  88.5%, so the gain is about 3.8 points (zero-copy-q1-poll100-summary.txt; results:54).
- **Polling on 4 queues.** intent.md:56 says "1–2 points". The bench measured 87.9% → 90.2%,
  which is 2.3 points (q4 and q4-poll100 summaries).
- **The in-flight spinning prototype.** intent.md:37 and design.md:223 say 89.4–90.0%. The
  results say "about 89%" against the raw runs beside it (results:109-111).
- **4 queues.** design.md:176-177 gives "84.7% to 88.3–88.7%". The bench gives 87.9%.

These numbers are the rationale for the out-of-scope list and for the next-candidate ranking.

**Fix:** replace them with the bench's figures and keep the October and hand values only as
labelled history. Then re-check the "trimming would come close" sentence against 1.6 µs.

### R1-C2 — Low
**The plan does not say what closes #9, and a recorded run already meets the issue's literal
"Done when".** "Done when" is "≥ 90% of raw at QD1 on fl-b, by the plan's measurement rules
(`make bench-reads`)". `QUEUES=4 POLL_US=100 make bench-reads` recorded 90.2%
(zero-copy-q4-poll100-summary.txt; results:55). The intent only says "not met here without
spinning" and "stays open" (intent.md:48-50). The criterion the plan applies (default settings,
or at least no polling) is written in neither the issue nor the plan.

**Fix:** state the closing condition in intent.md, for example "at the node's defaults" or
"without --poll-us", and amend the issue's "Done when" to match.

### R1-C3 — Low
**Two spec rules have no named test or check.**
- **S2, first bullet** (spec.md:18): the trace cannot run, because bpftrace fails, or it has no
  end marker, or the traced read fails or reads less than SPAN. The design adds the removal of
  the stale fio report specifically so that a failed read is not reported as "under 90%"
  (design.md:33-41). The tests cover only under 90% and over 110% (spec.md:72-76).
- **S5** (spec.md:41-47): the usage text and the default of 1 for `--queues`. No parse test
  checks the default (node.rs:140).

**Fix:** add hand stops on fl-b:
- `range` with a SPAN larger than DEV: fio reads less, so the step must say "the traced read …
  failed" and not "under 90%";
- a bpftrace program error, for example a disk with no `/sys/class/block/<name>/dev`, which must
  print the trace's tail.

Also name a check for S5: a `parse` unit test that the default is 1, plus a look at the usage
text.

### R1-C4 — Low
**docs/followups.md is missing from the Documents section and still shows the old numbers.**
docs/followups.md:13 says "QD32 (95%) … only 4K QD1 reads (84%, #9) remain". Against the new
reference the bench gives QD32 98% and QD1 85% (performance-plan.md:22-23). AGENTS.md requires
docs/followups.md to match results. The design's Documents list (design.md:233-260) does not
include it.

**Fix:** add followups §1 to the Documents list, with the new figures and a link to the results.

### R1-C5 — Low
**Cross-plan: #9's figures are not tied to a build, and the plans that change the read path
will move them without re-measuring the 4-queue and polling rows.**
- **#15** adds an engine thread that shares the engine lock with the queue threads, a generation
  check on `try_fast_read`, `plan_fast_read` and `finish_fast_read`, and drops the per-queue
  timer tasks. Its S12 accepts up to 3% lower zero-copy QD1, measured with the default single
  queue only (flaminio-15 design.md:248-252, 255-279, 945-960; spec.md:192-198).
- **#75** adds a QoS check in `try_fast_read`. Its cost is argued, not measured (75
  design.md:119-121, 322).

After #15 and #75 land, the Status figures, the "4 queues" line, and "what 90% needs" may no
longer hold (performance-plan.md:22-33; design.md:196-199). Those are the inputs to #9's next
step. #74 does not affect them, because zero copy stays in direct mode (74 design.md:200-201).

**Fix:**
- record the commit measured in the results document, as W4.3 does (performance-plan.md:95);
- say in the design's "What is left" that the next step first re-measures QD1 with
  `QUEUES=1/4` and `POLL_US=0/100` on the build that contains #15 and #75.

## Nits
- **N1:** the design says the usage line names `VOL` with its default, vol1 (design.md:113-115).
  scripts/lab.sh:2243 has `VOL` without "(vol1)" and leaves out `POLL_US`, which the Makefile
  help lists.
- **N2:** intent.md:14 says "`make bench-reads` reads the raw disk in a different 16 GiB region
  in each repetition" in the present tense. Since PR #28 it does not (lab.sh:1565). It should
  say "read, before #28".
- **N3:** intent.md:59 says "`--poll-us` stays a setting for latency-sensitive volumes". It is
  a node-wide flag (node.rs:44-46; ublk.rs:1011 `poll` comes from `c.poll_us` for every
  device). With #15's several volumes per node, every busy queue of every device spins. It
  should say "per node", as performance-plan.md:35-37 does.

Reviewed commit: e0c91cdcf0a0c89e432546ba290239683ba143a5
