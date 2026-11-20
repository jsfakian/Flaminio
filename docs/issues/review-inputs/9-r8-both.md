# Plan review: issue #9, round 8 (incremental), design-code + design-security
Reviewed commit: 6af6e5b796fa774b59fa17ca925f7fad599c9a5a (diff f1145d4..0b109f3)

R7-A: fixed. design.md Files touched now lists scripts/tools/lab-deletion-test.sh (drop valid_id and lost from the extract list, line 21); the test sources lib.sh at line 14, so the moved functions stay defined. decisions.md records it.

Other extractors: none. Only lab-deletion-test.sh extracts from lab.sh (grep of scripts/, Makefile, .github). Other `lost` hits are local variables (demo.sh, lab.sh:1143) or prose. valid_id has no other user outside lab.sh (lines 1735, 1814).

New findings: none.
