//! First-class cooperative cancellation. @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! A `CancelToken` is a cheap clonable first-request record paired with a tokio
//! notification (no thread bridge). The pipeline polls it at the reference checkpoints
//! and races an in-flight provider await against it, so a
//! cancellation that reaches the runtime before the commit boundary yields a
//! structured cancelled result with the scene unchanged.

use std::sync::{Arc, OnceLock};

use sergent_rs_core::timing::Timestamp;
use tokio::sync::Notify;

/// Shared first-write cancellation state pairing request time with provider-await wakeups.
struct CancelInner {
    requested_at: OnceLock<Timestamp>,
    notify: Notify,
}

/// A clonable cancellation token shared between a `RunHandle` and its run.
/// @sergent/docs/execution-model.md
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

impl Default for CancelToken {
    /// Builds the same untripped token as `CancelToken::new`.
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    /// Create an untripped token.
    /// @sergent/docs/execution-model.md
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancelInner {
                requested_at: OnceLock::new(),
                notify: Notify::new(),
            }),
        }
    }

    /// Record the first cancellation request and wake any awaiter.
    /// @sergent/docs/execution-model.md
    pub fn cancel(&self) {
        self.cancel_with(|| crate::clock::sample().timestamp());
    }

    /// Samples request time only while untripped, then delegates first-write ownership to `trip`.
    fn cancel_with(&self, timestamp: impl FnOnce() -> Timestamp) {
        if self.is_cancelled() {
            return;
        }
        self.trip(timestamp());
    }

    /// Whether cancellation has been requested (the synchronous checkpoint).
    /// @sergent/docs/execution-model.md
    pub fn is_cancelled(&self) -> bool {
        self.inner.requested_at.get().is_some()
    }

    /// Resolve as soon as the token is tripped, for racing a provider await.
    /// Re-checks request state around arming the notification to avoid a missed
    /// wakeup, so it is deterministic under a parked scheduler.
    /// @sergent/docs/execution-model.md
    pub async fn cancelled(&self) -> Timestamp {
        loop {
            if let Some(requested_at) = self.requested_at() {
                return requested_at;
            }
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(requested_at) = self.requested_at() {
                return requested_at;
            }
            notified.await;
            if let Some(requested_at) = self.requested_at() {
                return requested_at;
            }
        }
    }

    /// Returns the first request time for checkpoints and Run Record cancellation evidence.
    pub(crate) fn requested_at(&self) -> Option<Timestamp> {
        self.inner.requested_at.get().copied()
    }

    /// Installs the first request time and wakes provider-await racers only when this call wins.
    fn trip(&self, requested_at: Timestamp) {
        if self.inner.requested_at.set(requested_at).is_ok() {
            self.inner.notify.notify_waiters();
        }
    }

    #[cfg(test)]
    /// Requests cancellation at a supplied evidence time for deterministic owner tests.
    pub(crate) fn cancel_at(&self, requested_at: Timestamp) {
        self.cancel_with(|| requested_at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_cancellation_preserves_the_first_request_timestamp() {
        let cancel = CancelToken::new();

        cancel.cancel_at(Timestamp::from_unix_micros(41));
        cancel.cancel_at(Timestamp::from_unix_micros(99));

        assert_eq!(cancel.requested_at(), Some(Timestamp::from_unix_micros(41)));
    }
}
