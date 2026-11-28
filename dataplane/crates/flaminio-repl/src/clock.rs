//! Pure validation of suspend-aware elapsed-clock observations. No clock is read here.

use crate::types::Time;

/// The supported profile's maximum absolute error and reported clock resolution.
pub const MAX_OBSERVATION_ERROR_NS: u64 = 1_000;
/// Widening of the immutable startup offset interval, solely for suspend detection.
pub const SUSPEND_TOLERANCE_NS: i128 = 1_000_000;

/// Bounded, non-sensitive reasons for refusing or quarantining a lease clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockFault {
    Profile,
    SampleFailed,
    Backwards,
    SuspendOffset,
    Overflow,
}

impl ClockFault {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::SampleFailed => "sample_failed",
            Self::Backwards => "backwards",
            Self::SuspendOffset => "suspend_offset",
            Self::Overflow => "overflow",
        }
    }
}

impl std::fmt::Display for ClockFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for ClockFault {}

/// Qualified bounds, including read error and quantization, established at profile admission.
/// Reported resolution alone cannot establish the absolute observation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockQualification {
    boottime_resolution_ns: u64,
    monotonic_resolution_ns: u64,
    absolute_error_ns: u64,
}

impl ClockQualification {
    /// Missing bounds cannot construct a qualification. Both resolutions must be positive.
    pub fn new(
        boottime_resolution_ns: u64,
        monotonic_resolution_ns: u64,
        absolute_error_ns: u64,
    ) -> Result<Self, ClockFault> {
        if boottime_resolution_ns == 0
            || monotonic_resolution_ns == 0
            || boottime_resolution_ns > MAX_OBSERVATION_ERROR_NS
            || monotonic_resolution_ns > MAX_OBSERVATION_ERROR_NS
            || absolute_error_ns > MAX_OBSERVATION_ERROR_NS
        {
            return Err(ClockFault::Profile);
        }
        Ok(Self {
            boottime_resolution_ns,
            monotonic_resolution_ns,
            absolute_error_ns,
        })
    }

    pub const fn boottime_resolution_ns(self) -> u64 {
        self.boottime_resolution_ns
    }

    pub const fn monotonic_resolution_ns(self) -> u64 {
        self.monotonic_resolution_ns
    }

    pub const fn absolute_error_ns(self) -> u64 {
        self.absolute_error_ns
    }
}

/// One ordered MONOTONIC, BOOTTIME, MONOTONIC sampling sequence, in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockReading {
    pub monotonic_before_ns: u128,
    pub boottime_ns: u128,
    pub monotonic_after_ns: u128,
}

/// Checked conversion for a clock_gettime/clock_getres timespec. No wall clock is accepted
/// by the guard's contract; the backend chooses BOOTTIME and MONOTONIC explicitly.
pub fn checked_clock_ns(seconds: i64, nanoseconds: i64) -> Result<u128, ClockFault> {
    if seconds < 0 || !(0..1_000_000_000).contains(&nanoseconds) {
        return Err(ClockFault::SampleFailed);
    }
    let seconds = u128::try_from(seconds).map_err(|_| ClockFault::Overflow)?;
    let nanoseconds = u128::try_from(nanoseconds).map_err(|_| ClockFault::Overflow)?;
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_add(nanoseconds))
        .ok_or(ClockFault::Overflow)
}

/// Possible BOOTTIME minus MONOTONIC offsets, in signed nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetInterval {
    pub lower_ns: i128,
    pub upper_ns: i128,
}

impl ClockReading {
    /// The sampling interval is exactly [B - M2, B - M1]. A long scheduling pause may
    /// make it wide; it is never evidence that suspend did not occur.
    pub fn offset_interval(self) -> Result<OffsetInterval, ClockFault> {
        if self.monotonic_after_ns < self.monotonic_before_ns {
            return Err(ClockFault::Backwards);
        }
        let b = i128::try_from(self.boottime_ns).map_err(|_| ClockFault::Overflow)?;
        let m1 = i128::try_from(self.monotonic_before_ns).map_err(|_| ClockFault::Overflow)?;
        let m2 = i128::try_from(self.monotonic_after_ns).map_err(|_| ClockFault::Overflow)?;
        Ok(OffsetInterval {
            lower_ns: b.checked_sub(m2).ok_or(ClockFault::Overflow)?,
            upper_ns: b.checked_sub(m1).ok_or(ClockFault::Overflow)?,
        })
    }
}

/// One observer's history, with a shared immutable startup origin and offset anchor.
/// Clone the startup guard once per execution thread, never a global last observation:
/// samples from different threads can legitimately arrive in either order.
#[derive(Debug, Clone)]
pub struct ClockGuard {
    origin_ns: u128,
    anchor: OffsetInterval,
    last: ClockReading,
    fault: Option<ClockFault>,
}

impl ClockGuard {
    pub fn new(
        startup: ClockReading,
        _qualification: ClockQualification,
    ) -> Result<Self, ClockFault> {
        let offset = startup.offset_interval()?;
        let anchor = OffsetInterval {
            lower_ns: offset
                .lower_ns
                .checked_sub(SUSPEND_TOLERANCE_NS)
                .ok_or(ClockFault::Overflow)?,
            upper_ns: offset
                .upper_ns
                .checked_add(SUSPEND_TOLERANCE_NS)
                .ok_or(ClockFault::Overflow)?,
        };
        Ok(Self {
            origin_ns: startup.boottime_ns,
            anchor,
            last: startup,
            fault: None,
        })
    }

    pub const fn anchor(&self) -> OffsetInterval {
        self.anchor
    }

    pub const fn fault(&self) -> Option<ClockFault> {
        self.fault
    }

    /// The first fault sticks, including backend failure. Successful reads cannot reopen it.
    pub fn invalidate(&mut self, reason: ClockFault) -> ClockFault {
        *self.fault.get_or_insert(reason)
    }

    /// Check before processing the event associated with this observation. A supported
    /// process pause only widens an interval and advances elapsed time.
    pub fn observe(&mut self, reading: ClockReading) -> Result<Time, ClockFault> {
        if let Some(reason) = self.fault {
            return Err(reason);
        }
        match self.validate(reading) {
            Ok(elapsed) => {
                self.last = reading;
                Ok(elapsed)
            }
            Err(reason) => Err(self.invalidate(reason)),
        }
    }

    fn validate(&self, reading: ClockReading) -> Result<Time, ClockFault> {
        let interval = reading.offset_interval()?;
        if reading.boottime_ns < self.last.boottime_ns
            || reading.monotonic_before_ns < self.last.monotonic_after_ns
        {
            return Err(ClockFault::Backwards);
        }
        if interval.lower_ns > self.anchor.upper_ns || interval.upper_ns < self.anchor.lower_ns {
            return Err(ClockFault::SuspendOffset);
        }
        let elapsed_ns = reading
            .boottime_ns
            .checked_sub(self.origin_ns)
            .ok_or(ClockFault::Backwards)?;
        Time::try_from(elapsed_ns / 1_000).map_err(|_| ClockFault::Overflow)
    }
}

/// Pure representation of the runtime atomic admission word. The only open state is
/// generation zero; the first fault advances to generation one permanently. Atomic
/// ordering remains the caller's responsibility (including the successful open-to-open CAS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockAdmissionState(u64);

impl ClockAdmissionState {
    pub const OPEN: Self = Self(0);

    pub const fn from_word(word: u64) -> Self {
        Self(word)
    }
    pub const fn word(self) -> u64 {
        self.0
    }
    pub const fn generation(self) -> u64 {
        self.0 >> 4
    }
    pub const fn admits(self, generation: u64) -> bool {
        self.0 == 0 && generation == 0
    }

    pub const fn quarantine(self, reason: ClockFault) -> Self {
        if self.0 != 0 {
            return self;
        }
        let code = match reason {
            ClockFault::Profile => 1,
            ClockFault::SampleFailed => 2,
            ClockFault::Backwards => 3,
            ClockFault::SuspendOffset => 4,
            ClockFault::Overflow => 5,
        };
        Self(16 | (code << 1) | 1)
    }

    pub const fn reason(self) -> Option<ClockFault> {
        if self.0 & 1 == 0 {
            return None;
        }
        Some(match (self.0 >> 1) & 7 {
            1 => ClockFault::Profile,
            2 => ClockFault::SampleFailed,
            3 => ClockFault::Backwards,
            4 => ClockFault::SuspendOffset,
            _ => ClockFault::Overflow,
        })
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    fn r(m1: u128, b: u128, m2: u128) -> ClockReading {
        ClockReading {
            monotonic_before_ns: m1,
            boottime_ns: b,
            monotonic_after_ns: m2,
        }
    }
    fn guard() -> ClockGuard {
        ClockGuard::new(
            r(100, 200, 110),
            ClockQualification::new(1, 1000, 1000).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn interval_pause_short_suspend_and_detection_boundary() {
        assert_eq!(
            r(10, 30, 20).offset_interval().unwrap(),
            OffsetInterval {
                lower_ns: 10,
                upper_ns: 20
            }
        );
        let mut g = guard();
        assert_eq!(g.observe(r(10_000_100, 10_000_200, 10_000_110)), Ok(10000));
        // A wide sample is inconclusive, not a claim that suspend was absent.
        let mut g = guard();
        assert!(g.observe(r(110, 50_000_000, 100_000_000)).is_ok());
        for suspend in [1, 999, 1000, 999_999, 1_000_000] {
            let mut g = guard();
            assert!(g.observe(r(110, 210 + suspend, 120)).is_ok(), "{suspend}");
        }
        let mut g = guard();
        assert_eq!(
            g.observe(r(110, 2_000_210, 120)),
            Err(ClockFault::SuspendOffset)
        );
        assert_eq!(g.observe(r(200, 300, 210)), Err(ClockFault::SuspendOffset));
    }
    #[test]
    fn checked_conversion_and_every_fault_are_sticky() {
        assert_eq!(
            checked_clock_ns(i64::MAX, 999_999_999),
            Ok(i64::MAX as u128 * 1_000_000_000 + 999_999_999)
        );
        for (s, n) in [(-1, 0), (0, -1), (0, 1_000_000_000)] {
            assert_eq!(checked_clock_ns(s, n), Err(ClockFault::SampleFailed));
        }
        for (reading, fault) in [
            (r(100, 210, 120), ClockFault::Backwards),
            (r(120, 199, 130), ClockFault::Backwards),
            (r(120, u128::MAX, 130), ClockFault::Overflow),
            (
                r(120, (u64::MAX as u128 + 1) * 1000 + 200, 130),
                ClockFault::SuspendOffset,
            ),
        ] {
            let mut g = guard();
            assert_eq!(g.observe(reading), Err(fault));
            assert_eq!(g.observe(r(130, 230, 140)), Err(fault));
        }
        for fault in [
            ClockFault::Profile,
            ClockFault::SampleFailed,
            ClockFault::Backwards,
            ClockFault::SuspendOffset,
            ClockFault::Overflow,
        ] {
            let closed = ClockAdmissionState::OPEN.quarantine(fault);
            assert_eq!(closed.generation(), 1);
            assert!(!closed.admits(0));
            assert!(!closed.admits(1));
            assert_eq!(closed.reason(), Some(fault));
            assert_eq!(closed.quarantine(ClockFault::Profile), closed);
        }
        for b in [0, 1001] {
            assert_eq!(
                ClockQualification::new(b, 1, 1000),
                Err(ClockFault::Profile)
            );
        }
        assert!(ClockQualification::new(1, 1, 1001).is_err());
    }
}
