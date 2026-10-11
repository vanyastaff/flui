//! Timing proposals commit only with the position they describe.

use std::time::Duration;

use super::AnimationControllerInner;
use crate::PlaybackRate;

pub(super) struct SampleIdentity {
    pub(super) generation: u64,
    pub(super) epoch: u64,
}

/// Proposed timing belongs to a position sample until that sample commits.
/// A failed source cannot advance the derivative of the published value.
pub(super) struct SampleTime {
    elapsed: Duration,
    pub(super) local_elapsed: Duration,
    pending_rate: Option<PlaybackRate>,
}

impl SampleTime {
    pub(super) fn capture(inner: &AnimationControllerInner, elapsed: Duration) -> Self {
        let span = elapsed.saturating_sub(inner.rate_epoch_elapsed);
        let scaled = if inner.playback_rate == PlaybackRate::NORMAL {
            span
        } else if inner.playback_rate.is_paused() {
            Duration::ZERO
        } else {
            Duration::try_from_secs_f64(span.as_secs_f64() * inner.playback_rate.get())
                .unwrap_or(Duration::MAX)
        };
        Self {
            elapsed,
            local_elapsed: inner.rate_epoch_local.saturating_add(scaled),
            pending_rate: inner.pending_rate,
        }
    }

    pub(super) fn commit(&self, inner: &mut AnimationControllerInner) {
        inner.last_elapsed = self.elapsed;
        inner.local_elapsed = self.local_elapsed;
        if let Some(rate) = self.pending_rate {
            inner.rate_epoch_elapsed = self.elapsed;
            inner.rate_epoch_local = self.local_elapsed;
            inner.playback_rate = rate;
            // A source may have requested a newer rate while it was sampled.
            if inner.pending_rate == self.pending_rate {
                inner.pending_rate = None;
            }
        }
    }
}
