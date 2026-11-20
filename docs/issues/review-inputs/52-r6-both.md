# #52 plan review, round 6, focus design-code and design-security

Scope: `git diff f4a3f1c..6f34da0 -- docs/issues/52/ ':!docs/issues/52/report.md'`. Checked against
the unchanged S7 resume rules (spec.md:82-86, design.md:173-180, contracts.md:424-431), lab step 4
(design.md:385-387), AuthorizeSnapshotRead's idempotence (contracts.md:126-129), the reader liveness
bounds (contracts.md:305-308), and origin/main's `scripts/lab.sh` (`issue`, `put_file`, `cmd_pki`).

## Earlier findings
- R5-C1: fixed. The worker model follows a refused, barrier-closed or lost session with a registration, and the sim test registers every source first.
- R5-C2: fixed. The exits require `completed` with its own root, the abort path exits 1, a closed grant with no outcome takes the first close, and `verify` closes. The new abort rule breaks resume (R6-C1).
- R5-B1: fixed. `Refused` now carries `retry` or `closed`. Ends that are not a `Refused` are not classified (R6-C2).
- R5-C3: fixed. Nothing matches a message, and an unchanged `isr_ver` bounds the reselection.
- R5-S1: fixed. The worker check comes first in Register and in the worker rule (so in Close too), with tests.
- R5-C4: fixed. The extracted `cmd_pki` test asserts the `ca.crt` re-put on fl-w only.
- Nit: fixed.

### R6-C1 — High
**Under "every failure ends with its abort close", a backup that hits an ordinary failure can no longer be resumed, which S7 and lab step 4 need.**
- Evidence:
  - The new rule is "every failure that ends `backup` after it read its grant file and before its root is published ends with its abort close, ... retried" (contracts.md:452-455). That includes a store outage, a SIGINT during reads and a metadata identity error, not only a grant the worker can no longer read.
  - The aborted grant is closed. A rerun with the same grant file cannot register: "a grant closing or closed refuses every registration" (contracts.md:458). It then "resumes at the close" with the aborted outcome (contracts.md:476-477).
  - A new grant does not help. Re-authorizing with the same operation id returns the closed grant (contracts.md:127-128). A new operation id fails the checkpoint's binding: "binds SnapshotRef/grant/repository/operation" (design.md:176).
  - This contradicts:
    - S7: "unreachable store ... retry completes without reuploading verified arrived content", and "Resume uses the same operation/source identity" (spec.md:83-86).
    - Lab step 4: the store stopped mid-run, "the rerun completes, reusing what arrived" (design.md:385-387).
    - The failure matrix's "a fault at each put/checkpoint/root/close step ... resume reuses verified arrived versions only" (design.md:303).
  - A lost root reply followed by a store failure is a further case. The worker cannot tell whether the root was published (design.md:180), so it aborts. The archive may then exist with the grant recorded `aborted`, and S7's "lost final acknowledgements resolve by exact root/operation identity" can no longer reach the `completed` close.
- Fix:
  - Close `aborted` only on the failures that leave the grant unusable: the registration end rule (no ISR member registers at an unchanged `isr_ver`), or a grant answered closing or closed.
  - Every other failure (store, interrupt, metadata unavailable) exits 1 with the grant active and its checkpoint kept for the rerun, as before this round. Its grant, if a deletion's intent made it closing meanwhile, is closed by that rerun or by `snapshot-read close --abort`, as a risk named. The worker model, which has no store failures, keeps its abort rule.
  - In `failure_matrix.rs`, a store fault mid-run followed by a rerun with the same grant file that completes, and a closing-grant refusal that aborts.

### R6-C2 — Low
**The session rule covers only a `Refused` and a session past `Accepted`, so connections that end some other way have no defined reaction, and some end in a wasted registration.**
- Evidence:
  - The worker registers again "only after a `Refused` of reason `closed`, or after a session past its `Accepted` ends without the worker's Close" (contracts.md:460-462).
  - A connection that fails before `Accepted` matches neither case: the registered source's node is down (connection refused, TLS failure), or the node closes it under a liveness bound (the 32-pending eviction, the 5 s handshake limit; contracts.md:306-307, where no `Refused` can be sent before TLS completes). Under "only after", the worker may not reselect another ISR member. Then:
    - A chunk whose chosen ISR member is down (still in the ISR, for example on an idle volume) stalls, against S4's "reselect/revalidate" and "a failover does not stop such reads" (spec.md:63-64; lab step 5).
    - A closing grant with no other session left reaches the worker only through this path. That reopens R5-C1 for this window, and the worker model's "loses (its node down)" (design.md:224-225) is broader than the CLI.
  - The 60 s idle close (contracts.md:307) is a liveness bound, but it ends a session past `Accepted` without the worker's Close. Each one therefore costs a registration (a signing and an etcd write), against R5-B1's "a throttle costs no registration" (design.md:86-87).
  - No reason is given when ValidateSnapshotRead fails transiently (`Unavailable`, or the node's metadata client down). "The token refused by ... ValidateSnapshotRead" is `closed` (contracts.md:337-338), which makes a metadata blip cost a re-registration per Hello.
- Fix:
  - The node ends an authenticated session it closes for idleness with `Refused` (`retry`), and a transient ValidateSnapshotRead failure answers `retry`.
  - The worker treats a connection ending before `Accepted` without a `Refused` as `retry` toward the same node, then, after a bounded number of tries (proposed), registers another ISR member of that chunk.
  - Add cases to `closing_grant_aborted` and `validation_rate_bounded`.

## Nits
- contracts.md:483 still says `--compare-source` "requires a live registered reader grant", but `verify` now registers its own sources under a grant of its own (contracts.md:465-467): write "a live reader grant of its own".

Reviewed commit: eb95fa0837d586dd7a083db7153114c6e83e3362
