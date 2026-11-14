# Issue #86: Bound conservative Fenced uncertainty — specification

[Intent](intent.md), [design](design.md), [proposed ADR](adr-proposal.md),
[decisions](decisions.md). Approved 2026-11-13; not implemented.

## Terms

- **Precise record:** an accepted write extent and its Version=(epoch,seq), before proof of
  completion prunes it. Records include failed/unanswered attempts and duplicate writes.
- **Budget B:** 65,536; N is held chunks, R retained precise records. One report slot is
  reserved per held chunk: N+R≤B. All held chunks, not only the Hello placement, count.
- **Coarse cover:** a whole-chunk uncertainty marker plus the maximum Version of the records it
  replaced and of every later write to that chunk. It always represents at least one write.
- **Restart cover:** the existing restarted_at epoch marker, independent of coarse versions.
- **Prune below p:** remove records strictly less than p; p itself is not completed proof.

## The invariant

A report conservatively covers every block any retained/uncompleted write could have affected,
including after coarsening, retry, restart and partial reconciliation. No representation or
framing limit drops that uncertainty, a version barrier or restart evidence. All-ISR durable
acknowledgement, epoch/session fencing and target readiness remain the existing rules.

## Behaviour

**S1. Bounded retention.** Per volume, admit at most B held chunks and N+R≤B precise state.
A retained write or a new chunk that would exceed that demand first coarsens one chunk (S2).
Repeated/overlapping unpruned writes cannot grow state beyond the bound. A Hello or create that
would make N>B is refused before any create or fence, and after an earlier incarnation's chunks
and core state are discarded, as today: the refusal changes nothing of the volume the Hello
names. An unsupported held set found at startup is quarantined (S4), which deletes nothing.
Neither constructs an oversized report.

**S2. Conservative coarsening.** At N+R=B, a write that must retain a record coarsens only the
chunk it writes: that chunk's records are replaced by one coarse cover. A new chunk at N+R=B
coarsens one chunk that holds records, then starts known-empty. No other chunk changes. Later
writes to a coarse chunk update only its marker. A chunk whose cover clears (S3) holds precise
records again: a cleared cover represents no write. Coarsening may overreport
blocks, never truncate, treat gaps as clean or create a false write acknowledgement.

**S3. Pruning.** A coarse version cover can clear only when every represented Version is
strictly below the current-session prune/Settle bound. A Write or Settle whose version, prune or
bound carries an epoch other than its session's is dropped: a forged epoch then cannot clear
restart cover set in the fenced epoch, and a cover's maximum never exceeds the fenced epoch. A
bound of the session's own epoch stays its holder's claim, as today; this rule does not check
it. Partial/same-epoch prune
cannot erase a newer represented write. Restart cover still clears only with a strictly newer
epoch, as today. Repair writes with prune=None remain covered; later Settle may clear completed
repair cover. Coarse and restart reasons are tested/cleared independently.

**S4. Lifecycle.** Preserve coverage through same/new-epoch Hello, lost responses and retained
partial writes. Restart discards volatile precise records and coarse covers only by installing
restart cover on every held chunk. Removal updates budget accounting and ends only that chunk;
adding or recovering one cannot silently overwrite an existing chunk's uncertainty or exceed B.
**Quarantine:** a volume found at startup with more than B chunks, or a chunk index outside the
supported range, gets no in-memory state. Every Hello for it is refused before the
earlier-incarnation delete and before any store or core change, whatever its first epoch; no
session opens. Its chunks and durable fences stay as found; the reason is logged at startup and
returned in each refusal. Other volumes are served. It ends only by the existing replica delete
of a volume removed from the metadata service, or by operator cleanup and a restart: no
automatic erase.

**S5. Representable reports.** All producer Fenced rows are unique, ordered chunk identities;
extents are nonempty, checked, ordered and nonoverlapping. Row count N≤B, aggregate extent
count T≤B, and encoded body 13+8N+8T≤MAX_BODY=1,048,704 before allocation/send. For N=T=B,
body 1,048,589 fits; never silently raise a limit or paginate without another protocol design.

**S6. Checked framing.** Fenced goes through the checked encoder shared with #80 (one signature
and error; whichever issue lands first adds it), including
synthetically invalid frames. Count/body overflow, duplicate/unordered rows or invalid extents
returns a normal error before bytes are sent. Decoder enforces equivalent count/body/shape
bounds before reserving/iterating. No runtime Fenced send relies on encode assertions.

**S7. Consumer validation.** Validate a complete Fenced reply before granting session-open,
lease, first-fence or chunk-ready facts; a reply to Hello that is neither Stale nor a valid
Fenced grants none of them, and Stale still deposes. Reject chunk IDs outside the volume and
extents beyond
its chunk geometry, duplicates and invalid count/body/shape; never interpret an invalid row as
empty uncertainty. Then process/report union with bounded work rather than repeated searches.
Every required chunk advertised in that Hello must have a row; the Hello message carries that
set, fixed at send time, not taken from later placement. Chunks the Hello creates have explicit
empty rows. An explicit empty row carries exactly the trust a missing row carries today:
create-on-Hello is unchanged, so a node that lost its disk still creates blank chunks and
reports them empty. Telling a fresh allocation from a lost one is #13/#61's work and is not
claimed here. Valid held-but-unplaced rows may be ignored after validation; do not require an
exact assigned-only set. A malicious peer lying with an empty row is not detected by this
contract.

**S8. Bounded work.** Document/count limits for retention, coarsening, report materialization,
decode and target consumption. Writes to a coarse chunk use constant representation work;
coarsening frees each record once. No per-write global history or unbounded union is hidden
behind a small encoded reply. Resource refusal never rolls back a durable fence or reports
success.

**S9. Compatibility/docs.** Keep wire fields/tags, token, disk layout and metadata RPCs unchanged.
Full uncertainty can increase reconciliation traffic/latency; no availability shortcut is taken.
Coarsening is reached by ordinary workloads, not only attacks. A write prunes only its own
chunk, so idle chunks keep their last in-flight window (R≈chunks touched×queue depth: a
sequential fill of a 128 GiB volume at the defaults reaches B−N); and one stalled write holds
the volume's prune bound while B−N other writes complete. Each chunk written while N+R=B is
reconciled whole at the next takeover unless a later prune or Settle clears it first; other
chunks stay precise. Document these triggers and that cost.
#80 remains a coordinated prerequisite for supported geometry, but runtime defense is effective
when legacy/unsupported callers bypass admission. Document remediation and future #51 limits.

## Tests

- **S1–S4:** unit/reference properties at B−1/B/B+1 demand, duplicate/fragmented writes, mixed
  versions, partial/full prune, prune=None repair then Settle, restart/new epochs, add/remove/
  recover boundaries; exact shadow records are always covered, and memory accounting is bounded.
  Only the written chunk coarsens; a cleared cover is precise again after one more write; a
  sequential fill at the default depth and geometry coarsens a few chunks, not the volume;
  a version, prune or Settle bound of a lower or higher epoch is dropped, also against a
  restart-covered chunk. Node level: a quarantined volume beside a served one, refused Hello
  (also with a higher first epoch), unchanged inventory, delete still works; a Hello or a
  moved-chunk create beyond B is refused before the store create, each refusal asserted by its
  reason; a Hello whose set fits once an earlier incarnation is discarded succeeds.
- **S5, S6:** retained safe probes through real ReplicaCore and encode/decode; 1 and 2 legal 1 GiB
  chunks no longer reject/panic, max held set N=T=B roundtrip, one-over count/body, malformed
  rows/extents/overflow, zero socket bytes on refused encode and fuzz regression inputs.
- **S7, S8:** target tests reject invalid reports and a non-Fenced reply to Hello before
  fencing/lease side effects; include explicit empty rows for chunks the Hello created,
  valid retained extra rows, local/remote paths and max-set linear work instrumentation.
- **Invariant:** simulator uses small configurable budgets to exercise normal faults, pending
  writes, restart and repair. Independent coverage/state checker plus linearizability catches
  drop_on_coarsen, coarse_prune_early, bypass_uncertainty_budget, accept_invalid_fenced and,
  with forged-epoch requests injected, accept_foreign_epoch mutations. Runs with forged epochs
  and no bug stay clean, liveness included.
- **S9:** existing protocol mutations and interoperability bounds remain, no disk/wire upgrade
  format change or lab result is invented; producer/consumer rollout is documented.

## Open questions

- Per-chunk coarsening leaves the idle residue (S9) in place; pruning idle chunks at the
  source would be a new replica rule with its own proof and is not proposed here.
  Settled at approval, 2026-11-13: the idle residue stays; no idle-chunk pruning is added (see decisions.md, Settled at approval).
- A quarantined earlier incarnation blocks its name on that node until it is deleted (S4).
  Settled at approval, 2026-11-13: accepted, with no automatic erase (see decisions.md, Settled at approval).
- A prune or Settle bound of the session's own epoch is not checked by the replica (S3).
  Settled at approval, 2026-11-13: it stays the holder's claim, as today; the residual risk is accepted (see decisions.md, Settled at approval).
- B is per volume, not a certified aggregate memory/CPU limit across all volumes (#75).
  Settled at approval, 2026-11-13: B stays per volume; an aggregate limit is #75's work (see decisions.md, Settled at approval).
