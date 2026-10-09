//! Messages between a target and replica nodes (protocol spec §3–§7).

use crate::types::{Block, ChunkIdx, Epoch, Value, Version, VolumeId};

/// A request from a target to a replica node. Every request gets at most one reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToReplica {
    pub vol: VolumeId,
    pub epoch: Epoch,
    /// Session within the epoch. A `Hello` makes it current; other sessions are ignored.
    pub session: u64,
    /// Request id, echoed in the reply.
    pub req: u64,
    pub body: Request,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Opens a session and fences the volume at `epoch`. Reply: [`Response::Fenced`].
    Hello,
    /// Confirms the epoch for the lease. Reply: [`Response::Ok`].
    Heartbeat,
    /// A write of one block. `prune` lets the replica drop uncertain-set entries older than
    /// it: every write below it has completed. Reconciliation writes carry no `prune`.
    Write {
        chunk: ChunkIdx,
        block: Block,
        value: Value,
        version: Version,
        prune: Option<Version>,
    },
    /// Reply: [`Response::Value`].
    Read { chunk: ChunkIdx, block: Block },
    /// Starts a resync filter for the chunk: from now on, remember which blocks got live
    /// writes, and skip copies for them.
    BeginResync { chunk: ChunkIdx },
    /// A resync copy. Applied unless the block got a live write since `BeginResync`.
    Copy {
        chunk: ChunkIdx,
        block: Block,
        value: Value,
    },
}

/// A reply from a replica node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromReplica {
    pub vol: VolumeId,
    pub epoch: Epoch,
    pub session: u64,
    pub req: u64,
    pub body: Response,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The volume is fenced at the request's epoch. Lists, for every chunk of the volume this
    /// node holds, the blocks whose last write may not have completed.
    Fenced {
        uncertain: Vec<(ChunkIdx, Vec<Block>)>,
    },
    Ok,
    Value(Value),
    /// The volume is fenced at a higher epoch: the sender has been deposed.
    Stale {
        fenced: Epoch,
    },
}
