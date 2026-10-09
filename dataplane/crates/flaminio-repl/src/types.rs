//! Identifiers, versions and timing parameters shared by the state machines.

/// A node: a machine that may run a target, hold chunk replicas, or both.
pub type NodeId = u32;
/// A volume.
pub type VolumeId = u32;
/// Index of a chunk within its volume.
pub type ChunkIdx = u32;
/// Index of a block within its chunk.
pub type Block = u32;
/// Volume epoch, issued by the metadata service on every attach.
pub type Epoch = u64;
/// Block contents. Blocks that were never written read as 0.
pub type Value = u64;
/// Local monotonic time in microseconds. Each node has its own clock; no two agree.
pub type Time = u64;

/// Version of a write: the target's epoch and its sequence number within that epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub epoch: Epoch,
    pub seq: u64,
}

/// Timing parameters of a target. All durations are measured on the target's own clock.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Lease length `L`: a confirmation for a request sent at `t` is valid until `t + L`.
    pub lease: Time,
    /// Bound on clock-rate difference `ρ`, in parts per million: any two clocks run at speeds
    /// within a factor `1 + ρ` of each other.
    pub drift_ppm: u64,
    /// Interval between heartbeats to each replica node.
    pub heartbeat: Time,
    /// A request without a reply after this long fails its session.
    pub request_timeout: Time,
    /// Wait before reopening a failed session.
    pub reconnect: Time,
    /// A metadata request without a reply after this long is sent again.
    pub meta_timeout: Time,
}

impl Timing {
    /// How long a new target waits after fencing one ISR member before it may act without
    /// the others: `L × (1 + ρ)`, rounded up. By then any older target's lease has expired.
    pub fn takeover_wait(&self) -> Time {
        self.lease + (self.lease * self.drift_ppm).div_ceil(1_000_000)
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            lease: 5_000_000,
            drift_ppm: 100_000,
            heartbeat: 1_000_000,
            request_timeout: 3_000_000,
            reconnect: 500_000,
            meta_timeout: 2_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeover_wait_rounds_up() {
        let t = Timing {
            lease: 1_000_001,
            drift_ppm: 1,
            ..Timing::default()
        };
        assert_eq!(t.takeover_wait(), 1_000_003);
        assert_eq!(Timing::default().takeover_wait(), 5_500_000);
    }
}
