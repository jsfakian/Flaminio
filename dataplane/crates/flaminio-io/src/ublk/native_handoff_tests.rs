//! Same schedules as the independent model, through actual mTLS Attach CAS, both signatures,
//! the daemon's socket handler and Engine. Only device publication/completion is a fake sink.
use super::*;
use crate::test_support::{Clock, signer, token_key};
use flaminio_format::{layout::LayoutParams, mkfs::FormatOptions};
use flaminio_metaclient::{Config, MetaClient};
use flaminio_net::tls::Files;
use flaminio_node::clock::{ClockDomain, ClockQualification};
use flaminio_node::engine::{Engine, EngineConfig};
use flaminio_node::net::{self, NetConfig, ServerSide};
use flaminio_node::replica::ReplicaService;
use flaminio_repl::sim::process_handoff::{Actor, Step, nonce_bytes, parse_schedules};
use flaminio_repl::types::Timing;
use flaminio_store::MemDevice;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;

use crate::test_support::b64;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn rawurl(bytes: &[u8]) -> String {
    b64(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}
fn make_engine(
    dir: &Path,
    node: &str,
    nonce: [u8; 32],
    image: Option<MemDevice>,
) -> (Arc<Shared<MemDevice>>, Arc<Clock>) {
    let clock = Arc::new(Clock::default());
    let domain = Arc::new(
        ClockDomain::new(clock.clone(), ClockQualification::new(1, 1, 1000).unwrap()).unwrap(),
    );
    let device = image.unwrap_or_else(|| {
        let d = MemDevice::new(256 << 20);
        Store::format(
            &d,
            &FormatOptions {
                disk_uuid: Uuid([83; 16]),
                cluster_uuid: Uuid::NIL,
                format_nonce: 1,
                created_unix_secs: 0,
                layout: LayoutParams {
                    journal_size: Some(16 << 20),
                    ..Default::default()
                },
                label: String::new(),
            },
        )
        .unwrap();
        d
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    let sink = Mutex::new(tx.clone());
    let files = Files {
        cert: dir.join(format!("{node}.crt")),
        key: dir.join(format!("{node}.key")),
        ca: dir.join("ca.crt"),
    };
    let net = net::spawn(
        NetConfig {
            clock: domain.clone(),
            trust_domain: "lab".into(),
            server: Some(ServerSide {
                listener,
                tls: files.server_config().unwrap(),
                token_key: token_key(),
            }),
            client: Some(files.client_config().unwrap()),
        },
        Arc::new(move |e| {
            let _ = sink.lock().unwrap().send(Input::Net(e));
        }),
    )
    .unwrap();
    let meta_sink = Mutex::new(tx.clone());
    let mut observer = domain.observer();
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let meta = MetaClient::spawn_guarded(
        Config {
            endpoints: vec![read("addr").trim().into()],
            cert_pem: read(&format!("{node}.crt")),
            key_pem: read(&format!("{node}.key")),
            ca_pem: read("ca.crt"),
            timeout: Duration::from_millis(500),
        },
        move |id, answer| {
            let _ = meta_sink.lock().unwrap().send(Input::Meta { id, answer });
        },
        move || {
            let Ok(sample) = observer.sample() else {
                return false;
            };
            observer.domain().admit(sample).is_ok()
        },
    )
    .unwrap();
    let engine = Engine::new(
        EngineConfig {
            node: node.into(),
            address,
            timing: Timing::default(),
            max_io: 64,
            heartbeat: None,
        },
        ReplicaService::new(Store::open(device, StoreConfig::default()).unwrap()),
        net,
        Box::new(meta),
        domain,
        nonce,
    );
    (
        Arc::new(Shared {
            engine: Mutex::new(engine),
            inputs: Mutex::new(rx),
            sender: tx,
            fd: -1,
            device: "memory".into(),
            staged: AtomicBool::new(false),
            live: Arc::new(AtomicBool::new(true)),
            waiters: Mutex::new(HashMap::new()),
            next_op: AtomicU64::new(1000),
            volume: String::new(),
            dev_id: -1,
            verify: true,
            poll: Duration::ZERO,
            defer_taskrun: false,
            wakes: vec![],
            holding: false,
            cluster_size: 65536,
            arrivals: Arc::new(AtomicUsize::new(0)),
        }),
        clock,
    )
}
fn pump(node: &Shared<MemDevice>) -> Vec<Output> {
    let mut e = node.engine.lock().unwrap();
    while let Ok(i) = node.inputs.lock().unwrap().try_recv() {
        e.handle(i);
    }
    e.turn();
    e.take_output()
}
fn socket_command(
    line: &str,
    node: &Arc<Shared<MemDevice>>,
    peer: &Arc<Shared<MemDevice>>,
    nonce: [u8; 32],
    attached: &Attached,
) -> String {
    let (mut client, server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (ctl, requests) = mpsc::channel();
    let clock = crate::control::ClockControl {
        domain: node.engine.lock().unwrap().clock_domain().clone(),
        nonce,
        key: token_key(),
    };
    let sender = node.sender.clone();
    let attached_clone = attached.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let worker = thread::spawn(move || {
        crate::control::serve(server, &ctl, &sender, &wake, &attached_clone, &clock)
    });
    writeln!(client, "{line}").unwrap();
    let mut pending = None;
    for _ in 0..1000 {
        if let Ok(Control::Attach {
            volume,
            proof,
            reply,
            ..
        }) = requests.try_recv()
        {
            node.engine.lock().unwrap().handle(Input::Attach {
                volume: volume.clone(),
                proof: proof.clone(),
            });
            pending = Some((volume, proof, reply));
        }
        let _ = pump(peer);
        for output in pump(node) {
            match output {
                Output::Attached { volume, .. } => {
                    let (v, proof, reply) = pending
                        .take()
                        .expect("attach output without socket request");
                    assert_eq!(v, volume);
                    let permit = node
                        .engine
                        .lock()
                        .unwrap()
                        .attachment_permit(&v, &proof)
                        .unwrap();
                    node.live.store(true, Ordering::SeqCst);
                    *attached.lock().unwrap() = Some(LiveDevice {
                        volume: v,
                        id: 83,
                        proof,
                        live: node.live.clone(),
                        engine: Arc::downgrade(&(node.clone() as Arc<dyn EngineAccess>)),
                    });
                    permit.consume();
                    reply.send("ok /dev/ublkb83".into()).unwrap();
                }
                Output::AttachFailed { why, .. } => {
                    if let Some((_, _, reply)) = pending.take() {
                        reply.send(format!("error: {why}")).unwrap();
                    }
                }
                _ => {}
            }
        }
        if worker.is_finished() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let mut answer = String::new();
    BufReader::new(&mut client).read_line(&mut answer).unwrap();
    worker.join().unwrap();
    answer.trim().into()
}
fn reconciled_io(
    node: &Shared<MemDevice>,
    peer: &Shared<MemDevice>,
    volume: &str,
    previously_written: bool,
) {
    node.engine.lock().unwrap().handle(Input::Io {
        volume: volume.into(),
        op: 9001,
        io: Io::Read {
            offset: 0,
            len: 4096,
        },
    });
    let mut read = false;
    for _ in 0..2000 {
        let _ = pump(peer);
        for output in pump(node) {
            if let Output::Done {
                op: 9001,
                result,
                permit,
            } = output
            {
                let bytes = result.unwrap().unwrap();
                assert_eq!(bytes, vec![if previously_written { 0x55 } else { 0 }; 4096]);
                permit.expect("read needs final permit").consume();
                read = true;
            }
        }
        if read {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        read,
        "ordinary full-ISR fencing/reconciliation never served read"
    );
    node.engine.lock().unwrap().handle(Input::Io {
        volume: volume.into(),
        op: 9002,
        io: Io::Write {
            offset: 0,
            data: vec![0x55; 4096].into(),
        },
    });
    let mut written = false;
    for _ in 0..2000 {
        let _ = pump(peer);
        for output in pump(node) {
            if let Output::Done {
                op: 9002,
                result,
                permit,
            } = output
            {
                assert!(result.is_ok());
                permit.expect("durable write needs permit").consume();
                written = true;
            }
        }
        if written {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(written, "write never became durable on ISR");
}

#[test]
#[ignore = "production mTLS/CAS fixture: scripts/dev/clock-handoff-interop.sh"]
fn native_handoff_fixture() {
    let dir = std::path::PathBuf::from(std::env::var("FLAMINIO_CLOCK_INTEROP_DIR").unwrap());
    let address = std::fs::read_to_string(dir.join("addr")).unwrap();
    let file = parse_schedules(include_str!(
        "../../../../../meta/testdata/lease-clock-handoff.json"
    ))
    .unwrap();
    let expected_steps: usize = file.schedules.iter().map(|s| s.steps.len()).sum();
    let mut executed = 0;
    for schedule in file.schedules {
        let volume = format!("clock-{}", schedule.name);
        let mut nonce = nonce_bytes(1);
        let (mut node, mut clock) = make_engine(&dir, "fl-a", nonce, None);
        let (peer, _) = make_engine(&dir, "fl-b", [83; 32], None);
        let attached: Attached = Arc::new(Mutex::new(None));
        let mut slots: HashMap<String, (Vec<u8>, Vec<u8>)> = HashMap::new();
        let mut previously_written = false;
        for step in schedule.steps {
            executed += 1;
            match step {
                Step::Challenge { nonce: n } => {
                    nonce = nonce_bytes(n);
                    drop(node);
                    (node, clock) = make_engine(&dir, "fl-a", nonce, None);
                    assert_eq!(
                        socket_command("attach-challenge", &node, &peer, nonce, &attached),
                        format!("ok {}", hex(&nonce))
                    );
                }
                Step::Restart { nonce: n } => {
                    let image = node.engine.lock().unwrap().device().crash(0);
                    *attached.lock().unwrap() = None;
                    drop(node);
                    nonce = nonce_bytes(n);
                    (node, clock) = make_engine(&dir, "fl-a", nonce, Some(image));
                }
                Step::ClockFault => {
                    clock.failed.store(true, Ordering::SeqCst);
                    let _ = pump(&node);
                    assert!(
                        !node
                            .engine
                            .lock()
                            .unwrap()
                            .clock_domain()
                            .status()
                            .is_healthy()
                    );
                    let status: serde_json::Value = serde_json::from_str(&socket_command(
                        "clock-status",
                        &node,
                        &peer,
                        nonce,
                        &attached,
                    ))
                    .unwrap();
                    assert_eq!(status["state"], "quarantined");
                    assert_eq!(status["generation"], 1);
                    assert_eq!(status["reason"], "sample_failed");
                    assert!(
                        socket_command("attach-challenge", &node, &peer, nonce, &attached)
                            .contains("operator restart required")
                    );
                    assert!(
                        socket_command(&format!("delete {volume}"), &node, &peer, nonce, &attached)
                            .contains("operator restart required")
                    );
                    assert!(
                        socket_command("status", &node, &peer, nonce, &attached).starts_with("ok ")
                    );
                }
                Step::MetadataAvailable { available } => {
                    std::fs::write(dir.join("available"), if available { "1" } else { "0" })
                        .unwrap()
                }
                Step::AuthorizedAttach {
                    actor,
                    only_if_target,
                    unless_target,
                    nonce: n,
                    slot,
                    expect_issued,
                    expect_epoch,
                    expect_target,
                    expect_mutation,
                } => {
                    let actor = match actor {
                        Actor::Admin => "admin",
                        Actor::OwnAgent => "own_agent",
                        Actor::OtherAgent => "other_agent",
                        Actor::Io => "io",
                        Actor::Node => "node",
                        Actor::Reader => "reader",
                        Actor::Anonymous => "anonymous",
                    };
                    let before: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap())
                            .unwrap();
                    let request = serde_json::json!({"volume":volume,"node":"fl-a","only_if_target":only_if_target,"unless_target":unless_target,"target_nonce":b64(&nonce_bytes(n))});
                    let out = Command::new(dir.join("fixture"))
                        .args([
                            "--dir",
                            dir.to_str().unwrap(),
                            "--addr",
                            address.trim(),
                            "--actor",
                            actor,
                            "--call",
                            &request.to_string(),
                        ])
                        .output()
                        .unwrap();
                    assert_eq!(
                        out.status.success(),
                        expect_issued,
                        "{} actor{actor}: {}",
                        schedule.name,
                        String::from_utf8_lossy(&out.stderr)
                    );
                    if expect_issued {
                        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
                        let token =
                            crate::node::base64_decode(v["token"].as_str().unwrap()).unwrap();
                        let handoff =
                            crate::node::base64_decode(v["targetHandoff"].as_str().unwrap())
                                .unwrap();
                        assert_eq!(handoff.len(), 134);
                        assert!(
                            token_key()
                                .verify_handoff(
                                    &token,
                                    &handoff,
                                    &nonce_bytes(n),
                                    flaminio_net::handoff::Purpose::Normal
                                )
                                .is_ok()
                        );
                        slots.insert(slot, (token, handoff));
                    }
                    let after: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap())
                            .unwrap();
                    assert_eq!(after[&volume]["epoch"].as_u64(), Some(expect_epoch));
                    assert_eq!(
                        after[&volume]["target"],
                        if expect_target == 0 { "fl-a" } else { "fl-b" }
                    );
                    assert_eq!(before[&volume] != after[&volume], expect_mutation);
                }
                Step::TargetStart {
                    slot,
                    expect_started,
                } => {
                    let (token, handoff) = slots.get(&slot).unwrap();
                    let before = node.engine.lock().unwrap().service().store().stats().chunks;
                    let answer = socket_command(
                        &format!("attach {volume} {} {} 1000", b64(token), rawurl(handoff)),
                        &node,
                        &peer,
                        nonce,
                        &attached,
                    );
                    assert_eq!(
                        answer.starts_with("ok /dev/ublkb83"),
                        expect_started,
                        "{}: {answer}",
                        schedule.name
                    );
                    if !expect_started {
                        assert_eq!(
                            node.engine.lock().unwrap().service().store().stats().chunks,
                            before
                        );
                    } else {
                        reconciled_io(&node, &peer, &volume, previously_written);
                        previously_written = true;
                    }
                    if !schedule.local_replica {
                        assert!(
                            node.engine
                                .lock()
                                .unwrap()
                                .service()
                                .store()
                                .chunks()
                                .is_empty()
                        );
                    }
                }
                Step::StopTarget => {
                    node.engine.lock().unwrap().handle(Input::Detach {
                        volume: volume.clone(),
                    });
                    *attached.lock().unwrap() = None;
                }
                Step::TamperCapability { slot, field, value } => {
                    let (token, _) = slots.get_mut(&slot).unwrap();
                    let mut claims = token_key().verify(token).unwrap();
                    match field.as_str() {
                        "volume" => claims.volume = format!("clock-other{value}"),
                        "node" => claims.node = "fl-b".into(),
                        "epoch" => claims.epoch = value,
                        "incarnation" => claims.first_epoch = value,
                        _ => panic!("unknown fixture field"),
                    };
                    let (t, _) = crate::test_support::credentials(
                        &claims.volume,
                        &claims.node,
                        claims.epoch,
                        claims.first_epoch,
                        &nonce,
                    );
                    *token = t;
                }
                Step::TamperProof { slot, field, value } => {
                    let (_, handoff) = slots.get_mut(&slot).unwrap();
                    match field.as_str() {
                        "signature" => {
                            handoff[133] ^= 1;
                            continue;
                        }
                        "nonce" => handoff[6..38].copy_from_slice(&nonce_bytes(value)),
                        "purpose" => handoff[5] = value as u8,
                        "digest" => handoff[38] ^= 1,
                        _ => panic!("unknown fixture field"),
                    };
                    let signature = signer().sign(&handoff[..70]);
                    handoff[70..].copy_from_slice(signature.as_ref());
                }
            }
        }
        std::fs::write(dir.join("available"), "1").unwrap();
    }
    // The actual administrator CLI places its nonce flag before attach, and retains the
    // exact new proof while a live older lifetime retires by normal metadata deposition.
    let volume = "clock-same_epoch_issuance_before_any_start";
    let nonce = nonce_bytes(303);
    let (node, clock) = make_engine(&dir, "fl-a", nonce, None);
    let (peer, peer_clock) = make_engine(&dir, "fl-b", [83; 32], None);
    let attached: Attached = Arc::new(Mutex::new(None));
    let issue = || {
        let output = Command::new(dir.join("flaminio-meta"))
            .args([
                "--server",
                address.trim(),
                "--cert",
                dir.join("own_agent.crt").to_str().unwrap(),
                "--key",
                dir.join("own_agent.key").to_str().unwrap(),
                "--ca",
                dir.join("ca.crt").to_str().unwrap(),
                "--target-nonce",
                &hex(&nonce),
                "attach",
                volume,
                "fl-a",
                "only-if-target",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        (
            crate::node::base64_decode(value["token"].as_str().unwrap()).unwrap(),
            crate::node::base64_decode(value["targetHandoff"].as_str().unwrap()).unwrap(),
        )
    };
    let (first_token, first_handoff) = issue();
    assert!(
        socket_command(
            &format!(
                "attach {volume} {} {} 1000",
                b64(&first_token),
                rawurl(&first_handoff)
            ),
            &node,
            &peer,
            nonce,
            &attached
        )
        .starts_with("ok ")
    );
    reconciled_io(&node, &peer, volume, false);
    std::fs::write(dir.join("available"), "0").unwrap();
    reconciled_io(&node, &peer, volume, true);
    std::fs::write(dir.join("available"), "1").unwrap();
    let (retained_token, retained_handoff) = issue();
    let retained_command = format!(
        "attach {volume} {} {} 1000",
        b64(&retained_token),
        rawurl(&retained_handoff)
    );
    assert!(
        socket_command(&retained_command, &node, &peer, nonce, &attached)
            .contains("existing device lifetime")
    );
    // A busy response cannot return/adopt the older device for a newer epoch.
    assert_eq!(
        attached
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .proof
            .capability(),
        first_token
    );
    clock.ns.store(3_000_000_000, Ordering::SeqCst);
    peer_clock.ns.store(3_000_000_000, Ordering::SeqCst);
    let mut deposed = false;
    for _ in 0..1000 {
        let _ = pump(&peer);
        for output in pump(&node) {
            if matches!(output, Output::Deposed { .. }) {
                deposed = true;
            }
        }
        if deposed {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(deposed, "higher metadata epoch did not depose old target");
    node.live.store(false, Ordering::SeqCst);
    *attached.lock().unwrap() = None; // fake device stop/drain sink
    assert!(socket_command(&retained_command, &node, &peer, nonce, &attached).starts_with("ok "));
    let before = std::fs::read(dir.join("state.json")).unwrap();
    let refused = Command::new(dir.join("flaminio-meta"))
        .args([
            "--target-nonce",
            &"A".repeat(64),
            "attach",
            volume,
            "fl-a",
            "only-if-target",
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(dir.join("state.json")).unwrap(), before);
    assert_eq!(
        executed, expected_steps,
        "every fixture action must execute"
    );
    eprintln!("native handoff replay: {executed} steps, real mTLS/CAS/FLCT/FLCH/control/Engine");
}

async fn bounded_closure<F: std::future::Future>(future: F) -> F::Output {
    futures_lite::future::or(future, async {
        smol::Timer::after(Duration::from_secs(2)).await;
        panic!("quarantine did not close the live network session");
    })
    .await
}
fn assert_closed(result: std::io::Result<usize>) {
    match result {
        Ok(0) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::NotConnected
            ) => {}
        other => panic!("expected EOF/reset, not a timeout or data: {other:?}"),
    }
}
fn assert_listener_closed(addr: std::net::SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)) {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return,
            // Shutdown can race a connect already queued in the listener backlog.
            // These outcomes are not closure evidence; retry until refusal or the bound.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(e) => panic!("listener closure was not proven: {e}"),
            Ok(stream) => drop(stream),
        }
        assert!(
            Instant::now() < deadline,
            "quarantined listener still accepts connections"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "isolated loopback fixture: scripts/dev/clock-handoff-interop.sh"]
fn native_handoff_fixture_cancels_idle_sessions_writers_and_metadata() {
    use flaminio_net::{frame::Frame, session};
    use flaminio_node::net::{NetCommand, NetEvent};
    use flaminio_node::replica::Reply;
    use futures_rustls::TlsConnector;
    use std::io::Read;
    let dir = std::path::PathBuf::from(std::env::var("FLAMINIO_CLOCK_INTEROP_DIR").unwrap());
    let files = |node: &str| Files {
        cert: dir.join(format!("{node}.crt")),
        key: dir.join(format!("{node}.key")),
        ca: dir.join("ca.crt"),
    };
    // A TCP peer stalled before TLS and an authenticated peer stalled before Hello both close.
    for authenticated in [false, true] {
        let clock = Arc::new(Clock::default());
        let domain = Arc::new(
            ClockDomain::new(clock, ClockQualification::new(1, 1, 1000).unwrap()).unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let n = net::spawn(
            NetConfig {
                clock: domain.clone(),
                trust_domain: "lab".into(),
                server: Some(ServerSide {
                    listener,
                    tls: files("fl-b").server_config().unwrap(),
                    token_key: token_key(),
                }),
                client: None,
            },
            Arc::new(|_| panic!("no Hello/session may be admitted")),
        )
        .unwrap();
        if authenticated {
            let mut stream = smol::block_on(async {
                TlsConnector::from(files("fl-a").client_config().unwrap())
                    .connect(
                        addr.ip().into(),
                        smol::Async::<std::net::TcpStream>::connect(addr)
                            .await
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            });
            domain.quarantine(flaminio_node::clock::ClockFault::SampleFailed);
            use futures_lite::io::AsyncReadExt;
            assert_closed(smol::block_on(bounded_closure(stream.read(&mut [0; 1]))));
        } else {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            domain.quarantine(flaminio_node::clock::ClockFault::SampleFailed);
            assert_closed(stream.read(&mut [0; 1]));
        }
        assert_listener_closed(addr);
        drop(n);
    }
    // Both accepted writer queues and outgoing sessions are cancelled, including an idle TLS handshake.
    let clock = Arc::new(Clock::default());
    let domain =
        Arc::new(ClockDomain::new(clock, ClockQualification::new(1, 1, 1000).unwrap()).unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let n = net::spawn(
        NetConfig {
            clock: domain.clone(),
            trust_domain: "lab".into(),
            server: Some(ServerSide {
                listener,
                tls: files("fl-b").server_config().unwrap(),
                token_key: token_key(),
            }),
            client: None,
        },
        Arc::new(move |e| {
            let _ = tx.send(e);
        }),
    )
    .unwrap();
    let (token, _) = crate::test_support::credentials("v", "fl-a", 5, 1, &[83; 32]);
    let hello = Frame::Hello {
        req: 1,
        epoch: 5,
        session: 1,
        volume: "v".into(),
        token,
        size: 1 << 20,
        chunk_size: 1 << 20,
        chunks: vec![0],
    };
    let mut client = smol::block_on(async {
        session::connect(
            smol::Async::<std::net::TcpStream>::connect(addr)
                .await
                .unwrap(),
            &TlsConnector::from(files("fl-a").client_config().unwrap()),
            addr.ip(),
            "lab",
            "fl-b",
            &hello,
        )
        .await
        .unwrap()
    });
    let replies = match rx.recv_timeout(Duration::from_secs(1)).unwrap() {
        NetEvent::Opened { replies, .. } => replies,
        _ => panic!("expected authenticated session"),
    };
    replies.try_send(Reply::Send(Frame::Ok { req: 1 })).unwrap();
    domain.quarantine(flaminio_node::clock::ClockFault::SampleFailed);
    let _ = replies.try_send(Reply::Send(Frame::Ok { req: 2 }));
    let mut received = 0;
    loop {
        match smol::block_on(bounded_closure(client.recv())) {
            Ok(frame) => {
                assert!(matches!(frame, Frame::Ok { req: 1 }));
                received += 1;
                assert!(received <= 1, "postfault writer admitted queued frame");
            }
            Err(session::SessionError::Io(e)) => {
                assert_closed(Err(e));
                break;
            }
            Err(e) => panic!("session did not close cleanly: {e}"),
        }
    }
    assert_listener_closed(addr);
    drop(n);
    let clock = Arc::new(Clock::default());
    let domain =
        Arc::new(ClockDomain::new(clock, ClockQualification::new(1, 1, 1000).unwrap()).unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let n = net::spawn(
        NetConfig {
            clock: domain.clone(),
            trust_domain: "lab".into(),
            server: None,
            client: Some(files("fl-a").client_config().unwrap()),
        },
        Arc::new(|_| {}),
    )
    .unwrap();
    let generation = domain.admit(domain.observer().sample().unwrap()).unwrap();
    n.send(NetCommand::Connect {
        generation,
        key: 1,
        addr,
        node: "fl-b".into(),
        hello: Box::new(hello),
    });
    let (mut raw, _) = listener.accept().unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    domain.quarantine(flaminio_node::clock::ClockFault::SampleFailed);
    loop {
        match raw.read(&mut [0; 4096]) {
            Ok(0) => break,
            Err(e) => {
                assert_closed(Err(e));
                break;
            }
            Ok(_) => {}
        }
    }
    drop(n);
    // Persistent worker guard stops endpoint fallback, a queued second request and late callbacks.
    for fallback in [false, true] {
        std::fs::write(dir.join("available"), if fallback { "0" } else { "1" }).unwrap();
        let spare = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        spare.set_nonblocking(true).unwrap();
        let clock = Arc::new(Clock::default());
        let domain = Arc::new(
            ClockDomain::new(clock.clone(), ClockQualification::new(1, 1, 1000).unwrap()).unwrap(),
        );
        let mut observer = domain.observer();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let (tx, answers) = mpsc::channel();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        let worker = MetaClient::spawn_guarded(
            Config {
                endpoints: vec![
                    read("addr").trim().into(),
                    spare.local_addr().unwrap().to_string(),
                ],
                cert_pem: read("fl-a.crt"),
                key_pem: read("fl-a.key"),
                ca_pem: read("ca.crt"),
                timeout: Duration::from_millis(500),
            },
            move |id, answer| {
                tx.send((id, answer)).unwrap();
            },
            move || {
                if count.fetch_add(1, Ordering::SeqCst) == 2 {
                    clock.failed.store(true, Ordering::SeqCst);
                }
                let Ok(sample) = observer.sample() else {
                    return false;
                };
                observer.domain().admit(sample).is_ok()
            },
        )
        .unwrap();
        for id in [1, 2] {
            worker.call(
                id,
                flaminio_metaclient::Call::GetVolume(flaminio_metaclient::pb::GetVolumeRequest {
                    id: "clock-fresh_epoch_recovery_and_live_retry".into(),
                }),
            );
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while attempts.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(attempts.load(Ordering::SeqCst) >= 3);
        assert!(!domain.status().is_healthy());
        assert!(answers.try_recv().is_err());
        assert!(
            spare.accept().is_err(),
            "quarantined endpoint fallback contacted another server"
        );
    }
    std::fs::write(dir.join("available"), "1").unwrap();
}
