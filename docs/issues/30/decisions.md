# Issue #30: a volume created again starts above every earlier epoch — decisions

## Choices

**D1. The first epoch is the revision of the claim** (S1).
- **Why:** every attach is one header write, so an epoch never passes the revision of the
  header's last write, and the claim comes after the earlier volume's deletion. It needs no
  read of anything deleted, so it covers names deleted before this change and damaged headers.
- **Not chosen:**
  - **a high-water key per name** (`epochs/<id>`, kept after deletion): a new key that never
    goes away, and nothing to put in it for names deleted before the change;
  - **a global epoch counter:** one more hot key, written by every attach of every volume;
  - **the revision of the last write** (clearing `creating`): not known until it is written,
    and the claim's is already above every earlier epoch.

**D2. The first epoch is kept in the header** (`first_epoch`), not derived. The node needs it
(D3), and the epoch moves on at each attach.

**D3. The node drops chunks fenced below the first epoch** (S5).
- **Why:** a higher epoch alone turns deposition into adoption: every leftover replica is then
  fenced below the new epoch, and `Hello` keeps what it finds. The first epoch is the one
  number that separates a volume's own chunks (fenced above it) from an earlier
  volume's (below it).
- **Not chosen:**
  - **a volume UUID that includes the first epoch:** it changes the on-disk identity and every
    path that turns a name into a UUID (the deletion sweep, `delete VOLUME`, the disk tool). The
    old chunks would stay under their UUID, unseen, holding space;
  - **a `Hello` that refuses** instead of dropping: the new volume could then never use the
    node until someone deleted the leftovers by hand.

**D4. The first epoch rides in the capability token,** as a version 2 (S4).
- **Why:** it is signed by the metadata service, so a session cannot claim one it was not given.
  The target forwards the token unchanged: no new proto field, frame field or metadata client
  change.
- **Version 1 stays for volumes without a first epoch,** byte for byte, so existing volumes and
  old nodes see no change.
- **Not chosen:** a field in `Hello` (one trailing `u64`): unsigned, a frame format change, and
  the target would need it from a new proto field.

**D5. The local replica reads the first epoch from the attach token, decoded without its
signature** (S6), by `decode_unverified`, behind a crate feature only the node and the fuzz crate enable.
- **Why:** the local session has no `accept`, and the engine has no token key today. The token is
  the one place the first epoch is.
- **The check that the token names the volume, this node and its current epoch** is needed for that value to be
  this volume's: epochs of different volumes of a name never overlap (D1), so the epoch fixes
  the volume. It also refuses an old token at attach, which the issue asks for.
- **#15** turns this into a verified check (its S12).

**D6. The drop is `Store::delete_volume(uuid, below)`, decided from the store, in one commit per batch**
(S5). It is shared with #13, which adds the fence-only slot; `delete VOLUME` uses it too.
- **From the store, not `vols`:** an error mid-drop then leaves nothing in memory out of step,
  and the next `Hello` finds what is left.
- **One commit:** the drop runs on the engine thread; one commit per chunk would stall it.
- **The store's settle orders it** (`Store::guard` lands commits in flight and staged); no
  engine-level settle is added. Unlike `delete VOLUME`, the drop cannot refuse while a target
  of the name runs here: that target is of an earlier volume, and is deposed.

**D9. An attach with no token is refused** (S6). Without one the local replica has no first
epoch, and would adopt leftovers silently. Only a local-only volume of an `--insecure`
metadata service attached so; development loses that, and gains the fix.

**D7. The in-memory store's revision starts at the clock** (S2). One line, and a restarted
development service no longer starts below the volumes nodes still hold.

**D8. No new lab command.** The end-to-end check needs a tombstone removed by hand, which no
command should do. It is a recorded procedure (design, Tests), with the Rust engine test as the
repeatable version.

## Accepted risks

**R1. Old tokens on nodes the new volume has not reached** (S8). They open sessions there,
against the old volume's leftovers, and can create chunks of the name. Nothing of the new volume
is reachable through them: its first `Hello` there drops all of it. Expiry or revocation is a
separate change (#15 and later).

**R2. Leftover replicas on nodes the new volume never reaches** hold space until a sweep
deletes them (#13).

**R3. A store whose revision went back.** An etcd restore without `--bump-revision`, a copy of
the keys to a new cluster, an etcd wiped while nodes keep their data, a clock that went back
for the in-memory store. A volume created then
may start below an earlier one of its name, and the old fences and data are back for that name.
The documents say how to restore; the code cannot detect it.

**R4. Root on any node can sign tokens** (the token key is on every node), for any epoch and
first epoch. Such a token can drop a volume's chunks on a node, as it can already overwrite
them. Unchanged by this issue.

**R5. A downgrade of the metadata service** drops `first_epoch` from headers it rewrites at
attach. Those volumes then get version 1 tokens and keep their leftovers' data, as today; their
epochs still start above every earlier volume's. Worse, a volume an old instance creates
starts at epoch 0 and is deposed for good on nodes holding leftovers fenced near a revision:
no creation during a rolling metadata upgrade, and downgrades only with creation quiesced.

**R6. Mixed versions:** an old node refuses version 2 tokens, so a new volume's replica there is
unreachable until the node is upgraded. It fails rather than adopt.

**R7. Large epochs.** Epochs now start at a revision: millions in etcd, about 2^50 in the
in-memory store. Every field is a `u64`, and both stay below 2^53, which `jq` reads exactly.

**R8. The attach check is unsigned until #15** (D5). Root at the socket can pass a token it
made up, with any first epoch, for its own local replica. Root controls that replica already.

**R9. A zombie target's local I/O fails at the drop** (D6), with "unknown volume" rather than
`Stale`. It is of an earlier volume, deposed either way.
