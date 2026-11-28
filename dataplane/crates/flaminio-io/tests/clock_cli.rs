//! Real CLI refuses before opening storage, formatting, binding or metadata contact.
use std::process::Command;
#[test]
fn refused_startup_has_no_storage_or_socket_effects() {
    let dir = std::env::temp_dir().join(format!("fl-clock-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let device = dir.join("disk");
    let socket = dir.join("node.sock");
    std::fs::write(&device, b"unchanged storage sentinel").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_flaminio-io"))
        .args([
            "node",
            "--node",
            "fl-a",
            "--device",
            device.to_str().unwrap(),
            "--format",
            "--force",
            "--listen",
            "127.0.0.1:0",
            "--meta",
            "127.0.0.1:1",
            "--trust-domain",
            "test",
            "--clock-contract",
            dir.join("absent").to_str().unwrap(),
            "--control",
            socket.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("clock contract"));
    assert_eq!(
        std::fs::read(&device).unwrap(),
        b"unchanged storage sentinel"
    );
    assert!(!socket.exists());
    for mode in ["serve", "replica"] {
        let out = Command::new(env!("CARGO_BIN_EXE_flaminio-io"))
            .args([
                mode,
                "--device",
                device.to_str().unwrap(),
                "--size",
                "1M",
                "--format",
                "--force",
            ])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert_eq!(
            std::fs::read(&device).unwrap(),
            b"unchanged storage sentinel"
        );
    }
    let out = Command::new(env!("CARGO_BIN_EXE_flaminio-io"))
        .args([
            "clock-check",
            "--clock-contract",
            dir.join("absent").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["decision"], "refuse");
    assert_eq!(json["attestation"], false);
    assert_eq!(json["profile"], "physical-boottime-v1");
    assert_eq!(json["source"], "CLOCK_BOOTTIME");
    assert!(!json["excluded_events"].as_array().unwrap().is_empty());
    assert!(!socket.exists());
    std::fs::remove_dir_all(dir).unwrap();
}
