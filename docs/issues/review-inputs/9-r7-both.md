# Plan review: issue #9, round 7 (incremental), design-code + design-security

## Earlier findings
- R6-A: fixed. `bench_attached NODE VOL RC` maps 255 (lost), 3 (only `not running`), 0 (answers), other (refuse, message named); vectors cover RC 0, 3 (both outputs), 1, 255. Residual: R7-A.
- R6-N1: fixed. Spec, design vectors and the message all read "Unmount it if it is mounted" / "Delete the pod that uses it"; checks are case-sensitive.

## New findings

### R7-A — Medium: moving `lost` (and `valid_id`) out of lab.sh breaks lab-deletion-test.sh
`scripts/tools/lab-deletion-test.sh:15-29` extracts functions from `lab.sh` by name with an awk `extract`, and its list (line 21) includes `valid_id` and `lost`. If either function is no longer defined in lab.sh, `extract` prints nothing, `declare -F` fails and the test exits with "FAIL: lost not found in lab.sh". That test runs in `make lint`. The plan moves both functions and its file list (design "The files the next step touches") does not mention lab-deletion-test.sh. (The same defect already exists for `valid_id` from the earlier round; it was never caught.) All other callers are fine: lab.sh sources lib.sh at line 5, `lost` reads only `VM_C` (lab.conf:61, sourced by lib.sh) and `LOCAL_STATE_DIR` (lab.conf), and `warn` (lib.sh); the only lab.sh caller is line 724. The test already sources lib.sh (line 14), so the moved functions are defined there.
Fix: list `scripts/tools/lab-deletion-test.sh` among the touched files and remove `valid_id` and `lost` from its extract list (lib.sh already supplies them); note that its "lost" cases (fake ssh touching `$w/lost/…`) still go through the real `lost` and need `VM_C`/`LOCAL_STATE_DIR`, which lib.sh sets.

Reviewed commit: f1145d4811d9a3369a5c4a65f788ae6fccc4e256
