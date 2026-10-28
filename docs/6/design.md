# Issue #6: a faster first attach after a restart — design

## Measurements

Lab, 2026-10-28, `vol1` (16 GiB, 64 chunks, replicas fl-a and fl-c) attached on fl-a, Linux
7.0, zero copy. Each run detached the volume everywhere, stopped the replica nodes named, then
timed three things on fl-a: the metadata service's attach, the control socket's `attach`, and a
direct read of block 0.

| Replica nodes stopped | Metadata attach | Control `attach` answered | First read, from the start |
|---|---|---|---|
| none | 0.045 s | 0.055 s | 0.115 s |
| fl-c | 0.046 s | 0.094 s | 5.820 s |
| none | 0.035 s | 0.051 s | 0.104 s |

With fl-c stopped, the first read waits for the takeover: `L × (1 + ρ)` = 5.5 s, before the
target may act without fl-c. The control socket answered within 0.1 s all the same. Starting the
device (`START_DEV`) did not wait for the kernel's partition scan, whose reads would have waited
as long. On this kernel the scan runs after the device has started.

## What an attach waits for

From kubelet, inside NodeStageVolume's 2-minute deadline, with the node plugin's lock (`n.mu`)
held throughout (`control/csi/node.go`):
1. `Status` on the control socket, which is instant.
2. `Meta.Attach`, the metadata service's attach. It gives each endpoint up to 10 s
   (`attemptTimeout`), so up to 30 s with three.
3. The control socket's `attach`, in the data plane (`attach_and_serve`):
   - **Target start:** the engine reads the volume and the nodes from the metadata service
     (`GetVolume`, `ListNodes`), then starts the target. Today it waits up to 60 s
     (`drive_until`). The metadata client has one queue for all of the node's calls, and tries
     each endpoint for 3 s. When the service is unreachable, its heartbeats can queue up
     ahead of the attach.
   - **Adding the device:** `add_device` retries for 10 s, plus one more try at most, while a
     previous device with the same id goes away.
   - **Starting the device:** libublk's `UblkDev::new` waits up to 3 s for `/dev/ublkcN`.
     `START_DEV` itself takes milliseconds on Linux 7.0.
4. For a filesystem volume: `blkid`, `mkfs` if the volume is empty, and `mount`.

The control socket's own limit (120 s) starts at step 3. When it failed, its answer came after
kubelet's deadline, and the node plugin reported only kubelet's `DeadlineExceeded`.

## Time limits (S2, S4)

**Data plane** (`dataplane/crates/flaminio-io/src/ublk.rs`, `control.rs`). Named constants in
`ublk.rs`, `pub(crate)`:
- `METADATA_LIMIT` = 30 s: the target start's wait, replacing `drive_until`'s literal 60 s.
- `ADD_DEVICE_LIMIT` = 10 s: `add_device`'s retries, replacing its literal.
- `START_LIMIT` = 10 s: an allowance for starting the device. It covers libublk's 3 s wait, and,
  on a kernel that scans partitions inside `START_DEV`, a first read that waits for a takeover
  (5.5 s). It is not enforced: the kernel call cannot be cancelled. It is part of the sum below.

`control.rs` derives `ATTACH_LIMIT` = `METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT` = 50 s,
replacing 120 s. `DELETE_LIMIT` is 50 s too (was 60 s). `detach` keeps its 30 s. `attach` takes
its limit as a parameter, so a unit test can pass a short one. The error names the limit it
waited, in seconds with at most one decimal: `error: no answer within 50 s`.

**Withdrawing a request.** `Control::Attach` gets `withdrawn: Arc<AtomicBool>`. The control
thread sets it when its limit passes. The main thread checks it when it takes the request, and
skips a withdrawn one without answering, since nobody is waiting. Without this, a request that
timed out while the node served another volume would run after the next detach, with a token
nobody uses any more. An attach already under way is not stopped.

**CSI node plugin** (`control/csi/dataplane.go`):
- `ControlSocket` gets a field `Timeout time.Duration`. Zero means `DefaultControlTimeout` =
  60 s, replacing `controlTimeout` = 150 s. A caller's earlier deadline still wins, as now. The
  effective limit is computed in one place, `ControlSocket.timeout()`, which the test checks.
  `main.go` sets no timeout, so it gets the default.
- The comment says what the 60 s is for: a backstop for a data plane that never answers, and
  the limit for callers without a deadline (the tombstone sweep). It must stay above the data
  plane's 50 s (`control.rs`).

**Follow-ups.** `docs/followups.md` §10, and its row in the table, say:
- the limits are done, with these numbers;
- the partition scan no longer holds up an attach on Linux 7.0;
- scanning stays on until a libublk release includes ublk-org/libublk-rs#72.

They drop the claim that the device is created with scanning off, which is not true today.

## Dropping a timed-out attach (S3)

The engine (`dataplane/crates/flaminio-node/src/engine.rs`) keeps an attach in progress in
`attaching[volume]` until both metadata answers are in. Today nothing removes it when the
frontend gives up. A late answer then starts a target that the frontend never serves.

- **Removing the entry.** `Engine::detach` removes `attaching[volume]` too, whether or not a
  target exists. `attach_and_serve` sends `Input::Detach` when the target start runs out of time.
  `AttachFailed` needs nothing, as the engine has already dropped the entry.
- **A generation for each attach.** `Attaching` gets `gen`, from a counter in the engine, and
  `MetaWait::Attach` carries it. `on_meta` compares `gen` with the entry that holds the volume
  now before it looks at the answer, and ignores an answer that does not match. Data from the
  dropped attach would otherwise fill in the next attach of the same volume with an old view,
  at an old epoch. A failure would end the next attach with the old attach's error.

## No automatic partition scan (S1), once libublk allows it

The change, ready for when a libublk release includes
[ublk-org/libublk-rs#72](https://github.com/ublk-org/libublk-rs/pull/72):
- **Feature check.** `add_device` ORs `sys::UBLK_F_NO_AUTO_PART_SCAN` into `ctrl_flags` if
  `UblkCtrl::get_features()` has it. Otherwise it logs once: "the kernel scans the partitions of
  ublk devices: a partition table inside a volume shows on the host".
- **Lab check.** A new `scripts/checks/partscan.sh` saves the volume's first 4 KiB (its logical
  block), and writes a prepared 512-byte MBR with one partition over its start with `dd`. It
  detaches and attaches the volume, waits for udev to settle, and checks that no
  `/dev/ublkbNp1` exists. It then writes the saved 4 KiB back.

It is not in this pull request. libublk 0.4.8 refuses the flag in `build()`, so a device with it
could not be created at all. Issue #6 stays open for it, linked to the upstream pull request.

## Tests

- **Go** (`control/csi/dataplane_test.go`): a Unix socket server, which reads the command, then
  never answers or answers after a delay. The socket goes in a directory from
  `os.MkdirTemp("", "fl")`, removed by `t.Cleanup`: macOS allows socket paths of only 104
  bytes, and a `t.TempDir()` path with the test's name can be longer.
  - `Timeout` 50 ms with no answer: `Attach` fails within a second, and its error says no
    answer came.
  - `Timeout` 10 s with a context deadline of 50 ms: it fails within a second.
  - `Timeout` zero with an answer after 200 ms: `Attach` returns `/dev/ublkb3`. A unit check
    asserts that `ControlSocket{}.timeout()` is `DefaultControlTimeout`, and above 50 s.
- **Rust:**
  - **Control socket:** `control.rs` gets a unit test for `attach` with a live receiver that
    never answers, and a limit of 100 ms. It checks the answer, and that the request it
    received is marked withdrawn.
  - **Limits:** a unit test asserts `ATTACH_LIMIT == METADATA_LIMIT + ADD_DEVICE_LIMIT +
    START_LIMIT`, and `ATTACH_LIMIT < 60 s`, with a comment pointing to `dataplane.go`.
- **Engine** (`flaminio-node/tests/engine.rs`), driven by hand: the metadata answers sit in the
  input channel until the test lets the engine read them.
  - Attach, then Detach, then deliver the answers: there is no `Attached`, and no target
    (`Input::Io` gets `NotAttached`). A new Attach then gives `Attached`.
  - Attach at epoch E, then Detach. Raise the model's epoch to E + 1, then Attach with an
    E + 1 token, then deliver all the answers: one `Attached`, at E + 1, and no `AttachFailed`.
    The old answers carry epoch E, so without the generation check they would start a target
    at E.
  - As the last, but the test replaces the dropped attach's two answers in the channel with
    failures. It finds them as the first `GetVolume` and `ListNodes` answers in the channel,
    not by id: the engine's own calls, such as `RegisterNode`, come first. The new attach still
    gives one `Attached`, and no `AttachFailed`.
- **Lab:** the timed attaches above, after the change, and `scripts/checks/reconcile.sh`.

## Decisions

- **30 s for the target start, not 60 s.** It keeps the budget inside kubelet's 2 minutes: 30 s
  for `Meta.Attach`, plus 50 s, leaves 40 s for `mkfs` and `mount`. A metadata service that has
  not answered in 30 s is in an outage. With S3, the retry kubelet makes then works.
- **Derive `ATTACH_LIMIT` from its parts.** It should not be a third literal that has to be kept
  in step by hand.
- **The CSI limit is a field with a default, not a flag.** No deployment needs to change it. The
  field exists for tests.
- **A generation, not only removal.** Removing the entry alone would let a late answer fill in,
  or fail, the next attach of the same volume.
- **Withdraw a timed-out request, rather than let it run later.** It would attach the volume
  with a stale token, for a caller that has given up.
- **S1 waits for upstream.** We do not carry a patched copy of libublk (about 11 000 lines), and
  do not send our own `ADD_DEV`. The user chose an upstream pull request
  (ublk-org/libublk-rs#72).

## Risks

- **A metadata service slower than 30 s to answer fails the attach.** That is an outage, not a
  slow answer, and kubelet retries.
- **A kernel that scans partitions inside `START_DEV`** would hold the start for as long as the
  scan's first reads wait: up to a takeover, 5.5 s. `START_LIMIT` allows for it, and S1 removes
  the scan.
- **An attach already under way when the socket gives up still finishes** (S2). A retry finds
  its device through `status`.
