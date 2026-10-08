//! The target's view of its remote replica (MVP step 5): the connection, the units the replica
//! may be missing, and the background work that brings it back in sync.
//!
//! Writes go to the replica and the local store at the same time, and are acknowledged when both
//! are durable. When the replica is unreachable, the volume keeps serving from the local replica
//! (2-node rule, ADR 0006) and remembers every 64 KiB unit written meanwhile. In the background,
//! one small step at a time, it reconnects, compares checksums after a restart, and copies the
//! missing units. Each step runs between write batches on the same thread, and everything goes
//! over one ordered connection, so a resync copy can never overwrite a newer write.

use std::collections::BTreeSet;
use std::io::{self, BufReader, BufWriter};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::proto::{Msg, UNIT, WriteItem, read_msg, write_msg};
use crate::volume::VolumeLayout;

/// How often a lost replica is retried.
const RETRY: Duration = Duration::from_secs(2);
/// Units compared per scan step (16 MiB) and copied per resync step (1 MiB).
pub const SCAN_UNITS: u32 = 256;
pub const COPY_UNITS: usize = 16;

pub struct Conn {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl Conn {
    pub fn request(&mut self, msg: &Msg) -> io::Result<Msg> {
        self.send(msg)?;
        self.receive()
    }

    pub fn send(&mut self, msg: &Msg) -> io::Result<()> {
        write_msg(&mut self.writer, msg)
    }

    pub fn receive(&mut self) -> io::Result<Msg> {
        match read_msg(&mut self.reader)? {
            Msg::Error { message } => Err(io::Error::other(format!("replica: {message}"))),
            m => Ok(m),
        }
    }
}

/// Replica state, for status reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Connected, and holds everything the local replica holds.
    InSync,
    /// Connected, being brought up to date.
    Resyncing { dirty_units: usize, scanning: bool },
    /// Not reachable; writes are tracked for a later resync.
    Down { dirty_units: usize },
}

pub struct Remote {
    pub addr: String,
    timeout: Duration,
    pub conn: Option<Conn>,
    /// (chunk, unit) pairs the replica may be missing.
    pub dirty: BTreeSet<(u64, u64)>,
    /// Comparison in progress after a (re)start of the target: next (chunk, unit) to compare.
    pub scan: Option<(u64, u64)>,
    pub next_attempt: Instant,
    reported: Option<Health>,
}

impl Remote {
    pub fn new(addr: String, timeout: Duration) -> Self {
        Self {
            addr,
            timeout,
            conn: None,
            dirty: BTreeSet::new(),
            scan: Some((0, 0)),
            next_attempt: Instant::now(),
            reported: None,
        }
    }

    pub fn health(&self) -> Health {
        match (&self.conn, self.scan.is_some(), self.dirty.len()) {
            (None, _, d) => Health::Down { dirty_units: d },
            (Some(_), false, 0) => Health::InSync,
            (Some(_), scanning, d) => Health::Resyncing {
                dirty_units: d,
                scanning,
            },
        }
    }

    /// Logs state changes once (to the service log).
    pub fn report(&mut self) {
        let h = self.health();
        let changed = match (self.reported, h) {
            (Some(Health::Down { .. }), Health::Down { .. }) => false,
            (Some(Health::Resyncing { .. }), Health::Resyncing { .. }) => false,
            (prev, now) => prev != Some(now),
        };
        if changed {
            eprintln!("replica {}: {h:?}", self.addr);
            self.reported = Some(h);
        }
    }

    /// Connects and starts a session at `epoch`.
    pub fn connect(&mut self, layout: &VolumeLayout, epoch: u64) -> io::Result<()> {
        let addr = self
            .addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::other(format!("cannot resolve {}", self.addr)))?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        let mut conn = Conn {
            reader: BufReader::new(stream.try_clone()?),
            writer: BufWriter::new(stream),
        };
        let hello = Msg::Hello {
            volume: layout.volume,
            epoch,
            size: layout.size,
            chunk_size: layout.chunk_size,
        };
        match conn.request(&hello)? {
            Msg::Ok => {
                self.conn = Some(conn);
                Ok(())
            }
            other => Err(io::Error::other(format!(
                "unexpected reply to hello: {other:?}"
            ))),
        }
    }

    /// Drops the connection after a failure. Writes are tracked until it is back.
    pub fn fail(&mut self, why: &io::Error) {
        if self.conn.take().is_some() {
            eprintln!(
                "replica {}: {why}; serving from the local replica and tracking changes",
                self.addr
            );
        }
        self.next_attempt = Instant::now() + RETRY;
    }

    /// Records that `[offset, offset + len)` of `chunk` may be missing on the replica.
    pub fn mark_dirty(&mut self, chunk: u64, offset: u64, len: u64) {
        if len == 0 {
            return;
        }
        for unit in offset / UNIT..=(offset + len - 1) / UNIT {
            self.dirty.insert((chunk, unit));
        }
    }

    /// Writes as a [`Msg::Write`], given the items for each request piece.
    pub fn send_writes(&mut self, epoch: u64, items: Vec<WriteItem>) -> io::Result<()> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| io::Error::other("not connected"))?;
        conn.send(&Msg::Write { epoch, items })
    }

    /// The reply to [`send_writes`](Self::send_writes): Ok only if every item succeeded.
    pub fn finish_writes(&mut self, count: usize) -> io::Result<()> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| io::Error::other("not connected"))?;
        match conn.receive()? {
            Msg::WriteResult { statuses }
                if statuses.len() == count && statuses.iter().all(|&s| s == 0) =>
            {
                Ok(())
            }
            Msg::WriteResult { statuses } => Err(io::Error::other(format!(
                "replica rejected writes: {statuses:?}"
            ))),
            other => Err(io::Error::other(format!(
                "unexpected reply to write: {other:?}"
            ))),
        }
    }

    /// Forwards a discard.
    pub fn discard(&mut self, epoch: u64, chunk: u64, offset: u64, len: u64) -> io::Result<()> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| io::Error::other("not connected"))?;
        match conn.request(&Msg::Discard {
            epoch,
            chunk,
            offset,
            len,
        })? {
            Msg::Ok => Ok(()),
            other => Err(io::Error::other(format!(
                "unexpected reply to discard: {other:?}"
            ))),
        }
    }

    /// The replica's digests of `count` units of `chunk` from `first_unit`.
    pub fn digests(
        &mut self,
        epoch: u64,
        chunk: u64,
        first_unit: u64,
        count: u32,
    ) -> io::Result<Vec<u32>> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| io::Error::other("not connected"))?;
        match conn.request(&Msg::Digest {
            epoch,
            chunk,
            first_unit,
            count,
        })? {
            Msg::Digests { crcs } => Ok(crcs),
            other => Err(io::Error::other(format!(
                "unexpected reply to digest: {other:?}"
            ))),
        }
    }
}
