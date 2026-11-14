# Issue #74 — report

## Review rounds

### Plan review round 1 — 9d1db414e1f83bc77be64b57f5abd48e079c4984

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74c1 (design-code), r74s1 (design-security).

- **R1-C1 — High:** `UBLK_F_UNPRIVILEGED_DEV` removes user recovery, owner-only opens break raw block volumes, copy modes refused. Evidence: 6.8.12 `ublk_drv.c:2396–2414`, `:768–790`; `ublk.rs:906–907`.
- **R1-C2 — High:** broker-made rings leave the store without rings, cap FDs below queues, bind single-issuer rings to the broker; opcode list incomplete; "FD whitelist" undefined. Evidence: `uring.rs:138–143`, `node.rs:203`, `io_uring/register.c:229–236`, `ublk.rs:1079–1085`, `:1352`.
- **R1-C3 — High:** the registry pins `dev_t`, which changes across reboots. Evidence: design.md:16–18, 28–31; `superblock.rs:26`.
- **R1-C4 — Medium:** work assigned to #43, #16, #73, #80 plans that exclude it; `Resize`/`Recover` absent from code. Evidence: #43 intent.md:87, design.md:150; #16 plan.
- **R1-C5 — Medium:** the helper's metadata identity has no authorization design. Evidence: `meta/service/auth.go:58–84`; `token.rs:15–23`.
- **R1-C6 — Medium:** "unprivileged discovery" contradicts the agent's raw-device probes. Evidence: `control/agent/host.go:18–66`.
- **R1-C7 — Medium:** the helper socket's directory is the one the I/O process must write; the CSI hostPath exposes it. Evidence: `node.rs:137`, `control.rs:46–52`, `scripts/guest/node.sh:61–63`, `deploy/csi/driver.yaml:167`.
- **R1-C8 — Medium:** ledger crash ordering, kernel identity and reboot behaviour unspecified. Evidence: design.md:49–52.
- **R1-C9 — Medium:** no switch or default for hardened mode; direct callers not listed. Evidence: `node.sh:45`, `ublk.rs:141`, `cli.rs:99`, `node.rs:160`.
- **R1-C10 — Medium:** tests named by category; lab plan not runnable; S3, S5, S8, S10 clauses untested. Evidence: design.md:170–188.
- **R1-C11 — Low:** helper capability set does not match its operations. Evidence: design.md:43–44, 66–68.
- **R1-C12 — Low:** inventory of privileged call sites incomplete. Evidence: `ublk.rs:129–131`, `:626`, `:902–934`, `control.rs:202`, bench and disktool.
- **R1-C13 — Low:** pinning the SLSA builder by commit conflicts with the generator. Evidence: design.md:105–106 (from memory).
- **R1-C14 — Low:** third-party sidecar images outside the digest rules. Evidence: `deploy/csi/driver.yaml:64`, `:73`, `:78`, `:138`, `:150`.
- **Code nits N1–N4 — Nit:** `uring.rs` range; "incarnation" vs `first_epoch`; ARCHITECTURE §5.1 not named; `SO_PEERPIDFD`. Evidence: as cited in the findings.
- **R1-S1 — High:** the root CSI node plugin formats and mounts whatever device path the unprivileged I/O process answers. Evidence: `control/csi/dataplane.go:86–98`, `node.go:58–126`, `mount.go:40`, `:59–65`.
- **R1-S2 — High:** io_uring descriptors cannot be sent with `SCM_RIGHTS`; single-issuer and store-ring gaps. Evidence: `net/core/scm.c:107–111`; `register.c:234`; `uring.rs:138–143`.
- **R1-S3 — High:** unprivileged ublk ownership removes recovery and zero copy, against ADR 0003 and #48. Evidence: `ublk_drv.c:2391–2415`; libublk-rs-sys `ublk_cmd.h:223`.
- **R1-S4 — Medium:** peer authentication by cgroup unspecified; socket in a shared, hostPath-mounted directory. Evidence: `driver.yaml:132`, `:167`; #12 spec.md:48; `node.rs:82`.
- **R1-S5 — Medium:** the registry's write path has no design; agent probes need raw reads. Evidence: design.md:18–19; `host.go:18–47`.
- **R1-S6 — Medium:** registry values do not survive a reboot. Evidence: design.md:16–17, 28–31.
- **R1-S7 — Medium:** "stale" undefined per operation; tokens never expire. Evidence: spec.md:27–30; `captoken.go:28`.
- **R1-S8 — Medium:** ledger crash ordering and kernel identity unspecified; no file versions. Evidence: design.md:49–51.
- **R1-S9 — Medium:** the `CAP_SYS_ADMIN` broker becomes a network client. Evidence: design.md:37–39, 93; `captoken.go:43–51`.
- **R1-S10 — Medium:** `verify.sh` has no trust anchor. Evidence: design.md:125–127, 143–147.
- **R1-S11 — Medium:** runtime gate evidence has no trustworthy source. Evidence: `.github/workflows/ci.yml:6`, `:24`.
- **R1-S12 — Medium:** installing the root helper assigned to #43, which has no host service. Evidence: design.md:15–16; #43 spec.md:98.
- **R1-S13 — Medium:** rules and abuse cases without a test. Evidence: design.md:170–177.
- **R1-S14 — Low:** client messages may carry descriptors. Evidence: design.md:158.
- **R1-S15 — Low:** a handed-out disk descriptor outlives its grant. Evidence: design.md:52–53.
- **R1-S16 — Low:** "independent" rebuild undefined. Evidence: design.md:117.
- **Security nits — Nit:** "public" control socket; "incarnation"; `ublk.rs:168–176` vs `add_device`; "registry source". Evidence: `control.rs:9`, `:51`; `captoken.go:49`; `ublk.rs:902–934`.

Changes/resolution: The plan is now step 1 of #74 (issue step 1 and the I/O half of step 2); intent.md states what stays open (agent privileges, controller images, profiles and io_uring restrictions, storage binding, NetworkPolicies, the release track, disclosure, #12 packaging, #48 recovery). The boundary was rebuilt from kernel facts verified in the 6.8.12 tree (decisions.md lists each, and what stays unverified): a root `flaminio-helper` keeps `/dev/ublk-control` and `CAP_SYS_ADMIN` and starts `flaminio-io node --privsep` as its only child (dedicated UID/GID, no capabilities, `no_new_privs`) on an unnamed socketpair; it passes the disk, the control socket and each ordinary device's `/dev/ublkcN` with `SCM_RIGHTS` and serves add/start/stop/delete for devices it created. Per finding: R1-C1/R1-S3 fixed (ordinary devices, no unprivileged flag; zero copy refused under the helper). R1-C2/R1-S2 fixed (the I/O process creates all its rings; no ring handoff, opcode list or FD whitelist). R1-S1 fixed (new rule S6: the CSI node plugin checks `/dev/ublkb<N>` and the kernel's name before blkid/mkfs/mount; tests with a lying data plane). R1-C3/R1-S6, R1-C5/R1-S9, R1-C6/R1-S5, R1-C8/R1-S8, R1-S7, R1-S15 resolved by the cut of the registry, claim action, metadata identity, ledger, grants and release/erase. R1-C4 fixed (no #43/#16/#73/#80 work assumed; Resize/Recover dropped; #12, #48, #16 dependencies stated). R1-C7/R1-S4 fixed (no named helper socket; control socket bound by the helper as today, root 0600; token file placement stated). R1-C9 fixed (command-line switch, lab default `PRIVSEP=1`, direct mode listed). R1-C10/R1-S13 fixed (tests named per rule; lab commands, pass criteria and restore; `make privsep-check`). R1-C11 fixed (capability sets derived and tested). R1-C12 fixed (every call site listed). R1-C13, R1-C14, R1-S10, R1-S11, R1-S16 resolved by the cut of the release track. R1-S12 fixed (no host service; retained authority listed). R1-S14 fixed (no descriptors in requests; at most one per reply). Nits fixed or resolved by the cut. A coordinator note on #71's telemetry socket is recorded as #71's dependency. adr-proposal.md rewritten (Proposed); decisions.md created.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 96d792d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74c2 (incremental design-code), r74s2 (incremental design-security).

- **R2-C1 — Medium:** S5 fails while the single-threaded helper waits in START_DEV or DEL_DEV: it never sees its child die. Evidence: `ublk_drv.c:2252–2262`, `:2553–2568`; design.md:29, 140.
- **R2-S1 — Medium:** the same defect: ublk control commands run in io-wq and the helper polls the pidfd only between requests. Evidence: `ublk_drv.c:2925–2926`, `:2262`, `:2567`; `scripts/lab.sh:781–786`.
- **R2-C2 — Medium:** a failed attach after `add` never deletes the device; with the fixed `--id` every later `add` gets EEXIST. Evidence: design.md:56–62, 81–86.
- **R2-S2 — Medium:** #12's and #48's drafts do not contain the work assigned to them, and the plan never says Kubernetes stays privileged. Evidence: `flaminio-12/docs/issues/12/design.md:41–44` (ADR 0007); `flaminio-48/docs/issues/48/design.md:98–113`.
- **R2-C3 — Low:** `--device` still required (`node.rs:210`); path-based messages under `--privsep`; `/dev/urandom` is a device path. Evidence: `node.rs:255`, `ublk.rs:1002`, `:1549`, `cli.rs:52`.
- **R2-C4 — Low:** SIZE is not bounded though the spec says it is. Evidence: design.md:149–151.
- **R2-C5 — Low:** client serialisation unstated; a stop queued behind a blocked start. Evidence: design.md:138–140; `control.rs:202`, `ublk.rs:626`.
- **R2-C6 — Low:** START_DEV pid source unstated and untested. Evidence: `ublk_drv.c:2252–2281`.
- **R2-C7 — Low:** `lab.sh:1513`'s reuse comparison omits `--privsep`. Evidence: `scripts/lab.sh:1513`.
- **R2-C8 — Low:** "helper dies, child ends" untested; an idle child never notices. Evidence: decisions.md (PDEATHSIG cleared).
- **R2-C9 — Low:** suppressed partition scanning for raw block volumes not stated or tested. Evidence: `ublk_drv.c:2288–2290`.
- **R2-S3 — Low:** the device bound holds per helper life only; leaked devices exhaust `ublks_max`; no add rate limit. Evidence: `ublk_drv.c:504`, `:2439`.
- **R2-S4 — Low:** SIZE can overflow the kernel's byte size. Evidence: `ublk_drv.c:566–606`; `block/bdev.c:427`.
- **R2-S5 — Low:** the root CSI plugin reads an answer of any length. Evidence: `control/csi/dataplane.go:75`, `:41`, `:82`; `deploy/csi/driver.yaml:101`.
- **R2-S6 — Low:** the writable disk descriptor allows SG_IO write-class commands and persistent reservations. Evidence: `drivers/scsi/scsi_ioctl.c:251–340`; `block/ioctl.c:262–275`.
- **R2-S7 — Low:** the startup token is created world-readable; stale tokens become reachable in a 0711 directory. Evidence: `scripts/guest/node.sh:61–63`.
- **R2-S8 — Low:** ownership by bare id can name a device the helper did not create. Evidence: `ublk_drv.c:2418`, `:2455`; `node.sh:27–29`.
- **Nits (code) — Nit:** titles still name release artifacts; `bench-reads.sh:99` wording; affinity set after start.
- **Nits (security) — Nit:** std's setgid/setgroups/setuid order and ignored `EPERM`; the ADR omits UID 0; Q4 omits udev rules acting on contents.

Changes/resolution: R2-C1 and R2-S1 fixed once: after the capability drop the helper's main thread only waits on the child's pidfd and a server thread answers requests, so the child's exit ends the helper (with cleanup) even while a kernel command waits; kernel test `exits_with_its_child_while_blocked`. R2-C2, R2-S3 and R2-S8 settled in one design section, "Owned devices": ownership is the helper's open descriptor of `/dev/ublkcN`, which keeps the id from being freed; adds are paced; the six outcomes (add fails, attach fails, delete, child exits, helper dies, id reused) each have a closed result and a test. R2-S2: intent.md states once that Kubernetes stays privileged in direct mode until #12 adopts the helper and that #48's recovery is not covered; the ADR and design defer to it. R2-C3: parse and messages changed, S2 reworded, tests added. R2-C4, R2-S4: SIZE at most 2^63 − 4096, tested. R2-C5: client mutex and a fatal `start` limit, tested. R2-C6: pid source stated, tested. R2-C7: `--privsep` compared in `lab.sh`. R2-C8: watcher thread on the helper socket's hangup, tested. R2-C9: stated in Risks and `docs/lab.md`, kernel test. R2-S5: 4096-byte printable answer, tested. R2-S6: stated in the invariant and Risks; the ioctls join step 3's scope. R2-S7: resolved by a cut (`ATTACH` refused under the helper; no token file). Nits fixed. #71's telemetry socket is recorded as #71's dependency on this step, with no mechanism here. decisions.md records each choice and the kernel and std facts checked.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 7245c69

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74c3 (incremental design-code), r74s3 (incremental design-security).

- **R3-S1 — Critical:** a failed `add` makes the helper delete whatever ublk device has the id the child named: libublk's `UblkCtrlInner::new` returns on a failed ADD_DEV and its drop sends DEL_DEV for the requested id. Evidence: libublk 0.4.8 `ctrl.rs:1258`, `:1085–1096`, `:1134`; `ublk_drv.c:2134–2137`.
- **R3-C1 — Medium:** the approval left the plan contradicting itself (stale "None yet" text, ADR "proposed for review", "only if approved", "Nothing here is approved", design "New, Proposed", spec Q2). Evidence: decisions.md:5–6; adr-proposal.md:3–5, :27, :48, :68, :78; design.md:296.
- **R3-S2 — Medium:** outcome 4's cleanup can be defeated: a forked holder of the socket, an add in flight, serial DEL_DEVs, unpaced start/stop. Evidence: design.md:92–97; `ctrl.rs:1576`; `ublk_drv.c:2266–2270`.
- **R3-S3 — Medium:** #74 and #71 contradicted each other on the telemetry socket and neither owned the helper's part. Evidence: design.md:287–289; decisions.md:129–130; `71/contracts.md:32–35`.
- **R3-C2 — Low:** the pin keeps `UB_STATE_OPEN` set and blocks #48's START_USER_RECOVERY; the plan no longer said so. Evidence: `ublk_drv.c:2740–2744`, `:1356`.
- **R3-C3 — Low:** the libublk handle's drop deletes the device; the design did not state disowning or the drop order. Evidence: `ctrl.rs:1085–1096`, `:2790`.
- **R3-C4 — Low:** outcome 5 and the watcher barely tested. Evidence: design.md outcome 5; `helper_hangup_is_seen`.
- **R3-S4 — Low:** a root `del` of an owned device leaves it startable, so a child can hang `node.sh down`. Evidence: `ublk_drv.c:2266–2270`, `:2543–2546`; `node.sh:27–29`.
- **Code nit — Nit:** decisions.md carries workflow state ("Approved before its plan review cleared").
- **Security nits — Nit:** the approval contradicts itself (as R3-C1); the ADR omits the disk descriptor's SG_IO, reservation and passthrough power; the DEL_DEV of a held device is not bounded by what the Availability risk says; the pin must be closed by #48 before recovery.

Changes/resolution: R3-S1 verified in libublk 0.4.8 and fixed by a third change in the vendored patch: when the lifecycle step of `UblkCtrlInner::new` (and `new_async`) fails, the handle is marked deleted so its drop sends nothing; tests `failed_add_sends_no_delete`, `foreign_device_survives_add_of_its_id`, `add_of_an_owned_id_changes_nothing`. Today's direct-mode `add_device` with a fixed `--id` has the same defect; the patch covers it and `node.sh up` now deletes a leftover first. R3-C3 fixed: the handle is disowned, a pin is closed before the helper's own DEL_DEV on a fresh handle (`delete_closes_pin_before_del_dev`). R3-S2: outcome 4 shuts the socket down, waits up to 4 s for the current request, sends each DEL_DEV on its own thread; one `start` per owned device; an add outlasting the wait is left, as stated; tests named. R3-S3: replaced both #71 notes with #71's rule (a later step of #74; `--telemetry` refused with `--privsep` by whichever plan lands second; `TELEMETRY=1 needs PRIVSEP=0`). R3-S4: `node.sh down` stops the unit before `del`; a root `del` while the unit runs is stated as unsupported. R3-C2 and security nit 4: intent and ADR say #48 must close the pin before recovery. R3-C4: `flaminio-io/tests/privsep.rs` `exits_when_the_helper_goes`. R3-C1 and security nit 1: stale approval text removed; settled and still-open questions marked in place in spec, design and ADR. Security nit 2: the ADR names the disk ioctls. Security nit 3: a held device's `delete` stated as unbounded. Code nit: the approval's workflow state dated. Coordinator item: the I/O process's `io` key passing the metadata clients' check is stated under Risks with the follow-up "metadata clients check the `meta` URI SAN". These fixes came after the approval (decisions.md, Approvals).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 2040d58

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74b4 (incremental, design-code and design-security).

- **R4-S1 — Medium:** requests queued before the shutdown are still served after the child's exit, so a compromised child leaks devices at exit. Evidence: design.md:104–111; `net/core/datagram.c:103–105`; `af_unix.c:2361–2366`, `:965`, `:3591`.
- **R4-S2 — Medium:** the lab's stop signals helper and child together, so outcome 4 never runs, and `node.sh` deletes only id 1. Evidence: `scripts/guest/node.sh:13`, `:28–29`, `:65`; design.md:120–121, :330; `ublk_drv.c:504`, `:2439`.
- **R4-C1 — Low:** on the lab's kernels a taken id fails with `EOPNOTSUPP`, not `EEXIST`, after libublk's legacy-opcode retry. Evidence: libublk `ctrl.rs:1500–1520`, `:1571–1582`; `ublk_drv.c:1712–1714`; kernel-noble `debian.master/config/annotations:2199`.
- **R4-C2 — Low:** spec S3 and the ADR say a failed add deletes no device, but outcome 1 deletes the device the add created. Evidence: spec.md:43; adr-proposal.md:36; design.md:91–93.
- **R4-C3 — Low:** the watcher's exit is still untested: the test exits through the EOF-in-call path. Evidence: design.md:122, :148.
- **R4-C4 — Low:** `exits_with_its_child_while_blocked` leaves about 1 s of margin (9 s against 10 s). Evidence: design.md:113–115; `ublk_drv.c:2549–2567`.
- **R4-C5 — Low:** the patch also removes `serve`'s self-healing after a crash; only `node.sh` was compensated. Evidence: `scripts/guest/ublk.sh:14–31`; design.md:246–247.
- **Nit:** the `io`-key text says #71 closed the impersonation for its observer; it was for its Prometheus metadata job. Evidence: design.md:304; `71/design.md:268–276`.

Changes/resolution: Device cleanup is settled in one place (design, Owned devices). R4-S2: the helper adds only at the id its own command line names (`--id`, the lab's 1) and the child names none, so it owns at most one device and the only device a stop can leave is `/dev/ublkc<ID>`, which `node.sh down` deletes after stopping the unit; `node-scripts-test.sh` and `add_uses_the_configured_id` test it; SIGTERM handling was not chosen (no safe signal mask in rustix 1.1.5). R4-S1: at the child's exit `closing` is set before the shutdown and checked before each request, so queued requests are never served and a late `add` deletes its own device; `queued_requests_after_child_exit_ignored`. R4-C1: replies keep libublk's errno, documented as `EOPNOTSUPP` on Ubuntu; tests check what failed and what survived. R4-C2: "deletes no device but the one it added" in spec and ADR. R4-C3: the watcher takes an exit hook, and `exits_when_the_helper_goes` blocks the node on a FIFO outside any helper call, so only the watcher can end it. R4-C4: worst case (b) + (d) = 9 s stated; bounds 15 s. R4-C5: `ublk.sh up` deletes a leftover `/dev/ublkc0` when its unit is inactive; tested. Nit fixed; #71's notes applied (the `meta.flaminio.invalid` scrape check; a node under the helper shows `stale` and raises `FlaminioTelemetryLost`). These fixes came after the approval (decisions.md, Approvals).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 0564254

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74b5 (incremental, design-code and design-security).

- **R5-B1 — Low:** `closing` is checked only before the read, so a request read after it is set is still served; "never served" does not hold. Evidence: design.md:113–116; decisions.md:176; `af_unix.c:2361–2366`.
- **R5-C1 — Low:** the plan does not say that one device per helper gives up several volumes per node (#15), and the ADR fixes "at most one". Evidence: adr-proposal.md:34–37; `ublk.rs:682`; followups.md:127–140.
- **R5-C2 — Low:** the kernel tests now need fixed ids and the plan names none; parallel tests and the lab's ids 0 and 1 could collide. Evidence: design.md:235–236, :298, :303.
- **R5-C3 — Low:** the required, bounded `--id` has no parse test, though the one-device argument rests on it. Evidence: design.md:78–79, :235–236, :277; `node.rs:136`.
- **Nit:** the invariant's "add one bounded device" reads as one per helper lifetime. Evidence: spec.md:21–23.

Changes/resolution: R5-B1: the server thread also checks `closing` under the table lock after each `recvmsg` and drops a request read then unanswered; `queued_requests_after_child_exit_ignored` covers it. R5-C1: design Risks and the ADR's Consequences say one device per helper follows from one volume per node and that #15's several volumes need one owned id per volume, which changes the ADR; followups item 5 gains that step. R5-C2: each kernel test uses `--id` 100 plus its index, never 0 or 1, checked free in `/sys/class/ublk-char` first. R5-C3: `refuses_a_missing_or_bad_id` (no `--id`, `-1`, `1048576`). Nit: "at most one bounded device at a time". No new mechanism. These fixes came after the approval (decisions.md, Approvals).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — b1913c0fe5e3d9778048f76c594cd164833a77db

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74b6 (incremental design-code and design-security).

r74b6 confirmed every round-5 finding fixed (R5-B1 in the text; its test is R6-1), including the sysfs path and the 1048576 bound against the 6.8 tree, and reported:

- **R6-1 — Low:** No test reaches the window the post-`recvmsg` `closing` check closes: `queued_requests_after_child_exit_ignored` passes on the pre-read check alone, and can race a running server thread. Evidence: design.md:111–125.
- **R6-N1 — Nit:** `refuses_a_missing_or_bad_id` should also accept `0` and `1048575`, to catch an off-by-one the other way. Evidence: design.md S1 test row.

Changes/resolution (two test descriptions in design.md, made by the orchestrator; a change after the approval): `queued_requests_after_child_exit_ignored` now runs with the server loop paused on a latch; a new test `request_read_after_closing_dropped` makes the receive an injectable step that sets `closing` and then returns a valid `add`, and asserts no kernel call and no reply; `refuses_a_missing_or_bad_id` also asserts `0` and `1048575` are accepted.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 03b2658ef0b2c45a1fbc2df85912d6fb0b84270a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74b7 (incremental design-code and design-security).

r74b7 confirmed both round-6 findings fixed and reported:

- **R7-1 — Low:** The injectable receive was named only inside a test description; the Reading section did not say the server loop takes its receive as a parameter, that production keeps the ancillary-data, truncation and zero-length checks, or that the test closure runs without the table lock held. Evidence: design.md:70–72, :126.
- **R7-N1 — Nit:** decisions.md (R5-B1) still named `queued_requests_after_child_exit_ignored` as covering a request read after `closing` is set. Evidence: decisions.md:200–202.

Changes/resolution (made by the orchestrator; a change after the approval): one sentence added to design.md's Reading bullet stating the receive parameter, production's checks and the test closure's locking; decisions.md's R5-B1 entry names `request_read_after_closing_dropped` for that case.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — f40e9adafb5ff7cfc41b2b26d50875a5ab405705

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r74b8 (incremental design-code and design-security).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
