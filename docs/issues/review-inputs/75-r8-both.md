# Issue #75 plan review: round 8 (incremental, design-code and design-security)

Scope: git diff 4663835..02853e8 (design.md, decisions.md), checked in /tmp/flaminio-plans-2026-11-12/75.

## Round-7 findings
- R7-B1 (trap): fixed in substance. The trap rule and the two test cases match the finding. Residual in R8-1.
- R7-B2 (wait after each VM): fixed in intent. The wait is not specified exactly (R8-2, R8-3).
- R7-C1 (unique probe ids): fixed. Ids `qos-probe-<vm>-<T>` pass `valid_id` (lab.sh:1633). The tombstone refusal is at service.go:372-379. A residual is in R8-4.
- Nit 1: fixed. `errStatus` (service.go:133-152) turns a store error into `Unavailable`, and the S5 test row says so.
- Nit 2: fixed.

## Findings

### R8-2 - Low: `META_WAIT` does not exist, and `wait_for` cannot "name the undo"
- `grep META_WAIT scripts/` finds nothing. lab.sh:643 passes the literal `60`. The design (:221, :240) calls it an existing, overridable knob. It never says where it is defined or what its default is. The test needs it to be short.
- `wait_for` (lib.sh:211-219) calls `die "timed out after Ns waiting for $what"` inside itself and exits the process. The caller cannot append the undo (`make meta-flags QOS=0 VM=<vm>` or `make meta-up`) unless it is in `$what`, or the call runs in a subshell and the caller handles the failure.
- `wait_for` sleeps 5 s per iteration. A "short" `META_WAIT` in the test still costs at least 5 s per run.
- Fix: define `META_WAIT` (default 60) in lab.sh. Put the VM and the undo into the `wait_for` description, or wrap the call as `(wait_for ...) || die "<vm> ... undo ..."`. Say so in the design.

### R8-1 - Low: how the trap finds "the first instance that answers" and "NotFound" is unspecified
- metactl exits 1 for every error and prints `flaminio-meta: rpc error: code = ...` on stderr (main.go:77-79). vssh does not give the status code.
- The design does not say how the trap separates NotFound (done) from Unavailable (try the next instance), or what it does when no instance answers.
- A trap that treats any non-zero exit as "try the next instance" is wrong in both directions.
- Fix: state that the trap matches `code = NotFound` in the combined output, treats other errors as "try the next instance", and logs the leaked ids when none answers. Add that to the test.

### R8-4 - Low: two runs in the same second collide, and the trap then deletes the other run's probe
- The id is `qos-probe-<vm>-<unix time>`. A second run in the same second gets `AlreadyExists`.
- The failed run's trap deletes "every id this run passed to create", which includes the other run's live probe.
- A rerun in the same second after a successful run fails on the tombstone (service.go:372-379).
- Fix: add `$$` or a random suffix to T. `valid_id` still accepts it.

No other defects in the diff.
