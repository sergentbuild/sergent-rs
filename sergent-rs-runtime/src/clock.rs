//! Clock sampling for run evidence. @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! Core is clock-free (its `Timestamp` is inert); the runtime is the caller that
//! captures wall-clock endpoints and measures elapsed time with `Instant`.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sergent_rs_core::timing::{TimeSpan, Timestamp};

/// One wall-clock endpoint paired with the monotonic sample used only for
/// elapsed duration.
#[derive(Clone, Copy)]
pub(crate) struct ClockSample {
    timestamp: Timestamp,
    monotonic: Instant,
}

impl ClockSample {
    /// Read the wall-clock endpoint.
    pub(crate) fn timestamp(self) -> Timestamp {
        self.timestamp
    }
}

/// Capture the current wall and monotonic clocks together.
pub(crate) fn sample() -> ClockSample {
    sample_from(SystemTime::now(), Instant::now())
}

/// Pairs independently supplied wall and monotonic readings into one evidence sample.
fn sample_from(wall: SystemTime, monotonic: Instant) -> ClockSample {
    ClockSample {
        timestamp: timestamp_from(wall),
        monotonic,
    }
}

/// Converts wall time to Unix microsecond evidence, rejecting invalid host-clock bounds.
fn timestamp_from(wall: SystemTime) -> Timestamp {
    let elapsed = wall
        .duration_since(UNIX_EPOCH)
        .expect("host clock is before the Unix epoch");
    let microseconds =
        u64::try_from(elapsed.as_micros()).expect("host clock exceeds timestamp representation");
    Timestamp::from_unix_micros(microseconds)
}

/// Close a span with wall-clock endpoints and monotonic elapsed duration.
pub(crate) fn span_closed(started: ClockSample, finished: ClockSample) -> TimeSpan {
    let duration = finished.monotonic.duration_since(started.monotonic);
    let duration_ms =
        u64::try_from(duration.as_millis()).expect("elapsed duration exceeds representation");
    TimeSpan::closed(started.timestamp, finished.timestamp, duration_ms)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn supplied_samples_use_monotonic_elapsed_when_wall_clock_moves_back() {
        let monotonic = Instant::now();
        let started = sample_from(UNIX_EPOCH + Duration::from_millis(200), monotonic);
        let finished = sample_from(
            UNIX_EPOCH + Duration::from_millis(100),
            monotonic + Duration::from_millis(37),
        );

        let span = span_closed(started, finished);

        assert_eq!(span.started_at(), Timestamp::from_unix_micros(200_000));
        assert_eq!(span.finished_at(), Timestamp::from_unix_micros(100_000));
        assert_eq!(span.duration_ms(), 37);
    }

    #[test]
    #[should_panic(expected = "host clock is before the Unix epoch")]
    fn supplied_pre_epoch_wall_clock_panics_immediately() {
        let wall = UNIX_EPOCH.checked_sub(Duration::from_millis(1)).unwrap();

        let _ = sample_from(wall, Instant::now());
    }
}
