//! Injected elapsed clocks and process-wide, permanent success-admission quarantine.
//!
//! Each execution thread owns an observer. Only the immutable startup guard and atomic
//! admission domain are shared; no steady-state mutex or metadata access is required.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use flaminio_repl::clock::{
    ClockAdmissionState, ClockFault, ClockGuard, ClockQualification, ClockReading, OffsetInterval,
    checked_clock_ns,
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
        let closed = ClockAdmissionState::OPEN.quarantine(reason).word();
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
        if generation.domain != self.identity
            || !ClockAdmissionState::OPEN.admits(generation.number)
        {
            return Err(ClockError::StaleGeneration);
        }
        self.state
            .compare_exchange(0, 0, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|state| self.closed_error(state))?;
        Ok(())
    }

    fn decode(&self, state: u64) -> ClockStatus {
        let state = ClockAdmissionState::from_word(state);
        let reason = state.reason();
        ClockStatus {
            generation: ClockGeneration {
                domain: self.identity,
                number: state.generation(),
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

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;
    struct Script {
        value: Mutex<Result<ClockReading, ClockFault>>,
        reads: AtomicU64,
    }
    impl Script {
        fn new() -> Self {
            Self {
                value: Mutex::new(Ok(reading(0, 0))),
                reads: AtomicU64::new(0),
            }
        }
        fn set(&self, r: Result<ClockReading, ClockFault>) {
            *self.value.lock().unwrap() = r;
        }
    }
    impl ElapsedClock for Script {
        fn read(&self) -> Result<ClockReading, ClockFault> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            *self.value.lock().unwrap()
        }
    }
    fn reading(m: u128, b: u128) -> ClockReading {
        ClockReading {
            monotonic_before_ns: m,
            boottime_ns: b,
            monotonic_after_ns: m,
        }
    }
    fn domain(script: Arc<Script>) -> Arc<ClockDomain> {
        Arc::new(ClockDomain::new(script, ClockQualification::new(1, 1000, 1000).unwrap()).unwrap())
    }
    #[test]
    fn persistent_observers_have_independent_histories_and_one_shared_fault() {
        let script = Arc::new(Script::new());
        let d = domain(script.clone());
        let (mut a, mut b) = (d.observer(), d.observer());
        script.set(Ok(reading(100_000, 100_000)));
        let _ = a.sample().unwrap();
        script.set(Ok(reading(50_000, 50_000)));
        let _ = b.sample().unwrap();
        assert!(d.status().is_healthy());
        let stale = b.sample().unwrap();
        script.set(Ok(reading(99_000, 99_000)));
        assert!(a.sample().is_err());
        assert_eq!(d.status().generation.number(), 1);
        assert_eq!(d.status().reason, Some(ClockFault::Backwards));
        assert!(d.success_permit(stale).is_err());
        script.set(Ok(reading(200_000, 200_000)));
        assert!(b.sample().is_err());
        d.quarantine(ClockFault::SampleFailed);
        assert_eq!(d.status().reason, Some(ClockFault::Backwards));
    }
    #[test]
    fn sampled_before_fault_cannot_be_permitted_after_but_earlier_permit_can_deliver() {
        let script = Arc::new(Script::new());
        let d = domain(script.clone());
        let mut o = d.observer();
        let earlier = d.success_permit(o.sample().unwrap()).unwrap();
        let stale = o.sample().unwrap();
        d.quarantine(ClockFault::SampleFailed);
        assert!(d.success_permit(stale).is_err());
        earlier.consume();
        let reads = script.reads.load(Ordering::SeqCst);
        assert!(o.sample().is_err());
        assert_eq!(reads, script.reads.load(Ordering::SeqCst));
        let other = domain(Arc::new(Script::new()));
        let sample = other.observer().sample().unwrap();
        assert!(d.admit(sample).is_err());
        let sample = other.observer().sample().unwrap();
        let third = domain(Arc::new(Script::new()));
        assert_eq!(
            third.success_permit(sample).unwrap_err(),
            ClockError::StaleGeneration
        );
    }
    #[test]
    fn subscriptions_cannot_lose_a_concurrent_first_fault() {
        for _ in 0..128 {
            let d = domain(Arc::new(Script::new()));
            let barrier = Arc::new(Barrier::new(2));
            let worker = {
                let (d, b) = (d.clone(), barrier.clone());
                thread::spawn(move || {
                    b.wait();
                    d.quarantine(ClockFault::SuspendOffset);
                })
            };
            barrier.wait();
            let rx = d.subscribe();
            worker.join().unwrap();
            assert!(rx.try_recv().is_ok());
            assert_eq!(d.status().generation.number(), 1);
            assert!(d.subscribe().try_recv().is_ok());
        }
    }
    #[test]
    fn backend_failure_and_elapsed_overflow_close_all_observers() {
        for fault in [ClockFault::SampleFailed, ClockFault::Overflow] {
            let script = Arc::new(Script::new());
            let d = domain(script.clone());
            let mut o = d.observer();
            script.set(Err(fault));
            assert_eq!(o.sample().unwrap_err(), ClockError::Quarantined(fault));
            assert!(d.observer().sample().is_err());
        }
        let script = Arc::new(Script::new());
        let d = domain(script.clone());
        let ns = (u64::MAX as u128 + 1) * 1000;
        script.set(Ok(reading(ns, ns)));
        assert_eq!(
            d.observer().sample().unwrap_err(),
            ClockError::Quarantined(ClockFault::Overflow)
        );
    }
}
