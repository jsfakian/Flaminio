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
    /// How often the target reads its volume from the metadata service, to learn of
    /// requested replica moves.
    pub refresh: Time,
}

impl Timing {
    /// Two qualified absolute read errors and microsecond conversion quantization.
    pub const ELAPSED_ERROR_US: Time = 3;

    /// How long a new target waits after fencing one ISR member before it may act without
    /// the others: `ceil((L + E) × (1 + ρ)) + E`, with E = 3 us. By then any older
    /// target's lease has expired, even under opposing elapsed-observation errors.
    pub fn takeover_wait(&self) -> Result<Time, crate::clock::ClockFault> {
        let lease = u128::from(self.lease).checked_add(u128::from(Self::ELAPSED_ERROR_US));
        let rate = u128::from(self.drift_ppm).checked_add(1_000_000);
        let wait = lease
            .zip(rate)
            .and_then(|(lease, rate)| lease.checked_mul(rate))
            .and_then(|product| product.checked_add(999_999))
            .map(|product| product / 1_000_000)
            .and_then(|wait| wait.checked_add(u128::from(Self::ELAPSED_ERROR_US)))
            .ok_or(crate::clock::ClockFault::Overflow)?;
        Time::try_from(wait).map_err(|_| crate::clock::ClockFault::Overflow)
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
            refresh: 2_000_000,
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
        assert_eq!(t.takeover_wait(), Ok(1_000_009));
        assert_eq!(Timing::default().takeover_wait(), Ok(5_500_007));
    }
}
