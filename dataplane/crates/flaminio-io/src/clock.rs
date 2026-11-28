//! Admission of the fixed physical host profile. Probes establish prerequisites, never
//! measure or certify the operator's future rate and rollback obligations.

use std::fs::File;
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::Arc;

use flaminio_node::clock::{
    ClockDomain, ClockFault, ClockQualification, ClockReading, ElapsedClock,
};
use serde::{Deserialize, Serialize};

pub const PROFILE: &str = "physical-boottime-v1";
pub const SOURCE: &str = "CLOCK_BOOTTIME";
const CONTRACT_LIMIT: u64 = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    version: u32,
    profile: String,
    node: String,
    host_kind: String,
    clocksource: String,
    lease_us: u64,
    drift_ppm: u64,
    rate_bound_attested: bool,
    observation_error_ns: u64,
    no_live_rollback: bool,
}

/// Pure bounded schema decoder for untrusted contract bytes. It supplies no deployment
/// admission: production still checks ownership, modes, provenance, timing and syscalls.
pub fn decode_contract(bytes: &[u8]) -> Result<Contract, String> {
    if bytes.len() > CONTRACT_LIMIT as usize {
        return Err("clock contract too large".into());
    }
    serde_json::from_slice(bytes).map_err(|_| "invalid clock contract schema".into())
}

#[derive(Serialize)]
pub struct Inspection {
    profile: &'static str,
    source: &'static str,
    lease_us: u64,
    drift_ppm: u64,
    attestation: bool,
    observed_prerequisites: Vec<&'static str>,
    excluded_events: [&'static str; 4],
    decision: &'static str,
    reason: String,
}

impl Inspection {
    fn new() -> Self {
        Self {
            profile: PROFILE,
            source: SOURCE,
            lease_us: 5_000_000,
            drift_ppm: 100_000,
            attestation: false,
            observed_prerequisites: Vec::new(),
            excluded_events: [
                "virtual_clock_stop",
                "unqualified_live_migration",
                "running_memory_or_disk_rollback",
                "unknown_host_provenance",
            ],
            decision: "refuse",
            reason: String::new(),
        }
    }
}

pub struct Admission {
    pub domain: Arc<ClockDomain>,
    pub nonce: [u8; 32],
}

struct FileFacts {
    regular: bool,
    uid: u32,
    mode: u32,
    len: u64,
}

fn validate_file(f: FileFacts) -> Result<(), String> {
    if !f.regular || f.uid != 0 || f.mode & 0o022 != 0 || f.len > CONTRACT_LIMIT {
        return Err("clock contract must be a root-owned regular file, at most 4096 bytes, without group/world write permission".into());
    }
    Ok(())
}

fn read_contract(path: &Path) -> Result<Contract, String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| format!("clock contract: {e}"))?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("clock contract metadata: {e}"))?;
        validate_file(FileFacts {
            regular: metadata.is_file(),
            uid: metadata.uid(),
            mode: metadata.mode(),
            len: metadata.len(),
        })?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(CONTRACT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("clock contract read: {e}"))?;
        if bytes.len() > CONTRACT_LIMIT as usize {
            return Err("clock contract too large".into());
        }
        // Derive rejects duplicate and unknown fields; from_slice also rejects trailing JSON.
        decode_contract(&bytes)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("unsupported clock probe platform".into())
    }
}

#[cfg(target_os = "linux")]
fn bounded_text(path: &Path) -> Result<String, String> {
    let mut text = String::new();
    let file = File::open(path).map_err(|_| "unreadable host provenance or clocksource")?;
    #[cfg(target_os = "linux")]
    if path.starts_with("/run/flaminio/host")
        && !flaminio_uring::clock::is_readonly(&file)
            .map_err(|_| "cannot inspect host provenance mount")?
    {
        return Err("each host provenance input must be on a read-only mount".into());
    }
    file.take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|_| "unreadable host provenance or clocksource".to_string())?;
    if text.len() > 1_048_576 || text.trim().is_empty() {
        return Err("unknown host provenance or clocksource".into());
    }
    Ok(text)
}

#[cfg(target_os = "linux")]
fn provenance_root() -> Result<PathBuf, String> {
    let cgroup = bounded_text(Path::new("/proc/self/cgroup"))?;
    let container = Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || ["docker", "kubepods", "containerd", "lxc"]
            .iter()
            .any(|s| cgroup.contains(s));
    if !container {
        return Ok(PathBuf::from("/"));
    }
    let root = PathBuf::from("/run/flaminio/host");
    Ok(root)
}

struct HostEvidence {
    cpu: Result<String, String>,
    vendor: Result<String, String>,
    product: Result<String, String>,
    hypervisor: Result<bool, String>,
    source: Result<String, String>,
    boot_resolution: Result<u64, String>,
    active_resolution: Result<u64, String>,
    reading: Result<ClockReading, String>,
}

#[cfg(target_os = "linux")]
fn collect_evidence() -> Result<HostEvidence, String> {
    let root = provenance_root()?;
    let resolution = |source| -> Result<u64, String> {
        let (sec, ns) =
            flaminio_uring::clock::elapsed_resolution(source).map_err(|_| "clock_getres failed")?;
        let ns = flaminio_node::clock::checked_clock_ns(sec, ns).map_err(|e| e.to_string())?;
        u64::try_from(ns).map_err(|_| "clock resolution overflow".into())
    };
    Ok(HostEvidence {
        cpu: bounded_text(&root.join("proc/cpuinfo")),
        vendor: bounded_text(&root.join("sys/class/dmi/id/sys_vendor")),
        product: bounded_text(&root.join("sys/class/dmi/id/product_name")),
        hypervisor: root
            .join("sys/hypervisor/type")
            .try_exists()
            .map_err(|_| "unknown hypervisor provenance".into()),
        source: bounded_text(
            &root.join("sys/devices/system/clocksource/clocksource0/current_clocksource"),
        ),
        boot_resolution: resolution(flaminio_uring::clock::ElapsedSource::Boottime),
        active_resolution: resolution(flaminio_uring::clock::ElapsedSource::Monotonic),
        reading: LinuxClock.read().map_err(|e| e.to_string()),
    })
}

fn qualify_evidence(
    contract: &Contract,
    evidence: &HostEvidence,
    observed: &mut Vec<&'static str>,
) -> Result<ClockQualification, String> {
    let cpu = evidence
        .cpu
        .as_ref()
        .map_err(Clone::clone)?
        .to_ascii_lowercase();
    let vendor = evidence
        .vendor
        .as_ref()
        .map_err(Clone::clone)?
        .to_ascii_lowercase();
    let product = evidence
        .product
        .as_ref()
        .map_err(Clone::clone)?
        .to_ascii_lowercase();
    let virtual_markers = [
        "qemu",
        "kvm",
        "vmware",
        "virtual",
        "xen",
        "bochs",
        "bhyve",
        "parallels",
        "openstack",
        "hvm domu",
        "amazon ec2",
        "google compute",
    ];
    if cpu.split_whitespace().any(|s| s == "hypervisor")
        || *evidence.hypervisor.as_ref().map_err(Clone::clone)?
        || virtual_markers
            .iter()
            .any(|s| vendor.contains(s) || product.contains(s))
    {
        return Err("detected virtualization; physical-boottime-v1 refuses this host".into());
    }
    if [
        "unknown",
        "default string",
        "not specified",
        "to be filled",
        "none",
    ]
    .iter()
    .any(|s| vendor.contains(s) || product.contains(s))
    {
        return Err("unknown physical host provenance".into());
    }
    observed.push("physical_host_provenance");
    let source = evidence.source.as_ref().map_err(Clone::clone)?;
    if source.trim() != contract.clocksource
        || contract.clocksource.is_empty()
        || contract.clocksource.len() > 128
    {
        return Err("qualified clocksource does not match the host".into());
    }
    observed.push("matching_clocksource");
    let q = ClockQualification::new(
        *evidence.boot_resolution.as_ref().map_err(Clone::clone)?,
        *evidence.active_resolution.as_ref().map_err(Clone::clone)?,
        contract.observation_error_ns,
    )
    .map_err(|e| e.to_string())?;
    observed.push("boottime_and_monotonic_resolution_at_most_1us");
    flaminio_node::clock::ClockGuard::new(*evidence.reading.as_ref().map_err(Clone::clone)?, q)
        .map_err(|e| e.to_string())?;
    observed.push("ordered_clock_reads");
    Ok(q)
}

#[cfg(target_os = "linux")]
fn qualify(
    contract: &Contract,
    observed: &mut Vec<&'static str>,
) -> Result<ClockQualification, String> {
    qualify_evidence(contract, &collect_evidence()?, observed)
}

#[cfg(not(target_os = "linux"))]
fn qualify(_: &Contract, _: &mut Vec<&'static str>) -> Result<ClockQualification, String> {
    Err("unsupported clock probe platform".into())
}

pub struct LinuxClock;
impl ElapsedClock for LinuxClock {
    fn read(&self) -> Result<ClockReading, ClockFault> {
        #[cfg(target_os = "linux")]
        {
            use flaminio_uring::clock::ElapsedSource::{Boottime, Monotonic};
            fn read(source: flaminio_uring::clock::ElapsedSource) -> Result<u128, ClockFault> {
                let (sec, ns) = flaminio_uring::clock::elapsed_time(source)
                    .map_err(|_| ClockFault::SampleFailed)?;
                flaminio_node::clock::checked_clock_ns(sec, ns)
            }
            Ok(ClockReading {
                monotonic_before_ns: read(Monotonic)?,
                boottime_ns: read(Boottime)?,
                monotonic_after_ns: read(Monotonic)?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(ClockFault::Profile)
        }
    }
}

fn validate_contract(c: &Contract, node: Option<&str>) -> Result<(), String> {
    let valid_node = !c.node.is_empty()
        && c.node.len() <= 128
        && c.node.as_bytes()[0].is_ascii_alphanumeric()
        && c.node
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if c.version != 1
        || c.profile != PROFILE
        || c.host_kind != "physical"
        || c.clocksource.is_empty()
        || c.clocksource.len() > 128
        || !valid_node
        || node.is_some_and(|n| n != c.node)
        || c.lease_us != 5_000_000
        || c.drift_ppm != 100_000
        || c.observation_error_ns != 1_000
        || !c.rate_bound_attested
        || !c.no_live_rollback
    {
        return Err("clock contract requires the fixed physical profile, matching node, qualified error and explicit rate/rollback attestation".into());
    }
    Ok(())
}

fn inspect(path: &Path, node: Option<&str>) -> (Inspection, Option<ClockQualification>) {
    let mut result = Inspection::new();
    let checked = (|| {
        let c = read_contract(path)?;
        validate_contract(&c, node)?;
        result.attestation = true;
        result.observed_prerequisites.push("operator_contract");
        result
            .observed_prerequisites
            .push("profile_supports_process_scheduling_pause_and_qualified_physical_suspend");
        qualify(&c, &mut result.observed_prerequisites)
    })();
    match checked {
        Ok(q) => {
            result.decision = "admit";
            result.reason =
                "prerequisites observed; rate and rollback safety are operator attestations".into();
            (result, Some(q))
        }
        Err(e) => {
            result.reason = e;
            (result, None)
        }
    }
}

fn process_nonce() -> Result<[u8; 32], String> {
    #[cfg(target_os = "linux")]
    {
        flaminio_uring::clock::process_nonce().map_err(|_| "cannot obtain a complete nonzero process challenge from initialized Linux getrandom".into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("unsupported OS process challenge source".into())
    }
}

pub fn admit(path: &Path, node: &str) -> Result<Admission, String> {
    let (inspection, qualification) = inspect(path, Some(node));
    let qualification = qualification.ok_or(inspection.reason)?;
    let domain =
        Arc::new(ClockDomain::new(Arc::new(LinuxClock), qualification).map_err(|e| e.to_string())?);
    let nonce = process_nonce()?;
    Ok(Admission { domain, nonce })
}

pub fn main(args: &[String]) -> Result<(), String> {
    let [flag, path, json] = args else {
        return Err("usage: flaminio-io clock-check --clock-contract PATH --json".into());
    };
    if flag != "--clock-contract" || json != "--json" {
        return Err("usage: flaminio-io clock-check --clock-contract PATH --json".into());
    }
    let (inspection, qualified) = inspect(Path::new(path), None);
    println!(
        "{}",
        serde_json::to_string(&inspection).map_err(|e| e.to_string())?
    );
    if qualified.is_some() {
        Ok(())
    } else {
        Err("clock profile refused".into())
    }
}

pub fn status(domain: &ClockDomain) -> String {
    let status = domain.status();
    serde_json::json!({"profile": PROFILE, "source": SOURCE, "state": if status.is_healthy() { "healthy" } else { "quarantined" }, "generation": status.generation.number(), "reason": status.reason.map(ClockFault::as_str).unwrap_or("none")}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = r#"{"version":1,"profile":"physical-boottime-v1","node":"fl-a","host_kind":"physical","clocksource":"tsc","lease_us":5000000,"drift_ppm":100000,"rate_bound_attested":true,"observation_error_ns":1000,"no_live_rollback":true}"#;
    fn contract() -> Contract {
        decode_contract(VALID.as_bytes()).unwrap()
    }
    fn evidence() -> HostEvidence {
        HostEvidence {
            cpu: Ok("flags : constant_tsc nonstop_tsc".into()),
            vendor: Ok("Dell Inc.".into()),
            product: Ok("PowerEdge R750".into()),
            hypervisor: Ok(false),
            source: Ok("tsc\n".into()),
            boot_resolution: Ok(1),
            active_resolution: Ok(1000),
            reading: Ok(ClockReading {
                monotonic_before_ns: 100,
                boottime_ns: 200,
                monotonic_after_ns: 110,
            }),
        }
    }
    #[test]
    fn strict_bounded_contract_schema() {
        assert!(decode_contract(VALID.as_bytes()).is_ok());
        for bad in [
            "",
            "null",
            "{}",
            "[]",
            "{",
            &format!("{VALID}{{}}"),
            &VALID.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
            &VALID.replacen("\"version\":1", "\"version\":1,\"force\":true", 1),
            &VALID.replacen("5000000", "-1", 1),
            &VALID.replacen("5000000", "18446744073709551616", 1),
        ] {
            assert!(decode_contract(bad.as_bytes()).is_err(), "{bad}");
        }
        assert!(decode_contract(&[b' '; 4097]).is_err());
        assert!(decode_contract(&[0xff; 30]).is_err());
        for key in [
            "version",
            "profile",
            "node",
            "host_kind",
            "clocksource",
            "lease_us",
            "drift_ppm",
            "rate_bound_attested",
            "observation_error_ns",
            "no_live_rollback",
        ] {
            let mut v: serde_json::Value = serde_json::from_str(VALID).unwrap();
            v.as_object_mut().unwrap().remove(key);
            assert!(
                decode_contract(&serde_json::to_vec(&v).unwrap()).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn fixed_profile_and_attestation_have_no_override() {
        validate_contract(&contract(), Some("fl-a")).unwrap();
        for (key, value) in [
            ("version", serde_json::json!(2)),
            ("profile", serde_json::json!("virtual")),
            ("node", serde_json::json!("bad node")),
            ("host_kind", serde_json::json!("unknown")),
            ("clocksource", serde_json::json!("")),
            ("lease_us", serde_json::json!(4999999)),
            ("drift_ppm", serde_json::json!(99999)),
            ("rate_bound_attested", serde_json::json!(false)),
            ("observation_error_ns", serde_json::json!(1001)),
            ("no_live_rollback", serde_json::json!(false)),
        ] {
            let mut v: serde_json::Value = serde_json::from_str(VALID).unwrap();
            v[key] = value;
            let c = decode_contract(&serde_json::to_vec(&v).unwrap()).unwrap();
            assert!(validate_contract(&c, None).is_err(), "{key}");
        }
        assert!(validate_contract(&contract(), Some("fl-b")).is_err());
        for node in ["", "-bad", "bad/node", "bad\nnode", &"a".repeat(129)] {
            let mut c = contract();
            c.node = node.into();
            assert!(validate_contract(&c, None).is_err());
        }
    }
    #[test]
    fn qualification_fixture_matrix_observes_only_prerequisites() {
        let c = contract();
        let mut observed = vec![];
        qualify_evidence(&c, &evidence(), &mut observed).unwrap();
        assert_eq!(
            observed,
            [
                "physical_host_provenance",
                "matching_clocksource",
                "boottime_and_monotonic_resolution_at_most_1us",
                "ordered_clock_reads"
            ]
        );
        for case in 0..12 {
            let mut e = evidence();
            match case {
                0 => e.cpu = Err("unreadable host provenance or clocksource".into()),
                1 => e.vendor = Err("unknown host provenance or clocksource".into()),
                2 => e.cpu = Ok("flags: hypervisor".into()),
                3 => e.product = Ok("KVM virtual machine".into()),
                4 => e.hypervisor = Ok(true),
                5 => e.hypervisor = Err("unknown hypervisor provenance".into()),
                6 => e.vendor = Ok("Default string".into()),
                7 => e.source = Ok("hpet".into()),
                8 => e.boot_resolution = Ok(1001),
                9 => e.active_resolution = Ok(0),
                10 => e.boot_resolution = Err("clock_getres failed".into()),
                _ => e.reading = Err("sample_failed".into()),
            }
            assert!(
                qualify_evidence(&c, &e, &mut vec![]).is_err(),
                "case {case}"
            );
        }
        let json = serde_json::to_value(Inspection::new()).unwrap();
        assert_eq!(json["decision"], "refuse");
        assert_eq!(json["attestation"], false);
        assert!(json.get("measured_drift").is_none());
    }
    #[test]
    fn opened_file_facts_require_root_regular_bounded_nonwritable_file() {
        for mode in [0o400, 0o600, 0o644, 0o440] {
            validate_file(FileFacts {
                regular: true,
                uid: 0,
                mode,
                len: 4096,
            })
            .unwrap();
        }
        for f in [
            FileFacts {
                regular: false,
                uid: 0,
                mode: 0o600,
                len: 0,
            },
            FileFacts {
                regular: true,
                uid: 1000,
                mode: 0o600,
                len: 0,
            },
            FileFacts {
                regular: true,
                uid: 0,
                mode: 0o620,
                len: 0,
            },
            FileFacts {
                regular: true,
                uid: 0,
                mode: 0o602,
                len: 0,
            },
            FileFacts {
                regular: true,
                uid: 0,
                mode: 0o600,
                len: 4097,
            },
        ] {
            assert!(validate_file(f).is_err());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_nonregular_contract_inputs_never_wait_for_a_writer() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt, symlink};
        use std::process::Command;
        use std::time::Duration;
        let dir = std::env::temp_dir().join(format!("fl-clock-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(std::fs::metadata(&fifo).unwrap().file_type().is_fifo());
        let regular = dir.join("contract");
        std::fs::write(&regular, VALID).unwrap();
        std::fs::set_permissions(&regular, std::fs::Permissions::from_mode(0o666)).unwrap();
        let link = dir.join("link");
        symlink(&regular, &link).unwrap();
        let huge = dir.join("huge");
        std::fs::write(&huge, [b' '; 4097]).unwrap();
        for path in [&fifo, &dir, &link, &regular, &huge, Path::new("/dev/null")] {
            let owned = path.to_owned();
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let _ = tx.send(read_contract(&owned).is_err());
            });
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)),
                Ok(true),
                "contract open/read must refuse promptly: {}",
                path.display()
            );
            worker.join().unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
