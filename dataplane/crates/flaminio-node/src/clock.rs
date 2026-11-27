//! Injected elapsed clocks and process-wide, permanent success-admission quarantine.
//!
//! Each execution thread owns an observer. Only the immutable startup guard and atomic
//! admission domain are shared; no steady-state mutex or metadata access is required.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use flaminio_repl::clock::{
    ClockFault, ClockGuard, ClockQualification, ClockReading, OffsetInterval, checked_clock_ns,
};
use flaminio_repl::types::Time;

/// A backend must read MONOTONIC, BOOTTIME, MONOTONIC in that order on each call. It must
/// never substitute wall time, cached samples, or a wakeup timer for these observations.
pub trait ElapsedClock: Send + Sync {
    fn read(&self) -> Result<ClockReading, ClockFault>;
}

/// An admission identity is local to one process domain. Its numeric generation advances
/// on quarantine. The opaque domain identity also rejects samples from another domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockGeneration {
    domain: u64,
    number: u64,
}

impl ClockGeneration {
    pub const fn number(self) -> u64 {
        self.number
    }
}

/// A checked fresh observation. Moving it into final admission consumes that observation.
#[derive(Debug)]
#[must_use]
pub struct ClockSample {
    elapsed_us: Time,
    generation: ClockGeneration,
}

impl ClockSample {
    pub const fn elapsed_us(&self) -> Time {
        self.elapsed_us
    }

    pub const fn generation(&self) -> ClockGeneration {
        self.generation
    }
}

/// The final success decision, ordered atomically before quarantine. It may be delivered
/// after a later fault, but cannot be cloned or used for a second completion.
#[derive(Debug)]
#[must_use]
pub struct SuccessPermit {
    sample: ClockSample,
}

impl SuccessPermit {
    pub const fn generation(&self) -> ClockGeneration {
        self.sample.generation
    }

    pub const fn elapsed_us(&self) -> Time {
        self.sample.elapsed_us
    }

    /// Consume at the delivery boundary. No second clock check is required for a success
    /// already ordered before the fault; the caller must not reuse this as a new admission.
    pub fn consume(self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClockError {
    #[error("clock quarantined; operator restart required ({0})")]
    Quarantined(ClockFault),
    #[error("stale clock generation")]
    StaleGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockStatus {
    pub generation: ClockGeneration,
    pub reason: Option<ClockFault>,
}

impl ClockStatus {
    pub const fn is_healthy(self) -> bool {
        self.reason.is_none()
    }

    fn error(self) -> ClockError {
        match self.reason {
            Some(reason) => ClockError::Quarantined(reason),
            None => ClockError::StaleGeneration,
        }
    }
}

static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(1);
const GENERATION_SHIFT: u32 = 4;
const QUARANTINED: u64 = 1;

/// One daemon's clock admission domain. There is exactly one healthy-to-quarantined
/// transition and no reopen operation. State 0 is open generation 0; a closed state holds
/// generation 1, its fault reason, and the low quarantine bit in the same atomic word.
/// Consequently the generation can never wrap during a domain's lifetime.
pub struct ClockDomain {
    backend: Arc<dyn ElapsedClock>,
    startup: ClockGuard,
    identity: u64,
    state: AtomicU64,
    // Registered at startup; locked only on subscription or the exceptional fault path.
    cancellation: Mutex<Vec<smol::channel::Sender<()>>>,
}

impl ClockDomain {
    /// Call only after the deployment profile has been admitted. Qualification establishes
    /// the error bounds; it does not by itself attest hardware, clock rate or provenance.
    pub fn new(
        backend: Arc<dyn ElapsedClock>,
        qualification: ClockQualification,
    ) -> Result<Self, ClockFault> {
        let startup = ClockGuard::new(backend.read()?, qualification)?;
        let mut identity = NEXT_DOMAIN.load(Ordering::Relaxed);
        loop {
            let next = identity.checked_add(1).ok_or(ClockFault::Overflow)?;
            match NEXT_DOMAIN.compare_exchange_weak(
                identity,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => identity = actual,
            }
        }
        Ok(Self {
            backend,
            startup,
            identity,
            state: AtomicU64::new(0),
            cancellation: Mutex::new(Vec::new()),
        })
    }

    pub fn status(&self) -> ClockStatus {
        self.decode(self.state.load(Ordering::SeqCst))
    }

    /// Wake asynchronous workers on the first fault, including workers with no pending I/O.
    pub fn subscribe(&self) -> smol::channel::Receiver<()> {
        let (tx, rx) = smol::channel::bounded(1);
        let mut listeners = self.cancellation.lock().unwrap_or_else(|e| e.into_inner());
        if self.status().is_healthy() {
            listeners.push(tx);
        } else {
            let _ = tx.try_send(());
        }
        rx
    }

    /// One observer per execution thread. Cloning the startup guard preserves the common
    /// origin/anchor without comparing unrelated threads' observation ordering.
    pub fn observer(self: &Arc<Self>) -> ClockObserver {
        ClockObserver {
            domain: Arc::clone(self),
            guard: self.startup.clone(),
        }
    }

    pub fn check_generation(&self, generation: ClockGeneration) -> Result<(), ClockError> {
        let status = self.status();
        if let Some(reason) = status.reason {
            return Err(ClockError::Quarantined(reason));
        }
        if status.generation != generation {
            return Err(ClockError::StaleGeneration);
        }
        Ok(())
    }

    /// Close the whole process before dispatching a faulting observation's event. The
    /// first reason sticks and is published atomically with the advanced generation.
    pub fn quarantine(&self, reason: ClockFault) -> ClockStatus {
        let code = match reason {
            ClockFault::Profile => 1,
            ClockFault::SampleFailed => 2,
            ClockFault::Backwards => 3,
            ClockFault::SuspendOffset => 4,
            ClockFault::Overflow => 5,
        };
        let closed = (1 << GENERATION_SHIFT) | (code << 1) | QUARANTINED;
        let state = match self
            .state
            .compare_exchange(0, closed, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => {
                for listener in self
                    .cancellation
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .drain(..)
                {
                    let _ = listener.try_send(());
                }
                closed
            }
            Err(existing) => existing,
        };
        self.decode(state)
    }

    /// Consume a freshly observed sample only after rechecking the operation's lease,
    /// epoch, session and membership against its elapsed time. This CAS, including its
    /// successful 0-to-0 write, and quarantine's CAS share one atomic modification order.
    pub fn success_permit(&self, sample: ClockSample) -> Result<SuccessPermit, ClockError> {
        self.admit_generation(sample.generation)?;
        Ok(SuccessPermit { sample })
    }

    /// Order a freshly sampled operation/session dispatch against the first fault.
    /// This grants no client success; a separate final permit is still required.
    pub fn admit(&self, sample: ClockSample) -> Result<ClockGeneration, ClockError> {
        self.admit_generation(sample.generation)?;
        Ok(sample.generation)
    }

    fn admit_generation(&self, generation: ClockGeneration) -> Result<(), ClockError> {
        if generation.domain != self.identity || generation.number != 0 {
            return Err(ClockError::StaleGeneration);
        }
        self.state
            .compare_exchange(0, 0, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|state| self.closed_error(state))?;
        Ok(())
    }

    fn decode(&self, state: u64) -> ClockStatus {
        let reason = if state & QUARANTINED == 0 {
            None
        } else {
            Some(match (state >> 1) & 7 {
                1 => ClockFault::Profile,
                2 => ClockFault::SampleFailed,
                3 => ClockFault::Backwards,
                4 => ClockFault::SuspendOffset,
                _ => ClockFault::Overflow,
            })
        };
        ClockStatus {
            generation: ClockGeneration {
                domain: self.identity,
                number: state >> GENERATION_SHIFT,
            },
            reason,
        }
    }

    fn closed_error(&self, state: u64) -> ClockError {
        self.decode(state).error()
    }
}

/// Sequential observations for one execution thread. Moving an observer between threads
/// is allowed, but callers must not replace it with a new observer at each transition.
pub struct ClockObserver {
    domain: Arc<ClockDomain>,
    guard: ClockGuard,
}

impl ClockObserver {
    pub fn domain(&self) -> &Arc<ClockDomain> {
        &self.domain
    }

    /// Check the process gate, read afresh, validate locally, then recheck the gate. A
    /// concurrent fault can therefore never return a sample in the new closed generation.
    pub fn sample(&mut self) -> Result<ClockSample, ClockError> {
        let before = self.domain.status();
        if let Some(reason) = before.reason {
            return Err(ClockError::Quarantined(reason));
        }
        let observed = self
            .domain
            .backend
            .read()
            .and_then(|reading| self.guard.observe(reading));
        let elapsed_us = match observed {
            Ok(elapsed) => elapsed,
            Err(reason) => {
                self.guard.invalidate(reason);
                let status = self.domain.quarantine(reason);
                return Err(status.error());
            }
        };
        self.domain.check_generation(before.generation)?;
        Ok(ClockSample {
            elapsed_us,
            generation: before.generation,
        })
    }

    /// Create the final permit after the caller has revalidated the operation at this
    /// exact sample. Keeping sample and permit separate lets that core check run.
    pub fn success_permit(&self, sample: ClockSample) -> Result<SuccessPermit, ClockError> {
        self.domain.success_permit(sample)
    }
}
