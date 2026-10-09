//! The replica service: replication sessions served from the replica store.
//!
//! Every request goes through [`ReplicaCore`] (protocol spec §3, §6, §7), which decides; this
//! module carries out its decisions on the store:
//! - fences are made durable before the `Hello` reply;
//! - consecutive writes and copies, from all sessions, share one group commit;
//! - a read first commits the writes before it.
//!
//! A store failure on a request closes its session: the target then treats this replica as
//! failed, which the protocol already handles.

use std::collections::{BTreeMap, BTreeSet};

use flaminio_format::Uuid;
use flaminio_net::BLOCK;
use flaminio_net::frame::Frame;
use flaminio_net::session::Accepted;
use flaminio_repl::extent::Extent;
use flaminio_repl::msg::{Request, Response, ToReplica};
use flaminio_repl::replica::{Action, ReplicaCore};
use flaminio_repl::types::{Block, VolumeId};
use flaminio_store::{BlockDevice, ChunkKey, ReadReq, Store, StoreError, WriteReq};

use crate::volume_uuid;

/// The session a request arrived on, as established by its `Hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRef {
    pub vol: VolumeId,
    pub epoch: u64,
    pub session: u64,
}

/// What to do with a request's connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Send this frame.
    Send(Frame),
    /// Send nothing: the request belongs to a session that is no longer current.
    Nothing,
    /// Send `Refused` with the reason, then close the connection.
    Close(String),
}

#[derive(Debug)]
struct Vol {
    id: VolumeId,
    size: u64,
    chunk_size: u64,
    chunks: BTreeSet<u32>,
}

/// Replication sessions on one replica store.
pub struct ReplicaService<D: BlockDevice> {
    store: Store<D>,
    core: ReplicaCore,
    vols: BTreeMap<Uuid, Vol>,
    next_id: VolumeId,
}

/// A write to commit: the store request's parts, with the index of the request it answers.
struct PendingWrite {
    item: usize,
    key: ChunkKey,
    offset: u64,
    /// Byte range of the request frame's payload.
    range: std::ops::Range<usize>,
    seq: u64,
    epoch: u64,
}

impl<D: BlockDevice> ReplicaService<D> {
    /// Starts on an open store. The chunks found on it are known to the core, fenced at their
    /// recorded epochs and wholly uncertain, as after any restart.
    pub fn new(store: Store<D>) -> Self {
        let mut s = Self {
            store,
            core: ReplicaCore::default(),
            vols: BTreeMap::new(),
            next_id: 1,
        };
        for c in s.store.chunks() {
            let id = s.vol_id(c.key.volume, c.volume_size, c.chunk_size);
            let v = s.vols.get_mut(&c.key.volume).expect("just added");
            v.chunks.insert(c.key.index as u32);
            let blocks = (c.chunk_size / BLOCK as u64) as Block;
            s.core
                .recover_chunk(id, c.fenced_epoch, c.key.index as u32, blocks);
        }
        s
    }

    pub fn store(&self) -> &Store<D> {
        &self.store
    }

    pub fn into_store(self) -> Store<D> {
        self.store
    }

    fn vol_id(&mut self, uuid: Uuid, size: u64, chunk_size: u64) -> VolumeId {
        if let Some(v) = self.vols.get(&uuid) {
            return v.id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.vols.insert(
            uuid,
            Vol {
                id,
                size,
                chunk_size,
                chunks: BTreeSet::new(),
            },
        );
        id
    }

    /// Opens an accepted session (its token already checked by the transport). Returns the
    /// session and the reply to send, or a reason to close the connection.
    pub fn hello(&mut self, a: &Accepted) -> Result<(SessionRef, Frame), String> {
        let (size, chunk_size) = (a.size, a.chunk_size);
        if chunk_size == 0
            || !chunk_size.is_multiple_of(BLOCK as u64)
            || size == 0
            || !size.is_multiple_of(chunk_size)
        {
            return Err(
                "volume size must be a non-zero multiple of a 4 KiB-aligned chunk size".into(),
            );
        }
        let count = size / chunk_size;
        if a.chunks.iter().any(|&c| u64::from(c) >= count) {
            return Err("a placed chunk is beyond the volume".into());
        }
        let uuid = volume_uuid(&a.claims.volume);
        if let Some(v) = self.vols.get(&uuid)
            && (v.chunk_size != chunk_size || v.size != size)
        {
            return Err(format!(
                "volume exists here with {} bytes in {}-byte chunks",
                v.size, v.chunk_size
            ));
        }
        let vol = self.vol_id(uuid, size, chunk_size);
        let session = SessionRef {
            vol,
            epoch: a.epoch,
            session: a.session,
        };
        let msg = ToReplica {
            vol,
            epoch: a.epoch,
            session: a.session,
            req: a.req,
            body: Request::<()>::Hello,
        };
        let reply = match self.core.handle(&msg) {
            Action::Fence { epoch, reply } => {
                // Create the chunks placed here that do not exist yet, fenced at this epoch,
                // then fence the others: durable before the reply.
                let blocks = (chunk_size / BLOCK as u64) as Block;
                let missing: Vec<u32> = {
                    let v = &self.vols[&uuid];
                    a.chunks
                        .iter()
                        .copied()
                        .filter(|c| !v.chunks.contains(c))
                        .collect()
                };
                for c in missing {
                    let key = ChunkKey {
                        volume: uuid,
                        index: u64::from(c),
                    };
                    self.store
                        .create_chunk(key, chunk_size, size, epoch)
                        .map_err(|e| e.to_string())?;
                    self.core.add_chunk(vol, c, blocks);
                    self.vols.get_mut(&uuid).expect("volume").chunks.insert(c);
                }
                self.store.fence(uuid, epoch).map_err(|e| e.to_string())?;
                reply
            }
            Action::Reply(r) => r,
            other => return Err(format!("unexpected action for Hello: {other:?}")),
        };
        Ok((session, response_frame(a.req, reply)))
    }

    /// Handles requests from any sessions, in order, and returns one reply per request. A
    /// `Hello` here is refused: a connection carries one session, opened by [`Self::hello`].
    pub fn handle(&mut self, items: &[(SessionRef, Frame)]) -> Vec<Reply> {
        let mut replies: Vec<Option<Reply>> = vec![None; items.len()];
        let mut pending: Vec<PendingWrite> = Vec::new();
        for (i, (s, f)) in items.iter().enumerate() {
            let Some((req, body)) = to_request(f) else {
                replies[i] = Some(Reply::Close("not a request".into()));
                continue;
            };
            let msg = ToReplica {
                vol: s.vol,
                epoch: s.epoch,
                session: s.session,
                req,
                body,
            };
            let Some(uuid) = self.uuid_of(s.vol) else {
                replies[i] = Some(Reply::Close("unknown volume".into()));
                continue;
            };
            match self.core.handle(&msg) {
                Action::Reply(r) => replies[i] = Some(Reply::Send(response_frame(req, r))),
                Action::Fence { .. } => unreachable!("Hello never reaches handle"),
                Action::Drop => replies[i] = Some(Reply::Nothing),
                Action::Write { chunk, ext, .. } => {
                    let Frame::Write { version, data, .. } = f else {
                        unreachable!("a write action comes from a write")
                    };
                    pending.push(PendingWrite {
                        item: i,
                        key: key(uuid, chunk),
                        offset: offset(ext),
                        range: 0..data.len(),
                        seq: version.seq,
                        epoch: s.epoch,
                    });
                }
                Action::Copy {
                    chunk, ext, parts, ..
                } => {
                    if parts.is_empty() {
                        replies[i] = Some(Reply::Send(Frame::Ok { req }));
                    }
                    for p in parts {
                        let from = (p.start - ext.start) as usize * BLOCK;
                        pending.push(PendingWrite {
                            item: i,
                            key: key(uuid, chunk),
                            offset: offset(p),
                            range: from..from + p.len as usize * BLOCK,
                            seq: 0,
                            epoch: s.epoch,
                        });
                    }
                }
                Action::Read { chunk, ext } => {
                    // Reads see every write before them.
                    self.commit(items, &mut pending, &mut replies, req_of);
                    let mut buf = vec![0u8; ext.len as usize * BLOCK];
                    let mut r = [ReadReq {
                        key: key(uuid, chunk),
                        offset: offset(ext),
                        buf: &mut buf,
                        epoch: s.epoch,
                    }];
                    replies[i] = Some(match self.store.read_batch(&mut r) {
                        Ok(mut res) => match res.pop().expect("one result") {
                            Ok(()) => Reply::Send(Frame::Data { req, data: buf }),
                            Err(e) => Reply::Close(e.to_string()),
                        },
                        Err(e) => Reply::Close(e.to_string()),
                    });
                }
            }
        }
        self.commit(items, &mut pending, &mut replies, req_of);
        replies
            .into_iter()
            .map(|r| r.expect("every request answered"))
            .collect()
    }

    /// Commits the pending writes as one group, and fills in their replies.
    fn commit(
        &mut self,
        items: &[(SessionRef, Frame)],
        pending: &mut Vec<PendingWrite>,
        replies: &mut [Option<Reply>],
        req_of: fn(&Frame) -> u64,
    ) {
        if pending.is_empty() {
            return;
        }
        let reqs: Vec<WriteReq<'_>> = pending
            .iter()
            .map(|p| WriteReq {
                key: p.key,
                offset: p.offset,
                data: &payload(&items[p.item].1)[p.range.clone()],
                write_seq: p.seq,
                epoch: p.epoch,
            })
            .collect();
        let results: Vec<Result<(), StoreError>> = match self.store.write_batch(&reqs) {
            Ok(r) => r,
            Err(e) => pending.iter().map(|_| Err(clone_err(&e))).collect(),
        };
        for (p, r) in pending.iter().zip(results) {
            let reply = &mut replies[p.item];
            match r {
                // A copy split in parts answers once, unless a part failed.
                Ok(()) => {
                    if reply.is_none() {
                        *reply = Some(Reply::Send(Frame::Ok {
                            req: req_of(&items[p.item].1),
                        }));
                    }
                }
                Err(e) => *reply = Some(Reply::Close(e.to_string())),
            }
        }
        pending.clear();
    }

    fn uuid_of(&self, id: VolumeId) -> Option<Uuid> {
        self.vols.iter().find(|(_, v)| v.id == id).map(|(&u, _)| u)
    }
}

fn clone_err(e: &StoreError) -> StoreError {
    StoreError::Invalid(e.to_string())
}

fn key(volume: Uuid, chunk: u32) -> ChunkKey {
    ChunkKey {
        volume,
        index: u64::from(chunk),
    }
}

fn offset(ext: Extent) -> u64 {
    u64::from(ext.start) * BLOCK as u64
}

fn payload(f: &Frame) -> &[u8] {
    match f {
        Frame::Write { data, .. } | Frame::Copy { data, .. } => data,
        _ => &[],
    }
}

fn req_of(f: &Frame) -> u64 {
    match f {
        Frame::Hello { req, .. }
        | Frame::Heartbeat { req }
        | Frame::Write { req, .. }
        | Frame::Read { req, .. }
        | Frame::BeginResync { req, .. }
        | Frame::Copy { req, .. }
        | Frame::Fenced { req, .. }
        | Frame::Ok { req }
        | Frame::Data { req, .. }
        | Frame::Stale { req, .. } => *req,
        Frame::Refused { .. } => 0,
    }
}

/// A request frame as a core request. Payloads stay in the frame.
fn to_request(f: &Frame) -> Option<(u64, Request<()>)> {
    Some(match *f {
        Frame::Heartbeat { req } => (req, Request::Heartbeat),
        Frame::Write {
            req,
            chunk,
            ext,
            version,
            prune,
            ..
        } => (
            req,
            Request::Write {
                chunk,
                ext,
                data: (),
                version,
                prune,
            },
        ),
        Frame::Read { req, chunk, ext } => (req, Request::Read { chunk, ext }),
        Frame::BeginResync { req, chunk } => (req, Request::BeginResync { chunk }),
        Frame::Copy {
            req, chunk, ext, ..
        } => (
            req,
            Request::Copy {
                chunk,
                ext,
                data: (),
            },
        ),
        // A Hello opens a connection's only session (see `hello`); anything else is not a
        // request.
        _ => return None,
    })
}

fn response_frame(req: u64, r: Response<()>) -> Frame {
    match r {
        Response::Fenced { uncertain } => Frame::Fenced { req, uncertain },
        Response::Ok => Frame::Ok { req },
        Response::Stale { fenced } => Frame::Stale { req, fenced },
        Response::Data(()) => unreachable!("reads are carried out by the service"),
    }
}

#[cfg(test)]
mod tests;
