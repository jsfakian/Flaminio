# Issue #30: a volume created again starts above every earlier epoch — specification

## The metadata service

**S1. A new volume starts at its first epoch.** A volume's first epoch is the store's revision
of the transaction that claims its name. Creation records it in the header, and sets the
volume's epoch to it; the first attach then takes the first epoch + 1.
- **Every earlier volume of the name ended below it.** Each attach is one header write that
  adds one to the epoch. So a volume's epoch never passes the revision of its last header write.
  An earlier volume's last header write came before its deletion, and the deletion before the
  claim.
- **So everything an earlier volume left is below the new volume's epochs:** the fences its
  targets set, the epochs on its tokens, the epochs on its chunk replicas.
- **No new key.** The first epoch is a field of the header.

**S2. The store's revision never goes back.** S1 holds only while it does.
- **etcd:** the revision only grows; compaction does not change it. A restore from a snapshot
  must raise it above the revision of the cluster it replaces (`--bump-revision`). Moving the
  metadata to a new cluster any other way (copying keys), or wiping etcd while nodes keep their
  data, is not supported.
- **The in-memory store** (development and tests) starts its revision at the clock, in
  microseconds, so that a restarted service starts above the one before it, whose volumes
  nodes may still hold.

**S3. Existing volumes are unchanged.**
- **A volume created before this change** has no first epoch (read as 0), keeps its epoch, and
  behaves as today: its tokens and its replicas' handling do not change.
- **A name deleted before this change** and created after it starts above every epoch the
  earlier volume reached: S1 needs only the revisions, not the deleted header.
- **A damaged header** (one that does not parse) is deleted as today. A volume created after it
  starts above it, for the same reason.

## Capability tokens

**S4. A token carries the volume's first epoch.**
- **A volume with a first epoch** gets tokens of a new version, which add it to the signed
  claims.
- **A volume without one** gets tokens of today's version, byte for byte.
- **A token whose first epoch is not below its epoch** is invalid: no attach issues one.
- Tokens still never expire (S8).

## The node

**S5. A replica drops an earlier volume's chunks.** At a `Hello` whose token names a first
epoch, a node whose record of the volume is fenced below it holds only what earlier volumes of
the name left.
- **It deletes every chunk of the volume fenced below the first epoch,** placed here or not,
  as the store records them, in one commit per batch (one in practice); only then does it forget the volume's fence
  and sessions. A failed deletion fails the `Hello` and changes nothing else. Commits in flight
  land first. Then the `Hello` goes on as today: it creates
  the placed chunks, empty, fences them at its epoch, and replies.
- **The deletion is durable before the reply.** A crash or error during it leaves only chunks
  still fenced below the first epoch, which the next `Hello` deletes.
- **A volume's own chunks are never deleted.** Every chunk and fence a volume's targets make is
  at one of its epochs, above its first epoch.
- **A different size or chunk size** in the earlier volume no longer refuses the `Hello`.
- **The old volume's sessions** on this node end: their requests are refused as for an unknown
  volume.
- **Without a first epoch** (S3), the `Hello` is handled as today.

**S6. The target's own replica.** The node takes the first epoch for its local replica from the
attach token too.
- **The attach refuses a token** that does not decode, or names another volume, another node,
  or another epoch than the volume's current one. The token's signature is not checked here
  (#15 adds that).
- **An attach with no token is refused.** Remote replicas already refuse sessions without one;
  only a local-only volume of a metadata service without tokens (development) attached so.

## What this closes, and what is left

**S7. Closed.** For a volume deleted and created again with the same name:
- **no deposition:** no earlier fence is at or above its epochs;
- **no adoption:** on every node it opens a session with, the earlier volume's chunks are
  deleted first. Every session starts with a `Hello`, a move's destination included;
- **old tokens are refused** on every node the new volume has opened a session with
  (`Stale`), and by the new volume's attach (S6). Their epochs are below the new volume's.

**S8. Left.**
- **Old tokens on nodes the new volume has not reached.** Such a token still opens a session
  there, on the old volume's leftovers, and may create chunks of the name. The new volume
  deletes them at its first `Hello` there. Tokens still never expire.
- **Replicas a node kept without a record**, on nodes the new volume never reaches: they hold
  space until a sweep deletes them (#13).
- **Tombstones still block a name's re-creation** until their nodes confirm (#8, #14).
- **The token key is on every node:** root on any node can sign a token for any epoch.
- **A store whose revision went back** (S2 not followed) can start a volume below an earlier
  one: the deposition and the adoption come back for that name.

**S9. Mixed versions.**
- **An old node** refuses the new tokens. A new volume cannot open a session on it until it is
  upgraded; it fails, rather than adopt. So nodes are upgraded before the metadata service.
- **An old metadata service** (a downgrade) drops the first epoch from headers it rewrites at
  attach. Those volumes then get today's tokens: their replicas are handled as today, but their
  epochs stay above the earlier volumes'.
- **A volume created by an old metadata instance** (a rolling upgrade, or after a downgrade)
  starts at epoch 0 again, and is deposed for good on nodes holding leftovers fenced near a
  revision. So no volume is created during a rolling upgrade of the metadata service, and a
  downgrade needs creation quiesced.

## The contract

**S10. Go and Rust agree.**
- **The reference model** (`flaminio-repl`) creates a volume at a given first epoch, as the Go
  domain does. The vectors' `create` carries it, and are regenerated.
- **The token's new version** is encoded alike by Go's issuer and Rust's verifier, with shared
  vectors.

## Checks

- A volume deleted after 7 attaches and created again starts above 7, and so does one whose
  name was deleted before this change, or whose header was damaged.
- On the lab: a volume deleted, its tombstone removed by hand (standing in for a node it does
  not name), and created again with the same name and another size, attaches on the node that
  holds the old replicas. It reads zeros where the old one held data, takes writes without
  being deposed, and the old attach token is refused.
