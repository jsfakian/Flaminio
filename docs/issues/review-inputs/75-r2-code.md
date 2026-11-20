# #75 plan review, round 2, code focus (reviewed commit 83ae6dd)

Round-1 cut text (lanes, DRR, pools, gates, overrides, SetVolumeQoS): gone from spec, design,
adr-proposal and decisions (only decisions' "cut" notes remain). Code facts checked and holding:
one engine mutex taken per call in ublk.rs (no new lock; no metadata round trip); target
request_timeout starts at emission (target.rs:811-828); FLUSH/DISCARD answered in ublk.rs without
the engine; device privileged (no UNPRIVILEGED_DEV flag, ublk.rs:904-909); tag 40 free in
Volume, CreateVolumeRequest, GetVolumeRequest; gather timer and 10 ms tick exist as described.

## Findings

1. Medium - A head request larger than the bucket cap is never admitted. design.md "The gate"
   (cap = burst x 10^6; "the floors keep every request (at most 1 MiB) coverable"). The 1 MiB
   bound is the frontend's `--io-buf` (node.rs:222); `Engine::submit` validates only alignment
   and range (engine.rs:1694-1709), and `Input::Io` is public. A larger request (another driver,
   a test, a future io-buf) with a limited bytes dimension and a 1 MiB burst makes `due`
   infinite and blocks the FIFO forever, with the tag held. Fix: clamp a request's cost to the
   cap in the Gate (or make Engine::submit refuse > 1 MiB when limited) and add a qos.rs test
   (cost above cap is admitted at a full bucket).

2. Medium - Mixed-version refusal comes after Attach. S5/design: the metadata service refuses
   `GetVolume` to an older `io`, "so its attach fails". But failover, drain and the agent move
   the target with `Attach` (auth.go: agent may call it; epoch bump) and only then does the
   data plane call GetVolume (engine.rs:660-675). A limited volume failed over to an
   older-data-plane node then has a target that cannot start and an epoch already advanced, and
   nothing keeps the controllers from choosing that node again. Spec says only "its pods do not
   start". Fix: state what failover/drain do (retry another node? skip nodes that did not
   declare?) or accept it as a documented limit with a test or lab note; at least say Attach
   leaves the volume without a serving target until a capable node attaches.

3. Low - Fast path needs a non-mutating "covers". The Gate API (offer, take, release, due,
   drain) has no `covers(now, bytes)`; design.md "Fast path" requires the check before
   `rt.sm.try_fast_read` and the debit after all pieces. Add it (it refills but does not debit)
   and test it with `take` in `fast_reads_share_the_bucket`.

4. Low - Deposition path unspecified. Deposition is handled inside `drain` (engine.rs:1841-1853,
   `targets.remove`), reached from `release_held`/submit paths; the design says held requests get
   `Err(Deposed)` but not where the gate is drained, nor that `held: BTreeSet<String>` and
   `qos_timer` must tolerate a removed volume (release_held iterating a name whose target is
   gone). Name the call site and add that case to `held_requests_fail_when_the_target_is_deposed`.

5. Low - S4 test's bound is weak and mislabelled. design.md Tests: "k-th no sooner than
   (k - 1) x 0.5 s" while an empty start gives k x 0.5 s; the assertion would pass with a free
   first request. Assert k x 0.5 s (minus tolerance) so S3 and S4 are not satisfied by an
   unrelated bug.

6. Low - Kernel re-arm is the only guard against blk-mq timeout and is checked on one kernel.
   Design admits it (Open). The qos-check log grep ("I/O error", "timed out") is in `journalctl
   -k`; also assert the device stayed up (`ublk` device id unchanged, fio error 0) - already
   partly there. Acceptable if Q1/open stays listed in the ADR; add the kernel versions to the
   ADR Confirmation.

## Nits
- decisions.md:29 and intent.md:44 still name "#73's step"; the coordinator said 83ae6dd drops
  the remaining #73 references; the worktree files still have them. Check the right commit.
- spec.md S4 "the floors bound the head's wait to 1 s" is true per head only; say "after its
  predecessor is admitted".
- design.md Compatibility: "pods do not start on a node with an older data plane" should read
  "attach fails" (see finding 2).
