//! Ideal time is never read from a simulated node. Clock segments and detector memory may
//! change while the scheduler and checker keep their monotone real-time histories.

use crate::clock::{ClockFault, ClockGuard, ClockQualification, ClockReading};
use crate::types::{Time, Timing};

#[derive(Debug, Clone)]
pub(super) struct Clock {
    rate_ppm: u64,
    anchor_real: u64,
    boot_ns: u128,
    active_ns: u128,
    boot_advances: bool,
    active_advances: bool,
    origin_ns: u128,
    guard: ClockGuard,
    injected: Option<ClockFault>,
}

impl Clock {
    pub(super) fn new(rate_ppm: u64, offset_us: u64, real: u64) -> Self {
        let ns = u128::from(offset_us) * 1_000;
        let startup = ClockReading {
            monotonic_before_ns: ns,
            boottime_ns: ns,
            monotonic_after_ns: ns,
        };
        Self {
            rate_ppm,
            anchor_real: real,
            boot_ns: ns,
            active_ns: ns,
            boot_advances: true,
            active_advances: true,
            origin_ns: ns,
            guard: ClockGuard::new(startup, ClockQualification::new(1, 1, 1_000).unwrap()).unwrap(),
            injected: None,
        }
    }

    fn values(&self, real: u64) -> (u128, u128) {
        assert!(real >= self.anchor_real, "ideal time cannot rewind");
        let delta =
            u128::from(real - self.anchor_real) * 1_000 * u128::from(self.rate_ppm) / 1_000_000;
        (
            self.boot_ns + if self.boot_advances { delta } else { 0 },
            self.active_ns + if self.active_advances { delta } else { 0 },
        )
    }

    pub(super) fn local(&self, real: u64) -> Time {
        let (boot, _) = self.values(real);
        u64::try_from(boot.saturating_sub(self.origin_ns) / 1_000).unwrap_or(u64::MAX)
    }

    /// Invert only the current segment, bounded below by the scheduler's current time.
    /// A stopped clock has no future wakeup until a resume event changes its segment.
    pub(super) fn real_at(&self, local: Time, now: u64) -> Option<u64> {
        if !self.boot_advances {
            return None;
        }
        let required = self.origin_ns + u128::from(local) * 1_000;
        let delta = required.saturating_sub(self.boot_ns);
        let real_delta = (delta * 1_000_000).div_ceil(u128::from(self.rate_ppm) * 1_000);
        self.anchor_real
            .checked_add(u64::try_from(real_delta).ok()?)
            .map(|t| t.max(now))
    }

    fn rebase(&mut self, real: u64) {
        (self.boot_ns, self.active_ns) = self.values(real);
        self.anchor_real = real;
    }

    pub(super) fn suspend(&mut self, real: u64, virtual_stop: bool) {
        self.rebase(real);
        self.active_advances = false;
        self.boot_advances = !virtual_stop;
    }

    pub(super) fn resume(&mut self, real: u64) {
        self.rebase(real);
        self.boot_advances = true;
        self.active_advances = true;
    }

    pub(super) fn inject(&mut self, fault: ClockFault) {
        self.injected = Some(fault);
    }

    pub(super) fn observe(&mut self, real: u64) -> Result<Time, ClockFault> {
        let (boot, active) = self.values(real);
        let mut reading = ClockReading {
            monotonic_before_ns: active,
            boottime_ns: boot,
            monotonic_after_ns: active,
        };
        match self.injected.take() {
            Some(ClockFault::SampleFailed | ClockFault::Profile) => {
                return Err(self.guard.invalidate(ClockFault::SampleFailed));
            }
            Some(ClockFault::Backwards) => {
                // Invalid sample order, independent of lease expiry and forward clock time.
                reading.monotonic_before_ns = active + 1;
            }
            Some(ClockFault::Overflow) => reading.boottime_ns = u128::MAX,
            Some(ClockFault::SuspendOffset) => reading.boottime_ns += 2_000_000,
            None => {}
        }
        self.guard.observe(reading)
    }
}

/// Excluded traces deliberately restore detector memory as well as its time source. The
/// profile decision is independent of whether userspace can distinguish the observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludedEvent {
    VirtualClockStop,
    SnapshotRollback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedTrace {
    pub event: ExcludedEvent,
    pub profile_admitted: bool,
    pub observations_indistinguishable: bool,
    pub real_times: Vec<u64>,
    pub observed_elapsed: Vec<u64>,
}

pub fn excluded_trace(event: ExcludedEvent) -> ExcludedTrace {
    let mut clock = Clock::new(1_000_000, 100, 0);
    let first = clock.observe(100).unwrap();
    let mut snapshot = clock.clone();
    snapshot.rebase(100);
    let before = clock.observe(200).unwrap();
    match event {
        ExcludedEvent::VirtualClockStop => {
            clock.suspend(200, true);
            clock.resume(10_000_200);
        }
        ExcludedEvent::SnapshotRollback => {
            // Restoring node memory never restores ideal time or the append-only history.
            clock = snapshot;
            clock.anchor_real = 10_000_100;
        }
    }
    let after = clock.observe(10_000_200).unwrap();
    ExcludedTrace {
        event,
        profile_admitted: false,
        observations_indistinguishable: before == after,
        real_times: vec![100, 200, 10_000_200],
        observed_elapsed: vec![first, before, after],
    }
}

/// A read's device resolution and phase are separate from its signed measurement error.
/// Only readings whose *total* error (including resolution) is <= 1 us are qualified.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub resolution_ns: u64,
    pub phase_ns: u64,
    pub error_ns: i64,
}

impl Observation {
    pub fn read(self, ideal_ns: u128) -> Option<u128> {
        if self.resolution_ns == 0
            || self.resolution_ns > 1_000
            || self.phase_ns >= self.resolution_ns
        {
            return None;
        }
        let ideal = i128::try_from(ideal_ns).ok()?;
        let shifted = ideal.checked_add(i128::from(self.error_ns))?;
        let q = i128::from(self.resolution_ns);
        let phase = i128::from(self.phase_ns);
        let observed = shifted.checked_add(phase)?.div_euclid(q) * q - phase;
        if (observed - ideal).abs() > 1_000 {
            return None;
        }
        u128::try_from(observed).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingMutation {
    None,
    IgnoreObservationError,
}

#[derive(Debug, Clone)]
pub struct TakeoverWitness {
    pub wait_us: u64,
    pub takeover_real_ns: u64,
    pub old_lease_elapsed_us: u64,
    pub lease_us: u64,
}

impl TakeoverWitness {
    pub fn check(&self) -> Result<(), String> {
        if self.old_lease_elapsed_us < self.lease_us {
            Err(format!(
                "ideal takeover overlap: new target at {} ns while old elapsed={} < L={}",
                self.takeover_real_ns, self.old_lease_elapsed_us, self.lease_us
            ))
        } else {
            Ok(())
        }
    }
}

/// Use independently timed reads, not the takeover formula, to find the first instant the
/// faster target could finish its wait. The old clock has opposing send/completion errors.
/// A nanosecond binary search retains fractional read and microsecond-conversion phases.
pub fn takeover_witness(timing: Timing, mutation: TimingMutation, phase: u64) -> TakeoverWitness {
    let wait_us = match mutation {
        TimingMutation::None => timing.takeover_wait().unwrap(),
        TimingMutation::IgnoreObservationError => timing.takeover_wait_without_error().unwrap(),
    };
    let start = 10_000_000u128 + u128::from(phase % 1_000);
    let origin = 1u128;
    let old_send = Observation {
        resolution_ns: 1,
        phase_ns: 0,
        error_ns: 1_000,
    };
    let old_end = Observation {
        resolution_ns: 1_000,
        phase_ns: phase % 1_000,
        error_ns: 0,
    };
    let new_send = Observation {
        resolution_ns: 1,
        phase_ns: 0,
        error_ns: -1_000,
    };
    let new_end = Observation {
        resolution_ns: 1,
        phase_ns: 0,
        error_ns: 1_000,
    };
    let old_start = (old_send.read(start).unwrap() - origin) / 1_000;
    let new_start = (new_send.read(start).unwrap() - origin) / 1_000;
    let new_elapsed = |real: u64| {
        let ideal =
            start + u128::from(real) * (1_000_000 + u128::from(timing.drift_ppm)) / 1_000_000;
        (new_end.read(ideal).unwrap() - origin) / 1_000 - new_start
    };
    let mut lo = 0;
    let mut hi = wait_us.checked_mul(1_000).unwrap();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if new_elapsed(mid) >= u128::from(wait_us) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let old_elapsed = (old_end.read(start + u128::from(lo)).unwrap() - origin) / 1_000 - old_start;
    TakeoverWitness {
        wait_us,
        takeover_real_ns: lo,
        old_lease_elapsed_us: u64::try_from(old_elapsed).unwrap(),
        lease_us: timing.lease,
    }
}
