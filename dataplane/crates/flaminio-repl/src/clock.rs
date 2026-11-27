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
