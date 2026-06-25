//! Injectable wall-clock abstraction.
//!
//! Decay and TTL math must be deterministic in tests, so the system never calls
//! `SystemTime::now()` directly. Production uses [`SystemClock`]; tests drive
//! [`TestClock`] forward explicitly. All timestamps are UTC unix-epoch seconds (i64).

/// A source of the current UTC unix-epoch time, in seconds.
pub trait Clock: Send + Sync {
    /// Current UTC unix-epoch time, in whole seconds.
    fn now(&self) -> i64;
}

/// The production clock: real UTC wall time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        chrono::Utc::now().timestamp()
    }
}

/// A controllable clock for deterministic decay/TTL tests.
///
/// Gated behind `cfg(test)` or the `test-clock` feature so the binary crate's
/// integration tests can construct it without it leaking into release builds.
#[cfg(any(test, feature = "test-clock"))]
#[derive(Debug)]
pub struct TestClock(std::sync::atomic::AtomicI64);

#[cfg(any(test, feature = "test-clock"))]
impl TestClock {
    /// Create a test clock fixed at `start` epoch seconds.
    pub fn new(start: i64) -> Self {
        TestClock(std::sync::atomic::AtomicI64::new(start))
    }

    /// Move the clock forward by `delta` seconds.
    pub fn advance(&self, delta: i64) {
        self.0.fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
    }

    /// Set the clock to an absolute epoch-seconds value.
    pub fn set(&self, value: i64) {
        self.0.store(value, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(any(test, feature = "test-clock"))]
impl Clock for TestClock {
    fn now(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_positive_epoch() {
        assert!(SystemClock.now() > 0);
    }

    #[test]
    fn test_clock_advances_and_sets() {
        let clock = TestClock::new(1_000);
        assert_eq!(clock.now(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now(), 1_500);
        clock.set(42);
        assert_eq!(clock.now(), 42);
    }
}
