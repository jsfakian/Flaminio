//! A node holding chunk replicas (protocol spec §3, §6, §7).
//!
//! Durable state survives [`Replica::restart`]: block contents, uncertain sets and fenced
//! epochs. Sessions and resync filters do not.

use std::collections::{BTreeMap, BTreeSet};

use crate::msg::{FromReplica, Request, Response, ToReplica};
use crate::types::{Block, ChunkIdx, Epoch, Value, Version, VolumeId};

#[derive(Debug, Clone, Default)]
struct ChunkReplica {
    blocks: BTreeMap<Block, Value>,
    /// Blocks whose last write may not have completed, with that write's version.
    uncertain: BTreeMap<Block, Version>,
}

#[derive(Debug, Clone, Default)]
struct Vol {
    fenced: Epoch,
    chunks: BTreeMap<ChunkIdx, ChunkReplica>,
    /// Volatile: the current `(epoch, session)`.
    session: Option<(Epoch, u64)>,
    /// Volatile: per chunk under resync, the blocks that got live writes since `BeginResync`.
    filters: BTreeMap<ChunkIdx, BTreeSet<Block>>,
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

#[derive(Debug, Clone, Default)]
pub struct Replica {
    vols: BTreeMap<VolumeId, Vol>,
    #[cfg(test)]
    pub bugs: Bugs,
}

impl Replica {
    /// Creates an empty chunk replica.
    pub fn create_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx) {
        self.vols
            .entry(vol)
            .or_default()
            .chunks
            .entry(chunk)
            .or_default();
    }

    /// Loses volatile state, as after a process or node crash.
    pub fn restart(&mut self) {
        for v in self.vols.values_mut() {
            v.session = None;
            v.filters.clear();
        }
    }

    /// Contents of a block, for checks.
    pub fn block(&self, vol: VolumeId, chunk: ChunkIdx, block: Block) -> Option<Value> {
        let c = self.vols.get(&vol)?.chunks.get(&chunk)?;
        Some(c.blocks.get(&block).copied().unwrap_or(0))
    }

    /// Handles one request. Returns the reply, or `None` if the request is dropped.
    pub fn handle(&mut self, m: &ToReplica) -> Option<FromReplica> {
        let reply = |body| {
            Some(FromReplica {
                vol: m.vol,
                epoch: m.epoch,
                session: m.session,
                req: m.req,
                body,
            })
        };
        let v = self.vols.entry(m.vol).or_default();
        if m.epoch < v.fenced {
            return reply(Response::Stale { fenced: v.fenced });
        }
        if m.body == Request::Hello {
            // Durable before the reply.
            v.fenced = m.epoch;
            if v.session.is_none_or(|s| s < (m.epoch, m.session)) {
                v.session = Some((m.epoch, m.session));
                v.filters.clear();
            }
            let uncertain = v
                .chunks
                .iter()
                .map(|(&c, r)| (c, r.uncertain.keys().copied().collect()))
                .collect();
            return reply(Response::Fenced { uncertain });
        }
        let current = v.session == Some((m.epoch, m.session));
        #[cfg(test)]
        let current = current || (self.bugs.any_session && m.epoch == v.fenced);
        if !current {
            return None;
        }
        match m.body {
            Request::Hello => unreachable!(),
            Request::Heartbeat => reply(Response::Ok),
            Request::Read { chunk, block } => {
                let c = v.chunks.get(&chunk)?;
                reply(Response::Value(c.blocks.get(&block).copied().unwrap_or(0)))
            }
            Request::Write {
                chunk,
                block,
                value,
                version,
                prune,
            } => {
                let c = v.chunks.get_mut(&chunk)?;
                c.blocks.insert(block, value);
                let u = c.uncertain.entry(block).or_insert(version);
                *u = (*u).max(version);
                if let Some(p) = prune {
                    c.uncertain.retain(|_, w| *w >= p);
                }
                if let Some(f) = v.filters.get_mut(&chunk) {
                    f.insert(block);
                }
                reply(Response::Ok)
            }
            Request::BeginResync { chunk } => {
                v.chunks.get(&chunk)?;
                v.filters.insert(chunk, BTreeSet::new());
                reply(Response::Ok)
            }
            Request::Copy {
                chunk,
                block,
                value,
            } => {
                let skip = v.filters.get(&chunk).is_some_and(|f| f.contains(&block));
                #[cfg(test)]
                let skip = skip && !self.bugs.no_resync_filter;
                let c = v.chunks.get_mut(&chunk)?;
                if !skip {
                    c.blocks.insert(block, value);
                }
                reply(Response::Ok)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(epoch: Epoch, session: u64, body: Request) -> ToReplica {
        ToReplica {
            vol: 1,
            epoch,
            session,
            req: 0,
            body,
        }
    }

    fn write(block: Block, value: Value, seq: u64, prune: Option<u64>) -> Request {
        Request::Write {
            chunk: 0,
            block,
            value,
            version: Version { epoch: 1, seq },
            prune: prune.map(|seq| Version { epoch: 1, seq }),
        }
    }

    fn replica() -> Replica {
        let mut r = Replica::default();
        r.create_chunk(1, 0);
        r
    }

    fn body(r: Option<FromReplica>) -> Option<Response> {
        r.map(|r| r.body)
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
        assert_eq!(r.handle(&msg(1, 1, write(0, 5, 0, None))), None);
        assert_eq!(r.block(1, 0, 0), Some(0));
        assert_eq!(
            body(r.handle(&msg(1, 2, write(0, 5, 0, None)))),
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
    fn uncertain_set_is_pruned_by_completed_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        r.handle(&msg(1, 1, write(0, 10, 0, None))).unwrap();
        r.handle(&msg(1, 1, write(1, 11, 1, Some(0)))).unwrap();
        r.handle(&msg(1, 1, write(2, 12, 2, Some(2)))).unwrap();
        let fenced = body(r.handle(&msg(2, 1, Request::Hello)));
        assert_eq!(
            fenced,
            Some(Response::Fenced {
                uncertain: vec![(0, vec![2])]
            })
        );
    }

    #[test]
    fn copies_skip_blocks_with_live_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello)).unwrap();
        let copy = |block, value| Request::Copy {
            chunk: 0,
            block,
            value,
        };
        r.handle(&msg(1, 1, Request::BeginResync { chunk: 0 }))
            .unwrap();
        r.handle(&msg(1, 1, write(0, 20, 0, None))).unwrap();
        r.handle(&msg(1, 1, copy(0, 1))).unwrap();
        r.handle(&msg(1, 1, copy(1, 2))).unwrap();
        assert_eq!(r.block(1, 0, 0), Some(20));
        assert_eq!(r.block(1, 0, 1), Some(2));
    }
}
