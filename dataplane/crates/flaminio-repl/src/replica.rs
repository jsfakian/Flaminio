//! A node holding chunk replicas (protocol spec §3, §6, §7).
//!
//! [`ReplicaCore`] holds the protocol rules without storage: fencing, sessions, uncertain sets
//! and resync filters. For each request it returns an [`Action`] for its caller to carry out:
//! the data plane against the replica store, the simulator against the in-memory
//! [`Replica`].
//!
//! Durable state is the data and the fenced epochs. Sessions, filters and uncertain sets are
//! volatile. After a restart a replica cannot tell which of its recent writes completed, so it
//! reports every block of its chunks as uncertain. That lasts until a target of a newer epoch
//! than any it had seen prunes the set: such a target has reconciled the chunk.

use std::collections::BTreeMap;

use crate::extent::{Extent, ExtentSet};
use crate::msg::{FromReplica, Request, Response, ToReplica};
use crate::types::{Block, ChunkIdx, Epoch, Value, Version, VolumeId};

/// What the caller does with a request, in the order the requests arrived on their session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<P> {
    /// Send this reply.
    Reply(Response<P>),
    /// Make the volume's fenced epoch durable, then send the reply.
    Fence { epoch: Epoch, reply: Response<P> },
    /// Ignore the request: it belongs to a session that is no longer current.
    Drop,
    /// Write the extent, then reply `Ok`.
    Write {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
    },
    /// Read the extent, then reply `Data`.
    Read { chunk: ChunkIdx, ext: Extent },
    /// A `BeginResync` for a chunk this node does not hold: a replica moving here. Create the
    /// chunk, empty ([`ReplicaCore::add_chunk`]), then handle the request again.
    CreateChunk { chunk: ChunkIdx },
    /// Write only `parts` of the extent (the blocks without live writes since
    /// `BeginResync`), taking each part's data from its offset in `data`, then reply `Ok`.
    Copy {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
        parts: Vec<Extent>,
    },
    /// Return digests for the extent (store metadata or simulator block values).
    Digest { chunk: ChunkIdx, ext: Extent },
    /// Return the sub-extents of `ext` that hold data.
    Mapped { chunk: ChunkIdx, ext: Extent },
    /// Discard `parts` of the extent (blocks without live writes since `BeginResync`), then
    /// reply `Ok`.
    Discard {
        chunk: ChunkIdx,
        ext: Extent,
        parts: Vec<Extent>,
    },
}

#[derive(Debug, Clone)]
struct CoreChunk {
    blocks: Block,
    /// Extents written by writes that may not have completed, with their versions.
    uncertain: Vec<(Extent, Version)>,
    /// After a restart: every block may hold an unfinished write of an epoch up to this one.
    restarted_at: Option<Epoch>,
}

impl CoreChunk {
    fn report(&self) -> Vec<Extent> {
        if self.restarted_at.is_some() {
            return vec![Extent::new(0, self.blocks)];
        }
        let mut s = ExtentSet::new();
        for (x, _) in &self.uncertain {
            s.insert(*x);
        }
        s.iter().collect()
    }
}

#[derive(Debug, Clone, Default)]
struct CoreVol {
    fenced: Epoch,
    chunks: BTreeMap<ChunkIdx, CoreChunk>,
    session: Option<(Epoch, u64)>,
    filters: BTreeMap<ChunkIdx, ExtentSet>,
}

/// Deliberate protocol violations, for checking that the simulator catches them.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Bugs {
    /// Accept requests from any session of the current epoch.
    pub any_session: bool,
    /// Apply resync copies even over newer live writes.
    pub no_resync_filter: bool,
}

/// The protocol rules of a replica node.
#[derive(Debug, Clone, Default)]
pub struct ReplicaCore {
    vols: BTreeMap<VolumeId, CoreVol>,
    #[cfg(test)]
    pub bugs: Bugs,
}

impl ReplicaCore {
    /// Adds a new, empty chunk replica.
    pub fn add_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx, blocks: Block) {
        self.vols.entry(vol).or_default().chunks.insert(
            chunk,
            CoreChunk {
                blocks,
                uncertain: Vec::new(),
                restarted_at: None,
            },
        );
    }

    /// Adds a chunk replica found on disk at startup, with its volume's fenced epoch.
    pub fn recover_chunk(&mut self, vol: VolumeId, fenced: Epoch, chunk: ChunkIdx, blocks: Block) {
        let v = self.vols.entry(vol).or_default();
        v.fenced = v.fenced.max(fenced);
        v.chunks.insert(
            chunk,
            CoreChunk {
                blocks,
                uncertain: Vec::new(),
                restarted_at: Some(fenced),
            },
        );
    }

    /// Loses the volatile state, as after a process or node crash.
    pub fn restart(&mut self) {
        for v in self.vols.values_mut() {
            v.session = None;
            v.filters.clear();
            for c in v.chunks.values_mut() {
                c.uncertain.clear();
                c.restarted_at = Some(v.fenced);
            }
        }
    }

    /// Forgets a chunk replica that moved away. The volume's fenced epoch stays.
    pub fn remove_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx) {
        if let Some(v) = self.vols.get_mut(&vol) {
            v.chunks.remove(&chunk);
            v.filters.remove(&chunk);
        }
    }

    /// Forgets a deleted volume entirely, its fenced epoch included: for a volume that no
    /// longer exists, so that a new volume of the same name starts afresh. Requests of its
    /// sessions are not valid after this.
    pub fn remove_volume(&mut self, vol: VolumeId) {
        self.vols.remove(&vol);
    }

    pub fn fenced(&self, vol: VolumeId) -> Epoch {
        self.vols.get(&vol).map_or(0, |v| v.fenced)
    }

    /// Decides what to do with one request. Requests must be handled in the order they arrive
    /// on their connection, and their actions carried out in that order.
    pub fn handle<P: Clone>(&mut self, m: &ToReplica<P>) -> Action<P> {
        let v = self.vols.entry(m.vol).or_default();
        if m.epoch < v.fenced {
            return Action::Reply(Response::Stale { fenced: v.fenced });
        }
        if let Request::Hello = m.body {
            v.fenced = m.epoch;
            if v.session.is_none_or(|s| s < (m.epoch, m.session)) {
                v.session = Some((m.epoch, m.session));
                v.filters.clear();
            }
            let uncertain = v.chunks.iter().map(|(&c, s)| (c, s.report())).collect();
            return Action::Fence {
                epoch: m.epoch,
                reply: Response::Fenced { uncertain },
            };
        }
        let current = v.session == Some((m.epoch, m.session));
        #[cfg(test)]
        let current = current || (self.bugs.any_session && m.epoch == v.fenced);
        if !current {
            return Action::Drop;
        }
        let in_range = |chunk: &ChunkIdx, ext: &Extent| {
            v.chunks
                .get(chunk)
                .is_some_and(|c| ext.len > 0 && ext.end() <= c.blocks)
        };
        match &m.body {
            Request::Hello => unreachable!(),
            Request::Heartbeat => Action::Reply(Response::Ok),
            Request::Read { chunk, ext } if in_range(chunk, ext) => Action::Read {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Digest { chunk, ext } if in_range(chunk, ext) => Action::Digest {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Mapped { chunk, ext } if in_range(chunk, ext) => Action::Mapped {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Discard { chunk, ext } if in_range(chunk, ext) => {
                // Same filter as Copy: live writes since BeginResync win over a discard of a
                // hole observed before those writes.
                let parts = match v.filters.get(chunk) {
                    Some(f) => f.missing_from(ext),
                    None => vec![*ext],
                };
                Action::Discard {
                    chunk: *chunk,
                    ext: *ext,
                    parts,
                }
            }
            Request::Write {
                chunk,
                ext,
                data,
                version,
                prune,
            } if in_range(chunk, ext) => {
                let c = v.chunks.get_mut(chunk).expect("in range");
                c.uncertain.push((*ext, *version));
                if let Some(p) = prune {
                    c.uncertain.retain(|(_, w)| w >= p);
                    if c.restarted_at.is_some_and(|e| e < p.epoch) {
                        c.restarted_at = None;
                    }
                }
                if let Some(f) = v.filters.get_mut(chunk) {
                    f.insert(*ext);
                }
                Action::Write {
                    chunk: *chunk,
                    ext: *ext,
                    data: data.clone(),
                }
            }
            Request::Settle { chunk, below } if v.chunks.contains_key(chunk) => {
                let c = v.chunks.get_mut(chunk).expect("chunk");
                c.uncertain.retain(|(_, w)| w >= below);
                if c.restarted_at.is_some_and(|e| e < below.epoch) {
                    c.restarted_at = None;
                }
                Action::Reply(Response::Ok)
            }
            Request::BeginResync { chunk } if v.chunks.contains_key(chunk) => {
                v.filters.insert(*chunk, ExtentSet::new());
                Action::Reply(Response::Ok)
            }
            // The target resyncs only replicas that the metadata service places here.
            Request::BeginResync { chunk } => Action::CreateChunk { chunk: *chunk },
            Request::Copy { chunk, ext, data } if in_range(chunk, ext) => {
                let parts = match v.filters.get(chunk) {
                    Some(f) => f.missing_from(ext),
                    None => vec![*ext],
                };
                #[cfg(test)]
                let parts = if self.bugs.no_resync_filter {
                    vec![*ext]
                } else {
                    parts
                };
                Action::Copy {
                    chunk: *chunk,
                    ext: *ext,
                    data: data.clone(),
                    parts,
                }
            }
            // A request for a chunk this node does not hold, or out of its range: a protocol
            // error by the target. Dropping it makes the target's request time out.
            _ => Action::Drop,
        }
    }
}

/// The simulator's replica node: the core over in-memory blocks. Payloads are one value per
/// block of the extent.
#[derive(Debug, Clone, Default)]
pub struct Replica {
    pub core: ReplicaCore,
    data: BTreeMap<(VolumeId, ChunkIdx), BTreeMap<Block, Value>>,
    /// Size of the chunks it creates for replicas moving here (one volume shape per run).
    pub blocks_per_chunk: Block,
}

impl Replica {
    /// A node whose chunks have `blocks` blocks each.
    pub fn with_chunk_size(blocks: Block) -> Self {
        Self {
            blocks_per_chunk: blocks,
            ..Self::default()
        }
    }

    pub fn create_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx, blocks: Block) {
        self.core.add_chunk(vol, chunk, blocks);
        self.data.entry((vol, chunk)).or_default();
    }

    pub fn restart(&mut self) {
        self.core.restart();
    }

    /// Deletes a chunk replica that moved away, data included.
    pub fn remove_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx) {
        self.core.remove_chunk(vol, chunk);
        self.data.remove(&(vol, chunk));
    }

    /// Contents of a block, for checks.
    pub fn block(&self, vol: VolumeId, chunk: ChunkIdx, block: Block) -> Option<Value> {
        let c = self.data.get(&(vol, chunk))?;
        Some(c.get(&block).copied().unwrap_or(0))
    }

    /// Handles one request. Returns the reply, or `None` if the request is dropped.
    pub fn handle(&mut self, m: &ToReplica<Vec<Value>>) -> Option<FromReplica<Vec<Value>>> {
        let mut action = self.core.handle(m);
        if let Action::CreateChunk { chunk } = action {
            let blocks = self.blocks_per_chunk;
            self.create_chunk(m.vol, chunk, blocks);
            action = self.core.handle(m);
        }
        let body = match action {
            Action::CreateChunk { .. } => unreachable!("created above"),
            Action::Reply(r) | Action::Fence { reply: r, .. } => r,
            Action::Drop => return None,
            Action::Write { chunk, ext, data } => {
                assert_eq!(data.len(), ext.len as usize, "payload size");
                let c = self.data.get_mut(&(m.vol, chunk)).expect("chunk");
                for (b, v) in ext.blocks().zip(data) {
                    c.insert(b, v);
                }
                Response::Ok
            }
            Action::Read { chunk, ext } => {
                let c = &self.data[&(m.vol, chunk)];
                Response::Data(
                    ext.blocks()
                        .map(|b| c.get(&b).copied().unwrap_or(0))
                        .collect(),
                )
            }
            Action::Digest { chunk, ext } => {
                // Simulator digests are the block values (follow-ups §2). Never-written = 0.
                let c = self.data.get(&(m.vol, chunk));
                Response::Digests(
                    ext.blocks()
                        .map(|b| {
                            let v = c.and_then(|m| m.get(&b).copied()).unwrap_or(0);
                            Some(v as u32)
                        })
                        .collect(),
                )
            }
            Action::Mapped { chunk, ext } => {
                // Written blocks only (follow-ups §3). Never-written keys are absent.
                let mut mapped = ExtentSet::new();
                if let Some(c) = self.data.get(&(m.vol, chunk)) {
                    for b in ext.blocks() {
                        if c.contains_key(&b) {
                            mapped.insert(Extent::new(b, 1));
                        }
                    }
                }
                Response::Mapped(mapped.iter().collect())
            }
            Action::Discard { chunk, parts, .. } => {
                if let Some(c) = self.data.get_mut(&(m.vol, chunk)) {
                    for p in parts {
                        for b in p.blocks() {
                            c.remove(&b);
                        }
                    }
                }
                Response::Ok
            }
            Action::Copy {
                chunk,
                ext,
                data,
                parts,
            } => {
                assert_eq!(data.len(), ext.len as usize, "payload size");
                let c = self.data.get_mut(&(m.vol, chunk)).expect("chunk");
                for p in parts {
                    for b in p.blocks() {
                        c.insert(b, data[(b - ext.start) as usize]);
                    }
                }
                Response::Ok
            }
        };
        Some(FromReplica {
            vol: m.vol,
            epoch: m.epoch,
            session: m.session,
            req: m.req,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Msg = ToReplica<Vec<Value>>;

    fn msg(epoch: Epoch, session: u64, body: Request<Vec<Value>>) -> Msg {
        ToReplica {
            vol: 1,
            epoch,
            session,
            req: 0,
            body,
        }
    }

    fn write(start: Block, values: &[Value], seq: u64, prune: Option<u64>) -> Request<Vec<Value>> {
        Request::Write {
            chunk: 0,
            ext: Extent::new(start, values.len() as Block),
            data: values.to_vec(),
            version: Version { epoch: 1, seq },
            prune: prune.map(|seq| Version { epoch: 1, seq }),
        }
    }

    fn replica() -> Replica {
        let mut r = Replica::default();
        r.create_chunk(1, 0, 8);
        r
    }

    fn body(r: Option<FromReplica<Vec<Value>>>) -> Option<Response<Vec<Value>>> {
        r.map(|r| r.body)
    }

    fn uncertain(r: Option<FromReplica<Vec<Value>>>) -> Vec<Extent> {
        match body(r) {
            Some(Response::Fenced { uncertain }) => uncertain[0].1.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fencing_refuses_older_epochs_and_survives_restart() {
        let mut r = replica();
        r.handle(&msg(3, 1, Request::Hello)).unwrap();
        r.restart();
        assert_eq!(
            body(r.handle(&msg(2, 9, Request::Hello))),
            Some(Response::Stale { fenced: 3 })
        );
        assert_eq!(
            body(r.handle(&msg(2, 1, Request::Heartbeat))),
            Some(Response::Stale { fenced: 3 })
        );
    }

    #[test]
    fn only_the_current_session_is_served() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        r.handle(&msg(1, 2, Request::Hello)).unwrap();
        assert_eq!(r.handle(&msg(1, 1, write(0, &[5], 0, None))), None);
        assert_eq!(r.block(1, 0, 0), Some(0));
        assert_eq!(
            body(r.handle(&msg(1, 2, write(0, &[5], 0, None)))),
            Some(Response::Ok)
        );
        // An older session's late Hello does not take over.
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        assert_eq!(
            body(r.handle(&msg(1, 2, Request::Heartbeat))),
            Some(Response::Ok)
        );
        // After a restart, nothing is current until a Hello.
        r.restart();
        assert_eq!(r.handle(&msg(1, 2, Request::Heartbeat)), None);
    }

    #[test]
    fn out_of_range_requests_are_dropped() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        assert_eq!(r.handle(&msg(1, 1, write(7, &[1, 2], 0, None))), None);
        let read = Request::Read {
            chunk: 3,
            ext: Extent::new(0, 1),
        };
        assert_eq!(r.handle(&msg(1, 1, read)), None);
    }

    #[test]
    fn uncertain_set_is_pruned_by_completed_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        r.handle(&msg(1, 1, write(0, &[10, 11], 0, None))).unwrap();
        r.handle(&msg(1, 1, write(2, &[12], 1, Some(0)))).unwrap();
        r.handle(&msg(1, 1, write(5, &[13, 14], 2, Some(2))))
            .unwrap();
        assert_eq!(
            uncertain(r.handle(&msg(2, 1, Request::Hello))),
            vec![Extent::new(5, 2)]
        );
    }

    #[test]
    fn after_a_restart_everything_is_uncertain_until_a_newer_epoch_prunes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        r.restart();
        assert_eq!(
            uncertain(r.handle(&msg(1, 2, Request::Hello))),
            vec![Extent::new(0, 8)]
        );
        // Pruning by the same epoch is not enough: its unfinished writes may be anywhere.
        r.handle(&msg(1, 2, write(0, &[1], 5, Some(5)))).unwrap();
        assert_eq!(
            uncertain(r.handle(&msg(2, 1, Request::Hello))),
            vec![Extent::new(0, 8)]
        );
        let newer = Request::Write {
            chunk: 0,
            ext: Extent::new(3, 1),
            data: vec![9],
            version: Version { epoch: 2, seq: 0 },
            prune: Some(Version { epoch: 2, seq: 1 }),
        };
        r.handle(&msg(2, 1, newer)).unwrap();
        assert_eq!(uncertain(r.handle(&msg(3, 1, Request::Hello))), vec![]);
    }

    #[test]
    fn copies_skip_blocks_with_live_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        r.handle(&msg(1, 1, Request::BeginResync { chunk: 0 }))
            .unwrap();
        r.handle(&msg(1, 1, write(1, &[20, 21], 0, None))).unwrap();
        let copy = Request::Copy {
            chunk: 0,
            ext: Extent::new(0, 4),
            data: vec![1, 2, 3, 4],
        };
        r.handle(&msg(1, 1, copy)).unwrap();
        let blocks: Vec<_> = (0..4).map(|b| r.block(1, 0, b).unwrap()).collect();
        assert_eq!(blocks, vec![1, 20, 21, 4]);
    }
}
