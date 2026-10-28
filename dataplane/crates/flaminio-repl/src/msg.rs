//! Messages between a target and replica nodes (protocol spec §3–§7).
//!
//! `P` is the payload: the data of an extent. The simulator uses one value per block; the data
//! plane uses handles to buffers.

use crate::extent::Extent;
use crate::types::{ChunkIdx, Epoch, Version, VolumeId};

/// A request from a target to a replica node. Every request gets at most one reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToReplica<P> {
    pub vol: VolumeId,
    pub epoch: Epoch,
    /// Session within the epoch. A `Hello` makes it current; other sessions are ignored.
    pub session: u64,
    /// Request id, echoed in the reply.
    pub req: u64,
    pub body: Request<P>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request<P> {
    /// Opens a session and fences the volume at `epoch`. Reply: [`Response::Fenced`].
    Hello,
    /// Confirms the epoch for the lease. Reply: [`Response::Ok`].
    Heartbeat,
    /// A write of one extent. `prune` lets the replica drop uncertain-set entries older than
    /// it: every write below it has completed. Reconciliation writes carry no `prune`.
    Write {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
        version: Version,
        prune: Option<Version>,
    },
    /// Reply: [`Response::Data`].
    Read { chunk: ChunkIdx, ext: Extent },
    /// Starts a resync filter for the chunk: from now on, remember which blocks got live
    /// writes, and skip copies for them.
    BeginResync { chunk: ChunkIdx },
    /// Every uncertain-set entry of the chunk older than `below` is settled, and so is any
    /// uncertainty left from a restart: the ISR agrees on those blocks (sent once the chunk is
    /// reconciled, and to a replica that joins the ISR). Reply: [`Response::Ok`].
    Settle { chunk: ChunkIdx, below: Version },
    /// A resync copy. Applied to the blocks that got no live write since `BeginResync`.
    Copy {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
    },
    /// Per-block digests for an extent (follow-ups §2). Reply: [`Response::Digests`].
    /// Unwritten blocks are `None` on the wire; the simulator uses each block's value.
    Digest { chunk: ChunkIdx, ext: Extent },
    /// Sub-extents of `ext` that hold data on this replica (follow-ups §3). Reply:
    /// [`Response::Mapped`]. Unwritten ranges are omitted so the target can skip copies and
    /// discard stale data on the destination.
    Mapped { chunk: ChunkIdx, ext: Extent },
    /// Drops data in `ext` (advisory discard). Reply: [`Response::Ok`]. Used when the source
    /// of a resync or repair has nothing mapped there.
    Discard { chunk: ChunkIdx, ext: Extent },
}

/// A reply from a replica node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromReplica<P> {
    pub vol: VolumeId,
    pub epoch: Epoch,
    pub session: u64,
    pub req: u64,
    pub body: Response<P>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response<P> {
    /// The volume is fenced at the request's epoch. Lists, for every chunk of the volume this
    /// node holds, the extents whose last write may not have completed.
    Fenced {
        uncertain: Vec<(ChunkIdx, Vec<Extent>)>,
    },
    Ok,
    Data(P),
    /// One CRC-32C per block of the requested extent, of what a read returns: an unwritten
    /// block reads as zeros, so replicas compare by content whatever their layout. The
    /// simulator uses each block's value. `None` means no digest, which counts as differing.
    Digests(Vec<Option<u32>>),
    /// Extents inside the request that hold data. Disjoint, ordered, clipped to the request.
    Mapped(Vec<Extent>),
    /// The volume is fenced at a higher epoch: the sender has been deposed.
    Stale {
        fenced: Epoch,
    },
}
