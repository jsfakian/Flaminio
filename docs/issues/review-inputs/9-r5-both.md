# Plan review: issue #9, round 5 (incremental), design-code + design-security

## Earlier findings
- R4-A: fixed. Two patterns, die otherwise, distinct tokens, id printed after the regex; vectors fit the fake socket (CONTROL_SOCKET under FLAMINIO_SCRIPT_TEST=1, guest() in node-scripts-test.sh). One gap: R5-B2.
- R4-B: fixed in substance (`VOL=V scripts/lab.sh vol detach <letter>` is runnable: lab.sh:720 `vms_of "${2:-a}"`, VOL from env). Residual: R5-B3.
- R4-C: fixed (zero-chunk refusal with a vector; three-cause message).
- R4-D: fixed (40 s poll covers the trap's 30 s node detach).

## New findings

### R5-B1 — Low: a stopped node unit makes `bench_attached` refuse
`node.sh attached` prints `not running` and exits 3 when the unit is down (design step 3; node.sh `running()`, lines 95-99). The design says this "means that nothing is attached", but `bench_attached` reads stdin and passes only `none-attached` and `volume VOL`; `not running` falls to "unexpected answer". Nothing says the caller maps exit 3 to `none-attached`, and no vector covers it. A bench run on a node that was stopped (after `node-fault`, `lost`, a `node.sh down`) would then refuse, although step 4 would start it.
Fix: say who handles it. Either the caller turns rc 3 into `none-attached` before piping, or `bench_attached` accepts `not running`. Add a `bench_attached` vector for `not running`, and a lab check (node stopped, `make bench-reads` proceeds).

### R5-B2 — Low: the "empty answer" vector as the fake server serves it
The fake server (node-scripts-test.sh, python block) answers `error: no answer` when the `answers` file is empty, so an empty `answers` does not test an empty answer. The case needs a file holding one blank line (`echo >"$s/answers"`). Otherwise the vector passes for the wrong reason.
Fix: state that in the design's vector list. Also state that each vector sets `answers` after `fresh`, which writes `ok 2 replicas deleted` there.

### R5-B3 — Low: the stop message advises detaching a CSI volume
`bench_attached`'s message gives `scripts/lab.sh vol detach` for any V, `pvc-…` volumes included. For a volume a pod has mounted, node.sh `detach` has no mount check (the check at node.sh:24-27 is on the `down` path). The node's `kill_dev` would wait on the mounted device and the detach would fail after 30 s (control.rs:205-211), leaving the device half-deleted under a live filesystem.
Fix: when V is not a lab volume (for example it starts with `pvc-`), say "delete the pod that uses it" in place of the detach command, with a vector. Alternatively say in the design why the plain command is acceptable.

### R5-B4 — Nit: how `bench_attached` gets "NODE's letter" is unspecified
The message needs `b` from `fl-b`. The design does not say how (`${NODE##*-}`?) or what happens for a NODE name that has no letter. Name the derivation, and test it with the `fl-b` vector (already there).

Reviewed commit: a3bb571386131fdaa416d3cb263bc091999adf27
