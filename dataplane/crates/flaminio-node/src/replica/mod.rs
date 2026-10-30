//! The replica service: replication sessions served from the replica store.
//!
//! Every request goes through [`ReplicaCore`] (protocol spec §3, §6, §7), which decides; this
//! module carries out its decisions on the store:
//! - fences are made durable before the `Hello` reply;
//! - consecutive writes and copies, from all sessions, share one group commit;
//! - a read first commits the writes before it;
//! - with [`ReplicaService::start`], a commit's I/O can stay in flight while the caller does
//!   other work: the requests after its writes are answered with them, once durable.
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
use flaminio_store::{BlockDevice, ChunkKey, ReadReq, Started, Store, StoreError, WriteReq};

use crate::volume_uuid;

/// The session a request arrived on, as established by its `Hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRef {
    pub vol: VolumeId,
    pub epoch: u64,
    pub session: u64,
}

/// The outcome of [`ReplicaService::admit_read`].
#[derive(Debug)]
pub enum Admit {
    /// Read `len` bytes at `offset` of chunk `key`, with the session's `epoch`.
    Read {
        key: ChunkKey,
        offset: u64,
        len: usize,
        epoch: u64,
    },
    /// Do not read: answer with this.
    Reply(Reply),
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
    /// The group commits in flight, oldest first (several with pipelined commits). A group
    /// whose requests were all answered without a commit waits here too, for its turn.
    flying: std::collections::VecDeque<Flying>,
    /// The next group, staged behind them (see [`ReplicaService::stage`]).
    staged: Option<Flying>,
    /// When true, [`Self::poll`] does not move the staged group into flight: the engine grows
    /// it and starts it with [`Self::launch`] (see [`Store::set_hold_staged`]).
    hold_staged: bool,
}

/// What [`ReplicaService::start`] did with a batch of requests.
#[derive(Debug)]
pub struct Progress {
    /// Replies to the first `replies.len()` requests.
    pub replies: Vec<Reply>,
    /// How many requests were handled. Those after `replies` wait for the commit in flight,
    /// and [`ReplicaService::poll`] answers them; those after `handled` were not looked at,
    /// and go to `start` again once it has.
    pub handled: usize,
}

impl Progress {
    /// True if a commit is in flight: some handled requests are not answered yet.
    pub fn in_flight(&self) -> bool {
        self.replies.len() < self.handled
    }
}

/// A commit in flight, and the requests it holds back.
struct Flying {
    /// Replies to the held requests (the first one is the commit's first write), the writes'
    /// to be filled in.
    replies: Vec<Option<Reply>>,
    /// For each store request of the commit: its held request (an index into `replies`), and
    /// that request's id.
    writes: Vec<(usize, u64)>,
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
            flying: std::collections::VecDeque::new(),
            staged: None,
            hold_staged: false,
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

    /// Creates an empty chunk replica for a replica moving here, fenced at `epoch`.
    fn create_moved_chunk(
        &mut self,
        uuid: Uuid,
        vol: VolumeId,
        chunk: u32,
        epoch: u64,
    ) -> Result<(), String> {
        let v = self.vols.get(&uuid).ok_or("unknown volume")?;
        let (size, chunk_size) = (v.size, v.chunk_size);
        if u64::from(chunk) >= size.div_ceil(chunk_size) {
            return Err("out of range".into());
        }
        let key = ChunkKey {
            volume: uuid,
            index: u64::from(chunk),
        };
        self.store
            .create_chunk(key, chunk_size, size, epoch)
            .map_err(|e| e.to_string())?;
        self.core
            .add_chunk(vol, chunk, (chunk_size / BLOCK as u64) as Block);
        self.vols
            .get_mut(&uuid)
            .expect("volume")
            .chunks
            .insert(chunk);
        Ok(())
    }

    /// Handles requests from any sessions, in order, and returns one reply per request. A
    /// `Hello` here is refused: a connection carries one session, opened by [`Self::hello`].
    pub fn handle(&mut self, items: &[(SessionRef, Frame)]) -> Vec<Reply> {
        let mut replies = Vec::with_capacity(items.len());
        while replies.len() < items.len() {
            let done = replies.len();
            let p = self.start(&items[done..]);
            let in_flight = p.in_flight();
            replies.extend(p.replies);
            if in_flight {
                replies.extend(self.poll(true).expect("waited for the commit"));
            }
            debug_assert_eq!(replies.len(), done + p.handled);
        }
        replies
    }

    /// [`Self::handle`], without waiting for a commit's I/O when the store's device can do it
    /// asynchronously: see [`Progress`]. Nothing may start while a commit is in flight.
    pub fn start(&mut self, items: &[(SessionRef, Frame)]) -> Progress {
        assert!(
            !self.commit_in_flight(),
            "a commit is in flight: poll it first"
        );
        let mut replies: Vec<Option<Reply>> = vec![None; items.len()];
        let mut pending: Vec<PendingWrite> = Vec::new();
        for (i, (s, f)) in items.iter().enumerate() {
            if matches!(f, Frame::Read { .. })
                && !pending.is_empty()
                && self.commit(items, &mut pending, &mut replies)
            {
                // Reads see every write before them: this one waits for the commit.
                return self.held(replies, i);
            }
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
            let mut action = self.core.handle(&msg);
            if let Action::CreateChunk { chunk } = action {
                // A replica moving here: create the chunk, empty, then handle the request again.
                if let Err(e) = self.create_moved_chunk(uuid, s.vol, chunk, s.epoch) {
                    replies[i] = Some(Reply::Close(format!("create chunk {chunk}: {e}")));
                    continue;
                }
                action = self.core.handle(&msg);
            }
            match action {
                Action::CreateChunk { .. } => replies[i] = Some(Reply::Nothing),
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
                Action::Digest { chunk, ext } => {
                    replies[i] = Some(
                        match self.store.digests(key(uuid, chunk), offset(ext), len(ext)) {
                            Ok(sums) => Reply::Send(Frame::Digests { req, sums }),
                            Err(e) => Reply::Close(e.to_string()),
                        },
                    );
                }
                Action::Mapped { chunk, ext } => {
                    replies[i] = Some(
                        match self.store.mapped(key(uuid, chunk), offset(ext), len(ext)) {
                            Ok(ranges) => {
                                let exts = ranges
                                    .into_iter()
                                    .map(|(o, l)| {
                                        Extent::new(
                                            (o / BLOCK as u64) as u32,
                                            (l / BLOCK as u64) as u32,
                                        )
                                    })
                                    .collect();
                                Reply::Send(Frame::Mappings { req, exts })
                            }
                            Err(e) => Reply::Close(e.to_string()),
                        },
                    );
                }
                Action::Discard { chunk, parts, .. } => {
                    let mut err = None;
                    for p in &parts {
                        if let Err(e) =
                            self.store
                                .discard(key(uuid, chunk), offset(*p), len(*p), s.epoch)
                        {
                            err = Some(e);
                            break;
                        }
                    }
                    replies[i] = Some(match err {
                        None => Reply::Send(Frame::Ok { req }),
                        Some(e) => Reply::Close(e.to_string()),
                    });
                }
            }
        }
        if self.commit(items, &mut pending, &mut replies) {
            return self.held(replies, items.len());
        }
        Progress {
            replies: replies
                .into_iter()
                .map(|r| r.expect("every request answered"))
                .collect(),
            handled: items.len(),
        }
    }

    /// True when several commits may be in flight at once (the store's pipelined commits).
    pub fn pipelined(&self) -> bool {
        self.store.pipelined()
    }

    /// True while staged groups are held (see [`Self::set_hold_staged`]).
    pub fn holding(&self) -> bool {
        self.hold_staged
    }

    /// Defers starting the staged group when a commit lands (see [`Store::set_hold_staged`]).
    pub fn set_hold_staged(&mut self, hold: bool) {
        self.hold_staged = hold;
        self.store.set_hold_staged(hold);
    }

    /// No staged partner for flush coalesce: let the commit in flight flush alone.
    pub fn clear_coalesce_expect(&mut self) {
        self.store.clear_coalesce_expect();
    }

    /// Stages writes behind the commit in flight (the store's [`Store::stage_writes`]): takes
    /// the longest prefix of `items` made of writes the store can stage now, decides them, and
    /// holds their replies for their own commit, which starts as soon as the one in flight is
    /// durable (or sooner via [`Self::coalesce_partner`] / [`Self::launch`]). Returns how many
    /// items it took; the rest wait, in order.
    pub fn stage(&mut self, items: &[(SessionRef, Frame)]) -> usize {
        // Behind a commit in flight; or, with hold_staged, with nothing in flight too: the
        // writes start or join the held group (docs/10, S2).
        if self.flying.is_empty() && !self.hold_staged {
            return 0;
        }
        let mut taken = 0;
        for (s, f) in items {
            let Frame::Write {
                req, chunk, ext, ..
            } = f
            else {
                break;
            };
            let Some(uuid) = self.uuid_of(s.vol) else {
                break;
            };
            let key = key(uuid, *chunk);
            let at = offset(*ext);
            if !self.store.can_stage(key, at, payload(f).len() as u64) {
                break;
            }
            let (req, body) = (*req, to_request(f).expect("a write").1);
            let msg = ToReplica {
                vol: s.vol,
                epoch: s.epoch,
                session: s.session,
                req,
                body,
            };
            let st = self.staged.get_or_insert_with(|| Flying {
                replies: Vec::new(),
                writes: Vec::new(),
            });
            let at_reply = st.replies.len();
            match self.core.handle(&msg) {
                Action::Write { .. } => {
                    let Frame::Write { version, data, .. } = f else {
                        unreachable!("a write")
                    };
                    let w = WriteReq {
                        key,
                        offset: at,
                        data: data.clone().into(),
                        write_seq: version.seq,
                        epoch: s.epoch,
                    };
                    let st = self.staged.as_mut().expect("staged");
                    match self.store.stage_writes(&[w]) {
                        Ok(1) => {
                            st.replies.push(None);
                            st.writes.push((at_reply, req));
                        }
                        // can_stage said yes and the store took nothing: it failed.
                        Ok(_) => st.replies.push(Some(Reply::Close(
                            "the store refused a staged write".into(),
                        ))),
                        Err(e) => st.replies.push(Some(Reply::Close(e.to_string()))),
                    }
                }
                Action::Reply(r) => self
                    .staged
                    .as_mut()
                    .expect("staged")
                    .replies
                    .push(Some(Reply::Send(response_frame(req, r)))),
                Action::Drop => self
                    .staged
                    .as_mut()
                    .expect("staged")
                    .replies
                    .push(Some(Reply::Nothing)),
                other => self
                    .staged
                    .as_mut()
                    .expect("staged")
                    .replies
                    .push(Some(Reply::Close(format!(
                        "unexpected action for a staged write: {other:?}"
                    )))),
            }
            taken += 1;
        }
        taken
    }

    /// With flush coalesce, promotes the staged group's journal behind the commit in flight so
    /// both share one fdatasync. Returns whether it did: the caller must move its staged
    /// tracking into the commits in flight, as with [`Self::launch`].
    pub fn coalesce_partner(&mut self) -> bool {
        match self.store.maybe_coalesce_partner() {
            Ok(true) => {
                if let Some(st) = self.staged.take() {
                    self.flying.push_back(st);
                }
                true
            }
            // A store failure shows at the next poll.
            Ok(false) | Err(_) => false,
        }
    }

    /// The replies to the requests the oldest commit in flight held back, once it is durable
    /// (with `wait`, waits for it). `None` while it is still in flight. A group staged behind
    /// the commits in flight joins them when the oldest lands (its commit starts then), and a
    /// later poll answers it.
    pub fn poll(&mut self, wait: bool) -> Option<Vec<Reply>> {
        let f = self.flying.front()?;
        if f.writes.is_empty() {
            // Answered without a commit: its turn has come.
            let f = self.flying.pop_front().expect("in flight");
            return Some(
                f.replies
                    .into_iter()
                    .map(|r| r.expect("answered"))
                    .collect(),
            );
        }
        let results = match self.store.poll_write_batch(wait) {
            Ok(None) => return None,
            Ok(Some(r)) => r,
            Err(e) => f.writes.iter().map(|_| Err(clone_err(&e))).collect(),
        };
        let Flying {
            mut replies,
            writes,
        } = self.flying.pop_front().expect("in flight");
        // Without hold, the store started the staged group's commit with this one's landing.
        if !self.hold_staged
            && let Some(st) = self.staged.take()
        {
            self.flying.push_back(st);
        }
        for ((at, req), r) in writes.into_iter().zip(results) {
            fill(&mut replies[at], req, r);
        }
        Some(
            replies
                .into_iter()
                .map(|r| r.expect("every held request answered"))
                .collect(),
        )
    }

    /// True while a commit is in flight.
    pub fn commit_in_flight(&self) -> bool {
        !self.flying.is_empty()
    }

    /// Starts the staged group's commit now, behind the commits in flight, if the store can
    /// (pipelined commits, below their depth: `Store::launch_staged`). Returns whether it did.
    ///
    /// A group whose items were all answered without a write (stale epochs, dropped sessions,
    /// store refusals) has nothing for the store to commit. With nothing in flight it is
    /// answered now, behind a commit in flight at that commit's landing (docs/10, S5).
    pub fn launch(&mut self) -> bool {
        let Some(st) = &self.staged else {
            return false;
        };
        if st.writes.is_empty() {
            if !self.flying.is_empty() {
                return false;
            }
            let st = self.staged.take().expect("staged");
            self.flying.push_back(st);
            return true;
        }
        // A settle behind the engine's back would have started the store's group already,
        // stranding this one (docs/10, S5).
        debug_assert!(
            self.store.holds_group(),
            "the store lost the staged group's writes"
        );
        match self.store.launch_staged() {
            Ok(true) => {
                let st = self.staged.take().expect("staged");
                self.flying.push_back(st);
                true
            }
            Ok(false) => false,
            // The store is poisoned now (it was, or the promotion failed): nothing retries.
            // The group's writes are answered with the error, and the group is answered in its
            // turn, without the store.
            Err(e) => {
                let mut st = self.staged.take().expect("staged");
                for (at, req) in std::mem::take(&mut st.writes) {
                    fill(&mut st.replies[at], req, Err(clone_err(&e)));
                }
                self.flying.push_back(st);
                true
            }
        }
    }

    /// The store's data cluster size, in bytes.
    pub fn cluster_size(&self) -> u64 {
        self.store.cluster_size()
    }

    /// The progress of `start` when a commit stays in flight: the requests up to its first
    /// write are answered; those up to `handled` are held for it.
    fn held(&mut self, replies: Vec<Option<Reply>>, handled: usize) -> Progress {
        let f = self.flying.back_mut().expect("a commit in flight");
        let first = f.writes.first().expect("a commit has writes").0;
        let mut replies = replies;
        replies.truncate(handled);
        let held = replies.split_off(first);
        for w in &mut f.writes {
            w.0 -= first;
        }
        f.replies = held;
        Progress {
            replies: replies
                .into_iter()
                .map(|r| r.expect("requests before the commit answered"))
                .collect(),
            handled,
        }
    }

    /// Commits the pending writes as one group. Returns true if its I/O is in flight
    /// (`self.flying` then holds it); otherwise their replies are filled in.
    fn commit(
        &mut self,
        items: &[(SessionRef, Frame)],
        pending: &mut Vec<PendingWrite>,
        replies: &mut [Option<Reply>],
    ) -> bool {
        if pending.is_empty() {
            return false;
        }
        let reqs: Vec<WriteReq<'_>> = pending
            .iter()
            .map(|p| WriteReq {
                key: p.key,
                offset: p.offset,
                // A slice of the frame's shared buffer: the store writes it without a copy.
                data: shared_payload(&items[p.item].1)
                    .slice(p.range.clone())
                    .into(),
                write_seq: p.seq,
                epoch: p.epoch,
            })
            .collect();
        let started = self.store.start_write_batch(&reqs);
        let writes: Vec<(usize, u64)> = pending
            .drain(..)
            .map(|p| (p.item, req_of(&items[p.item].1)))
            .collect();
        let results: Vec<Result<(), StoreError>> = match started {
            Ok(Started::InFlight) => {
                self.flying.push_back(Flying {
                    replies: Vec::new(),
                    writes,
                });
                return true;
            }
            Ok(Started::Done(r)) => r,
            Err(e) => writes.iter().map(|_| Err(clone_err(&e))).collect(),
        };
        for ((item, req), r) in writes.into_iter().zip(results) {
            fill(&mut replies[item], req, r);
        }
        false
    }

    /// Admits a read through the replica rules without performing it: for a caller that
    /// reads the store asynchronously itself ([`Self::plan_read`]). Returns where to read, or
    /// the reply to give instead.
    pub fn admit_read(&mut self, s: SessionRef, req: u64, chunk: u32, ext: Extent) -> Admit {
        let Some(uuid) = self.uuid_of(s.vol) else {
            return Admit::Reply(Reply::Close("unknown volume".into()));
        };
        let msg = ToReplica {
            vol: s.vol,
            epoch: s.epoch,
            session: s.session,
            req,
            body: Request::<()>::Read { chunk, ext },
        };
        match self.core.handle(&msg) {
            Action::Read { chunk, ext } => Admit::Read {
                key: key(uuid, chunk),
                offset: offset(ext),
                len: ext.len as usize * BLOCK,
                epoch: s.epoch,
            },
            Action::Reply(r) => Admit::Reply(Reply::Send(response_frame(req, r))),
            Action::Drop => Admit::Reply(Reply::Nothing),
            other => Admit::Reply(Reply::Close(format!(
                "unexpected action for a read: {other:?}"
            ))),
        }
    }

    /// Plans an admitted read into `buf` (see `Store::plan_read`).
    pub fn plan_read(
        &self,
        key: ChunkKey,
        offset: u64,
        buf: &mut [u8],
        epoch: u64,
    ) -> Result<flaminio_store::ReadPlan, StoreError> {
        self.store.plan_read(key, offset, buf, epoch)
    }

    /// Ends a planned read.
    pub fn finish_read(&self, plan: flaminio_store::ReadPlan) {
        self.store.finish_read(plan);
    }

    /// True if the volume of session `s` is still fenced at the session's epoch. A read
    /// admitted on `s` and performed later counts only if this holds after it: a newer target
    /// writes only to replicas it has fenced (spec §5, fast reads).
    pub fn still_fenced_at(&self, s: SessionRef) -> bool {
        self.fenced_epoch(s) == s.epoch
    }

    /// Deletes every chunk replica of the volume named `volume` from the store, and forgets the
    /// volume: for a volume deleted from the metadata service. Its sessions are not valid any
    /// more (their requests get "unknown volume"). Scans the store so chunks still survive a
    /// process restart that forgot `vols`. Returns how many replicas were deleted.
    pub fn delete_volume(&mut self, volume: &str) -> Result<usize, String> {
        let uuid = volume_uuid(volume);
        if let Some(v) = self.vols.remove(&uuid) {
            self.core.remove_volume(v.id);
        }
        // Epoch higher than any target's fence (docs/followups.md §8).
        let indices: Vec<u64> = self
            .store
            .chunks()
            .into_iter()
            .filter(|c| c.key.volume == uuid)
            .map(|c| c.key.index)
            .collect();
        for index in &indices {
            match self.store.delete_chunk(
                ChunkKey {
                    volume: uuid,
                    index: *index,
                },
                u64::MAX,
            ) {
                Ok(()) => {}
                Err(StoreError::NoSuchChunk(_)) => {}
                Err(e) => return Err(format!("delete chunk {index}: {e}")),
            }
        }
        Ok(indices.len())
    }

    /// The epoch the volume of session `s` is fenced at now.
    pub fn fenced_epoch(&self, s: SessionRef) -> u64 {
        self.core.fenced(s.vol)
    }

    /// The chunk size of a volume known here.
    pub fn chunk_size_of(&self, id: VolumeId) -> Option<u64> {
        self.vols
            .values()
            .find(|v| v.id == id)
            .map(|v| v.chunk_size)
    }

    fn uuid_of(&self, id: VolumeId) -> Option<Uuid> {
        self.vols.iter().find(|(_, v)| v.id == id).map(|(&u, _)| u)
    }
}

fn clone_err(e: &StoreError) -> StoreError {
    StoreError::Invalid(e.to_string())
}

/// A write's reply from its store result. A copy split in parts answers once, unless a part
/// failed.
fn fill(reply: &mut Option<Reply>, req: u64, r: Result<(), StoreError>) {
    match r {
        Ok(()) => {
            if reply.is_none() {
                *reply = Some(Reply::Send(Frame::Ok { req }));
            }
        }
        Err(e) => *reply = Some(Reply::Close(e.to_string())),
    }
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

fn len(ext: Extent) -> u64 {
    u64::from(ext.len) * BLOCK as u64
}

/// The shared payload of a write or a copy.
fn shared_payload(f: &Frame) -> &bytes::Bytes {
    match f {
        Frame::Write { data, .. } | Frame::Copy { data, .. } => data,
        _ => unreachable!("only writes and copies carry a payload to store"),
    }
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
        | Frame::Digest { req, .. }
        | Frame::Mapped { req, .. }
        | Frame::Discard { req, .. }
        | Frame::BeginResync { req, .. }
        | Frame::Settle { req, .. }
        | Frame::Copy { req, .. }
        | Frame::Fenced { req, .. }
        | Frame::Ok { req }
        | Frame::Data { req, .. }
        | Frame::Digests { req, .. }
        | Frame::Mappings { req, .. }
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
        Frame::Digest { req, chunk, ext } => (req, Request::Digest { chunk, ext }),
        Frame::Mapped { req, chunk, ext } => (req, Request::Mapped { chunk, ext }),
        Frame::Discard { req, chunk, ext } => (req, Request::Discard { chunk, ext }),
        Frame::BeginResync { req, chunk } => (req, Request::BeginResync { chunk }),
        Frame::Settle { req, chunk, below } => (req, Request::Settle { chunk, below }),
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
        Response::Digests(_) => unreachable!("digests are carried out by the service"),
        Response::Mapped(_) => unreachable!("mappings are carried out by the service"),
    }
}

#[cfg(test)]
mod tests;
