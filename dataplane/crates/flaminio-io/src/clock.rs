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
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || metadata.len() > CONTRACT_LIMIT
        {
            return Err("clock contract must be a root-owned regular file, at most 4096 bytes, without group/world write permission".into());
        }
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

#[cfg(target_os = "linux")]
fn qualify(
    contract: &Contract,
    observed: &mut Vec<&'static str>,
) -> Result<ClockQualification, String> {
    let root = provenance_root()?;
    let cpu = bounded_text(&root.join("proc/cpuinfo"))?.to_ascii_lowercase();
    let vendor = bounded_text(&root.join("sys/class/dmi/id/sys_vendor"))?.to_ascii_lowercase();
    let product = bounded_text(&root.join("sys/class/dmi/id/product_name"))?.to_ascii_lowercase();
    let hypervisor = root.join("sys/hypervisor/type");
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
        || hypervisor
            .try_exists()
            .map_err(|_| "unknown hypervisor provenance")?
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
    let source = bounded_text(
        &root.join("sys/devices/system/clocksource/clocksource0/current_clocksource"),
    )?;
    if source.trim() != contract.clocksource
        || contract.clocksource.is_empty()
        || contract.clocksource.len() > 128
    {
        return Err("qualified clocksource does not match the host".into());
    }
    observed.push("matching_clocksource");
    let resolution = |id| -> Result<u64, String> {
        let (sec, ns) =
            flaminio_uring::clock::elapsed_resolution(id).map_err(|_| "clock_getres failed")?;
        let ns = flaminio_node::clock::checked_clock_ns(sec, ns).map_err(|e| e.to_string())?;
        u64::try_from(ns).map_err(|_| "clock resolution overflow".into())
    };
    let q = ClockQualification::new(
        resolution(flaminio_uring::clock::ElapsedSource::Boottime)?,
        resolution(flaminio_uring::clock::ElapsedSource::Monotonic)?,
        contract.observation_error_ns,
    )
    .map_err(|e| e.to_string())?;
    observed.push("boottime_and_monotonic_resolution_at_most_1us");
    flaminio_node::clock::ClockGuard::new(LinuxClock.read().map_err(|e| e.to_string())?, q)
        .map_err(|e| e.to_string())?;
    observed.push("ordered_clock_reads");
    Ok(q)
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

fn inspect(path: &Path, node: Option<&str>) -> (Inspection, Option<ClockQualification>) {
    let mut result = Inspection::new();
    let checked = (|| {
        let c = read_contract(path)?;
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
