//! The replica service (MVP step 5): owns a store and applies requests from one volume target
//! at a time, in order. Replies to writes once they are durable.

use std::io::{self, BufReader, BufWriter};
use std::net::{TcpListener, TcpStream};

use flaminio_store::{BlockDevice, Store, StoreError, WriteReq};

use crate::proto::{Msg, read_msg, write_msg};
use crate::volume::{VolumeLayout, errno, unit_digests};

/// Serves sessions from `listener`, one at a time, forever.
pub fn serve<D: BlockDevice>(mut store: Store<D>, listener: TcpListener) -> ! {
    eprintln!(
        "replica: listening on {}",
        listener.local_addr().map_or("?".into(), |a| a.to_string())
    );
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let peer = stream.peer_addr().map_or("?".into(), |a| a.to_string());
                eprintln!("replica: session from {peer}");
                match session(&mut store, stream) {
                    Ok(()) => eprintln!("replica: session from {peer} closed"),
                    Err(e) => eprintln!("replica: session from {peer} ended: {e}"),
                }
            }
            Err(e) => eprintln!("replica: accept failed: {e}"),
        }
    }
    unreachable!("TcpListener::incoming never ends")
}

fn session<D: BlockDevice>(store: &mut Store<D>, stream: TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);
    let mut layout = None;
    loop {
        let msg = match read_msg(&mut reader) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        let reply = handle(store, &mut layout, msg);
        write_msg(&mut writer, &reply)?;
    }
}

fn error(e: impl std::fmt::Display) -> Msg {
    Msg::Error {
        message: e.to_string(),
    }
}

fn handle<D: BlockDevice>(
    store: &mut Store<D>,
    layout: &mut Option<VolumeLayout>,
    msg: Msg,
) -> Msg {
    match msg {
        Msg::Hello {
            volume,
            epoch,
            size,
            chunk_size,
        } => {
            let l = VolumeLayout {
                volume,
                size,
                chunk_size,
            };
            if chunk_size == 0 || size == 0 || !size.is_multiple_of(chunk_size) {
                return error("volume size must be a non-zero multiple of the chunk size");
            }
            for i in 0..l.chunks() {
                match store.chunk_info(l.key(i)) {
                    Some(info) if info.chunk_size != chunk_size => {
                        return error(format!("chunk {i} exists with size {}", info.chunk_size));
                    }
                    Some(_) => {}
                    None => {
                        if let Err(e) = store.create_chunk(l.key(i), chunk_size, size, epoch) {
                            return error(e);
                        }
                    }
                }
            }
            // Raise the fence: a target with an older epoch can no longer write here.
            if let Err(e) = store.fence(volume, epoch) {
                return error(e);
            }
            *layout = Some(l);
            Msg::Ok
        }
        Msg::Write { epoch, items } => {
            let Some(l) = layout else {
                return error("write before hello");
            };
            let reqs: Vec<WriteReq<'_>> = items
                .iter()
                .map(|it| WriteReq {
                    key: l.key(it.chunk),
                    offset: it.offset,
                    data: (&it.data).into(),
                    write_seq: it.write_seq,
                    epoch,
                })
                .collect();
            match store.write_batch(&reqs) {
                Ok(results) => Msg::WriteResult {
                    statuses: results
                        .iter()
                        .map(|r| r.as_ref().map_or_else(errno, |_| 0))
                        .collect(),
                },
                Err(e) => error(e),
            }
        }
        Msg::Discard {
            epoch,
            chunk,
            offset,
            len,
        } => {
            let Some(l) = layout else {
                return error("discard before hello");
            };
            match store.discard(l.key(chunk), offset, len, epoch) {
                Ok(()) => Msg::Ok,
                Err(e) => error(e),
            }
        }
        Msg::Digest {
            epoch,
            chunk,
            first_unit,
            count,
        } => {
            let Some(l) = layout else {
                return error("digest before hello");
            };
            if chunk >= l.chunks() {
                return error(StoreError::NoSuchChunk(l.key(chunk)));
            }
            match unit_digests(store, l.key(chunk), first_unit, count, epoch, l.chunk_size) {
                Ok(crcs) => Msg::Digests { crcs },
                Err(e) => error(e),
            }
        }
        other => error(format!("unexpected request {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::Duration;

    use flaminio_format::Uuid;
    use flaminio_format::consts::MIB;
    use flaminio_format::layout::LayoutParams;
    use flaminio_format::mkfs::FormatOptions;
    use flaminio_store::{MemDevice, Store, StoreConfig};

    use crate::remote::Health;
    use crate::volume::{Op, Request, Volume, VolumeLayout};

    const CHUNK: u64 = 4 * MIB;

    fn layout() -> VolumeLayout {
        VolumeLayout {
            volume: Uuid([5; 16]),
            size: 4 * CHUNK,
            chunk_size: CHUNK,
        }
    }

    fn store() -> Store<MemDevice> {
        let dev = MemDevice::new(256 * MIB);
        let opts = FormatOptions {
            disk_uuid: Uuid([1; 16]),
            cluster_uuid: Uuid::NIL,
            format_nonce: 9,
            created_unix_secs: 0,
            layout: LayoutParams {
                journal_size: Some(16 * MIB),
                ..Default::default()
            },
            label: String::new(),
        };
        Store::format(&dev, &opts).unwrap();
        Store::open(dev, StoreConfig::default()).unwrap()
    }

    /// A replica service on a loopback port, in its own thread (with its own store).
    fn start_replica() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || super::serve(store(), listener));
        addr
    }

    fn target(addr: &str, epoch: u64) -> Volume<MemDevice> {
        let mut v = Volume::open(store(), layout(), epoch).unwrap();
        v.attach_remote(addr.to_string(), Duration::from_secs(5));
        v
    }

    fn write(v: &mut Volume<MemDevice>, offset: u64, fill: u8, len: usize) {
        let mut b = vec![fill; len];
        let r = v.process(&mut [Request {
            op: Op::Write,
            offset,
            len: len as u64,
            buf: &mut b,
        }]);
        assert_eq!(r, vec![len as i32]);
    }

    fn settle(v: &mut Volume<MemDevice>) {
        for _ in 0..10_000 {
            v.maintain();
            if v.remote_health() == Some(Health::InSync) {
                return;
            }
        }
        panic!("replica never came in sync: {:?}", v.remote_health());
    }

    fn assert_same(v: &mut Volume<MemDevice>) {
        let local = v.digests().unwrap();
        let remote = v.remote_digests().unwrap();
        assert_eq!(local.len(), remote.len());
        for (c, (a, b)) in local.iter().zip(&remote).enumerate() {
            let diff = a.iter().zip(b).filter(|(x, y)| x != y).count();
            assert_eq!(diff, 0, "chunk {c}: {diff} units differ");
        }
    }

    #[test]
    fn writes_are_mirrored() {
        let addr = start_replica();
        let mut v = target(&addr, 1);
        settle(&mut v);
        write(&mut v, 0, 0x11, 64 * 1024);
        write(&mut v, CHUNK - 8192, 0x22, 16384); // crosses a chunk boundary
        write(&mut v, 3 * CHUNK + 4096, 0x33, 4096);
        assert_eq!(
            v.remote_health(),
            Some(Health::InSync),
            "no resync needed for mirrored writes"
        );
        assert_same(&mut v);
    }

    #[test]
    fn writes_continue_while_the_replica_is_down_and_are_resynced() {
        let addr = start_replica();
        let mut v = target(&addr, 1);
        settle(&mut v);
        write(&mut v, 0, 0x11, 64 * 1024);
        v.drop_remote_connection();
        // The volume keeps working on its local replica, and remembers what changed.
        write(&mut v, 4096, 0x44, 8192);
        write(&mut v, 2 * CHUNK, 0x55, 128 * 1024);
        assert!(
            matches!(v.remote_health(), Some(Health::Down { dirty_units: 3 })),
            "{:?}",
            v.remote_health()
        );
        settle(&mut v);
        assert_same(&mut v);
    }

    #[test]
    fn data_written_before_the_replica_was_attached_is_found_and_copied() {
        let addr = start_replica();
        let mut v = Volume::open(store(), layout(), 1).unwrap();
        write(&mut v, CHUNK + 65536, 0x66, 3 * 65536);
        write(&mut v, 3 * CHUNK, 0x77, 4096);
        v.attach_remote(addr, Duration::from_secs(5));
        settle(&mut v);
        assert_same(&mut v);
    }

    #[test]
    fn a_target_with_an_older_epoch_is_refused() {
        let addr = start_replica();
        {
            let mut v = target(&addr, 5);
            settle(&mut v);
        } // the session closes
        let mut old = target(&addr, 4);
        for _ in 0..3 {
            old.maintain();
        }
        assert!(
            matches!(old.remote_health(), Some(Health::Down { .. })),
            "{:?}",
            old.remote_health()
        );
    }
}
