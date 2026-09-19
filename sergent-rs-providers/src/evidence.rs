//! Shared call-evidence constructors. Wall timestamps are inert evidence;
//! elapsed durations come from a monotonic clock. An interrupted await creates
//! no synthetic attempt. @sergent/docs/execution-model.md

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sergent_rs_core::error::RunError;
use sergent_rs_core::model::Attempt;
use sergent_rs_core::timing::{TimeSpan, Timestamp};

/// One opened timing span, retaining independent wall and monotonic facts.
pub(crate) struct ClockMark {
    wall: Timestamp,
    monotonic_ms: u64,
}

/// The closed clock source used by transport. Production uses system clocks;
/// tests may inject a manually advanced clock without virtual dispatch.
#[derive(Clone)]
pub(crate) enum Clock {
    System {
        origin: Instant,
    },
    #[cfg(test)]
    Manual(ManualClock),
}

impl Clock {
    /// Creates the system clock and anchors monotonic elapsed measurements at
    /// construction time.
    pub(crate) fn production() -> Self {
        Self::System {
            origin: Instant::now(),
        }
    }

    /// Opens an evidence span with independent wall and monotonic readings.
    pub(crate) fn start(&self) -> ClockMark {
        ClockMark {
            wall: self.wall_timestamp(),
            monotonic_ms: self.monotonic_ms(),
        }
    }

    /// Closes a span with wall endpoints while deriving duration only from the
    /// monotonic source.
    pub(crate) fn close(&self, mark: &ClockMark) -> TimeSpan {
        TimeSpan::closed(
            mark.wall,
            self.wall_timestamp(),
            self.monotonic_ms().saturating_sub(mark.monotonic_ms),
        )
    }

    /// Returns monotonic elapsed milliseconds without consulting wall time.
    pub(crate) fn elapsed_ms(&self, mark: &ClockMark) -> u64 {
        self.monotonic_ms().saturating_sub(mark.monotonic_ms)
    }

    /// Reads inert wall-time evidence from the selected system or manual
    /// source.
    fn wall_timestamp(&self) -> Timestamp {
        match self {
            Self::System { .. } => wall_now_timestamp(),
            #[cfg(test)]
            Self::Manual(clock) => Timestamp::from_unix_micros(
                clock
                    .state()
                    .wall_ms
                    .checked_mul(1_000)
                    .expect("manual wall clock exceeds timestamp representation"),
            ),
        }
    }

    /// Reads elapsed-time authority from the system origin or manual clock.
    fn monotonic_ms(&self) -> u64 {
        match self {
            Self::System { origin } => millis(origin.elapsed().as_millis()),
            #[cfg(test)]
            Self::Manual(clock) => clock.state().monotonic_ms,
        }
    }
}

/// One successful attempt: no retryability and no error.
pub(crate) fn success_attempt(span: TimeSpan) -> Attempt {
    Attempt::success(span)
}

/// One failed attempt carrying its retryability and structured error.
pub(crate) fn failure_attempt(span: TimeSpan, mut error: RunError, retryable: bool) -> Attempt {
    error.metadata =
        serde_json::Map::from_iter([("retryable".to_owned(), serde_json::Value::Bool(retryable))]);
    Attempt::failure(span, retryable, error)
}

/// Reads Unix wall time for inert evidence and rejects a pre-epoch host clock
/// instead of fabricating epoch zero.
fn wall_now_timestamp() -> Timestamp {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch");
    let microseconds =
        u64::try_from(elapsed.as_micros()).expect("system clock exceeds timestamp representation");
    Timestamp::from_unix_micros(microseconds)
}

/// Narrows millisecond evidence to `u64`, saturating only unrepresentably
/// large durations.
fn millis(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Shared deterministic test clock controlling wall and monotonic evidence
/// without reading system time.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ManualClock {
    state: std::sync::Arc<std::sync::Mutex<ManualClockState>>,
}

/// One synchronized snapshot of the manual clock's independent evidence
/// sources.
#[cfg(test)]
#[derive(Clone, Copy)]
struct ManualClockState {
    wall_ms: u64,
    monotonic_ms: u64,
}

#[cfg(test)]
impl ManualClock {
    /// Seeds wall time, starts monotonic time at zero, and returns both the
    /// provider clock and its test controller.
    pub(crate) fn new(wall_ms: u64) -> (Clock, Self) {
        let clock = Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(ManualClockState {
                wall_ms,
                monotonic_ms: 0,
            })),
        };
        (Clock::Manual(clock.clone()), clock)
    }

    /// Advances wall and monotonic readings together with saturating
    /// arithmetic.
    pub(crate) fn advance(&self, milliseconds: u64) {
        let mut state = self.state.lock().expect("manual clock mutex");
        state.monotonic_ms = state.monotonic_ms.saturating_add(milliseconds);
        state.wall_ms = state.wall_ms.saturating_add(milliseconds);
    }

    /// Repositions only wall evidence, allowing tests to prove duration remains
    /// monotonic across wall-clock changes.
    pub(crate) fn set_wall(&self, wall_ms: u64) {
        self.state.lock().expect("manual clock mutex").wall_ms = wall_ms;
    }

    /// Copies one synchronized state snapshot for clock reads.
    fn state(&self) -> ManualClockState {
        *self.state.lock().expect("manual clock mutex")
    }
}
