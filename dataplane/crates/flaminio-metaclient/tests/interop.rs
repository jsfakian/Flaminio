//! Against the Go metadata service: run with `make meta-interop`, which starts one and sets
//! `FLAMINIO_META_INTEROP` (its address) and `FLAMINIO_META_INTEROP_DIR` (certificates). The
//! script has registered fl-a and fl-b, created volume `v` (2 chunks, RF 2) and attached it to
//! fl-a. This test is fl-a's data plane.

use std::time::Duration;

use flaminio_metaclient::{Answer, Call, Config, call_blocking, pb};

fn config(endpoints: Vec<String>) -> Config {
    let dir = std::env::var("FLAMINIO_META_INTEROP_DIR").expect("set by make meta-interop");
    let read = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).unwrap();
    Config {
        endpoints,
        cert_pem: read("fl-a.crt"),
        key_pem: read("fl-a.key"),
        ca_pem: read("ca/ca.crt"),
        timeout: Duration::from_secs(3),
    }
}

#[test]
#[ignore = "needs a metadata service: make meta-interop"]
fn interop_with_the_go_service() {
    let addr = std::env::var("FLAMINIO_META_INTEROP").expect("set by make meta-interop");
    // Two dead endpoints first, the client moves on to the live one: one that never answers
    // (a partition: the connection is accepted by the kernel, then nothing) and one that refuses.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let cfg = config(vec![
        silent.local_addr().unwrap().to_string(),
        "127.0.0.1:1".into(),
        addr,
    ]);

    let Answer::GetVolume(r) = call_blocking(
        &cfg,
        Call::GetVolume(pb::GetVolumeRequest { id: "v".into() }),
    )
    .unwrap() else {
        panic!("GetVolume failed");
    };
    let v = r.volume.unwrap();
    // Attached once: the epoch after its first, the revision of its claim, which the in-memory
    // store starts at the clock (docs/issues/30).
    let epoch = v.epoch;
    assert!(epoch > 1, "epoch {epoch}");
    assert_eq!((v.target.as_str(), v.chunks.len()), ("fl-a", 2));
    assert_eq!(
        (v.size_bytes, v.chunk_bytes, v.replicas, v.min_isr),
        (2 << 20, 1 << 20, 2, 1)
    );
    let c0 = v.chunks[0].clone();
    assert_eq!(c0.isr.len(), 2);

    // As the target, shrink chunk 0's ISR.
    let change = |expected_ver: u64, new_isr: Vec<String>| {
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch,
            changes: vec![pb::IsrChange {
                chunk: 0,
                expected_ver,
                new_isr,
                ..Default::default()
            }],
        })
    };
    let Answer::ChangeIsr(r) =
        call_blocking(&cfg, change(0, vec![c0.replicas[0].clone()])).unwrap()
    else {
        panic!("ChangeIsr failed");
    };
    assert!(r.applied);
    assert_eq!(
        (r.epoch, r.chunks[0].isr_ver, r.chunks[0].isr.len()),
        (epoch, 1, 1)
    );
    // A retry is refused and shows the result.
    let Answer::ChangeIsr(r) =
        call_blocking(&cfg, change(0, vec![c0.replicas[0].clone()])).unwrap()
    else {
        panic!("ChangeIsr failed");
    };
    assert!(!r.applied);
    assert_eq!(r.chunks[0].isr_ver, 1);

    // fl-a heartbeats itself, and may not heartbeat fl-b.
    let hb = |node: &str| Call::NodeHeartbeat(pb::NodeHeartbeatRequest { node: node.into() });
    assert!(
        matches!(call_blocking(&cfg, hb("fl-a")).unwrap(), Answer::NodeHeartbeat(h) if h.ttl_seconds == 10)
    );
    assert!(matches!(
        call_blocking(&cfg, hb("fl-b")).unwrap(),
        Answer::Failed {
            code: tonic::Code::PermissionDenied,
            ..
        }
    ));
    let Answer::ListNodes(l) =
        call_blocking(&cfg, Call::ListNodes(pb::ListNodesRequest {})).unwrap()
    else {
        panic!("ListNodes failed");
    };
    let mut nodes: Vec<(String, String)> = l.nodes.into_iter().map(|n| (n.id, n.address)).collect();
    nodes.sort();
    assert_eq!(
        nodes,
        vec![
            ("fl-a".into(), "fl-a:7410".into()),
            ("fl-b".into(), "fl-b:7410".into())
        ]
    );
    eprintln!("interop: GetVolume, ChangeIsr (applied, then refused), heartbeats, ListNodes: ok");
}
