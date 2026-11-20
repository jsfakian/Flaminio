# Issue49 history v1 binary contract (Accepted with the plan, 2026-11-13)

This appendix fixes the [design](design.md)'s record seam. All integers are unsigned little
endian except signed result; IDs/times are never shell fragments. Reject unknown versions,
nonzero reserved fields, overflowing sizes, duplicate IDs and invalid UTF-8 names/paths.

## Segment header

Prefix:8 bytes `FLHST001`, u32 payload length≤4096, payload, u32 CRC32C over prefix+payload.
Payload in order: run_nonce[32], segment_id:u64 (1..1024, the segment's place in the run),
boot_id[16], node_len:u16/node UTF-8 bytes≤128, block_count:u16≤16,
block_indices[block_count]:u64. Block indices unique/in-range and identical across all run
segments. run_nonce and segment_id must match the segment's manifest descriptor; boot_id is the
boot of every time in the file (`end` checks it again); node is the VM's name, context for the
report only. No volume epoch is recorded: no rule reads one. No device path, key/token or
wall-clock epoch authorizes ordering.

## Records

Frame: u32 body length exactly128, body[128], u32 CRC32C over length+body (136 bytes total).
Body offsets:

| Offset | Field |
|---|---|
|0|type:u8 (1 Invoke,2 Return,3 Checkpoint,4 End,5 Issued)|
|1|kind:u8 (1 Read,2 Write;0 only End)|
|2|flags:u16 (bit0 foreign read content; others0)|
|4|result:i32 (0 success, positive errno;0 for Invoke/Issued/Checkpoint/End)|
|8|sequence:u64, starts1, no gaps|
|16|op_id:u64, nonzero except End; high 32 bits = the header's segment_id, low 32 bits a counter from 1|
|24|block_index:u64; UINT64_MAX only End|
|32|local_boottime_ns:u64, same boot as header|
|40|write_value_id:u64; observed value for successful nonforeign Read; for End, the kill's first sample (0: no kill); zero for Issued|
|48|value_run_nonce[32]; observed nonce for Read, current run for Write; zero for Issued|
|80|value_block_index:u64; for End, the kill's second sample (0: no kill); zero for Issued|
|88|foreign_sha256[32]; zero unless foreign Read|
|120|reserved:u64=0|

Invoke precedes device issue and must be durable first. A Write's value ID, like every op_id,
carries its segment in the high 32 bits: an Invoke whose op_id or Write value ID names another
segment than the header is malformed (undecided). A Read's observed value ID may name its own
or an earlier segment. Return references exactly one Invoke; Write result reports the invoked value
ID. Error Read has no value. A foreign read's checksum summarizes the raw4096-byte content; no
checker accepts it as a valid value. Recorder itself validates complete4096-byte encoding before
classifying a read. Checkpoint references an acknowledged nonforeign Read after all burst
operations on that block answered. It grants no extra completion and is only a potential
decomposition cut.

**Issued** is a frame like the others (136 bytes, its CRC32C as every frame's), one per
operation. The client's I/O is blocking (`pread(2)`/`pwrite(2)`, one goroutine per operation), so
Issued marks the last point before the syscall, not the issue itself: after the fdatasync of the
batch holding its Invoke, the operation's goroutine samples the clock, appends its Issued frame
through the recorder and then enters the syscall directly. kind, op_id and block_index are those
of its Invoke, local_boottime_ns that sample, sequence the file's next; every other field is zero
(flags, result, offsets 40, 48 and 80, foreign_sha256, reserved). An Issued operation may not yet
have reached the device for the gap between that append and the syscall's entry. It needs no fsync
of its own: a client SIGKILL does not lose a completed `write(2)`, and the next batch's fdatasync
covers it. An Issued with no earlier Invoke of its op_id, or whose kind or block differs from
that Invoke's, or with a nonzero field it must leave zero, a second Issued for one op_id, an
Issued after its operation's Return, and a Return with no earlier Issued are malformed
(undecided). An Invoke with no Issued may have been issued or not (the client was killed before
the frame, or the frame was lost in a torn tail): it is unanswered, and never counted as issued.

**Opening reads.** In a segment with segment_id > 1, the first Invoke on each block is a Read,
and that Read's successful Return comes, in sequence order, before the segment's first Write
Invoke on the block. Otherwise the history is undecided (S2, S7).

**End** is written by `flaminio-history end` once the client is gone (it holds the file's
`flock`), never beside a live writer. Its time is on the header's boot; its offsets 40 and 80
hold the kill's two samples, `kill-at`'s `CLOCK_BOOTTIME` ns on the same boot just before and just
after its SIGKILL (`pidfd_send_signal`), both 0 when the segment ended without a kill; one 0
without the other, or a second sample earlier than the first, is malformed. End is the last
frame: a frame after it is malformed. `end` truncates a torn tail (fewer than 136 bytes after the
last whole frame whose CRC holds) before appending End; a torn frame was never an issued
invocation, or was a lost Issued or Return, which leaves its operation unanswered. More than that
after the last good frame is refused, and the segment has no End. Missing End or unknown
middle/trailing evidence yields Undecided.

## Run manifest

Binary `FLRUN001`, written by `flaminio-history seal` on the workstation, run_nonce[32],
segment_count:u32≤1024, descriptors in causal segment order: segment_id:u64, relative_path_len:u16,
relative UTF-8 path≤512 bytes, sealed_length:u64 (the file's bytes through End). segment_id is
1..segment_count in that order, with no gap: the harness checks the count against the run's rounds
(ROUNDS + 1, plus 1 with the move back). Finish with CRC32C over all preceding bytes. Paths may
contain only bounded safe components; reject absolute paths, dot/dotdot, symlink traversal, control
bytes and duplicate descriptors. Only seal a segment after collecting its authoritative End;
preserve unsealed artifacts on failure and report Undecided. A file whose length differs from
sealed_length is undecided, and so is a manifest whose sealed_length values total more than
128 MiB (134,217,728 bytes), the run's cap on its segment files. Lengths and CRCs protect against
accidental evidence changes, not malicious forgery. Fault log and human JSON report are separate
contextual artifacts and cannot invent cross-node precedence. Verdict generation checks every
descriptor and every final block read.

## Heal ledger

Not `FLRUN001` and not checked evidence: `heal-ledger` in the run directory on the workstation,
its only copy, mode 0600. The harness appends a line and syncs the file before each action, and
after each confirmation. ASCII lines, fields separated by one space, each line ≤200 bytes, at
most 10,000 lines:

- line 1: `flaminio-heal-ledger 1 <run_nonce as 64 lowercase hex digits>`;
- then `intend|confirmed|healed <kind> <vm> [<unit>]`.

Lines carry no round or sequence: an `intend` is healed by the next `healed` line after it of
the same kind, VM and unit.

| kind | vm | unit | healed by |
|---|---|---|---|
| `restart`, `kill` | a data VM | none | `cmd_node up`, unit active and node ready |
| `partition` | a data VM | none | `partition.sh off` (its fixed tag `flaminio-partition`), probe |
| `delay` | a data VM | none | `chaos.sh slow off` (only the netem root qdisc of handle `49:` on wg0) |
| `witness` | `$VM_W` | none | etcd member and metadata service up, membership and RPC healthy |
| `client`, `fio` | a data VM | `flaminio-history-<h>-<segment>`, `flaminio-fio-<h>-<round>` | the unit stopped and not active (`inactive` or `failed`) |

A data VM is one of `vms_of all`, matched whole; `<h>` is the run nonce's first 16 hex digits,
and `<segment>`, `<round>` are decimal numbers ≤1024. `chaos-heal` refuses the whole ledger,
before any remote command, when any line fails: another header or nonce, a kind, VM or unit
outside this table, an unknown verb, a field too many, a line too long. Nothing else is
accepted, and no rule tag, qdisc handle or unit comes from anywhere but this table.
