//! Open owner-local gesture recognition and event timing.
use crate::{
    arena::GestureArenaMember,
    events::{PointerButton, PointerEvent},
    routing::{PointerDispatch, RoutePanic},
};
use flui_foundation::geometry::Offset;
use web_time::{Duration, Instant};

/// A recognizer admits Down and receives the remaining pointer stream.
/// Strong ownership lives in widget state; arena and attachments use weak references.
pub trait GestureRecognizer: GestureArenaMember {
    /// Admit a Down, committing membership before invoking user callbacks.
    fn add_pointer(&self, down: PointerDispatch<'_>);
    /// Process a dispatch in local and root coordinates.
    fn handle_event(&self, dispatch: PointerDispatch<'_>);
    /// Cancel current work; repeated cancellation is idle.
    fn cancel(&self) -> CancelOutcome;
}

/// Whether cancellation withdrew active recognition work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelOutcome {
    /// No active recognition work remained.
    Idle,
    /// Active recognition work was cancelled.
    Cancelled,
}

/// Cancel every supplied recognizer before resuming the first callback failure.
pub fn cancel_all<'a>(
    recognizers: impl IntoIterator<Item = &'a dyn GestureRecognizer>,
) -> CancelOutcome {
    let incoming_failure = std::thread::panicking();
    if incoming_failure {
        return CancelOutcome::Idle;
    }
    let mut first = None;
    let mut outcome = CancelOutcome::Idle;
    for recognizer in recognizers {
        let candidate = RoutePanic::capture(|| {
            if recognizer.cancel() == CancelOutcome::Cancelled {
                outcome = CancelOutcome::Cancelled;
            }
        });
        RoutePanic::preserve_first(&mut first, candidate, "recognizer cancellation");
    }
    super::callback_containment::finish_containment(first, incoming_failure);
    outcome
}

pub(crate) fn is_primary_down(event: &PointerEvent) -> bool {
    matches!(event, PointerEvent::Down(data) if data.button() == PointerButton::PRIMARY)
}

pub(crate) fn event_time(event: &PointerEvent) -> Option<u64> {
    crate::events::get_event_time(event).map(flui_platform_api::EventTime::as_nanos)
}

/// Historical local positions only. Geometry and callbacks still publish the
/// frame's current sample, while the tracker consumes every hardware timestamp.
pub(crate) fn motion_history(
    event: &PointerEvent,
) -> impl Iterator<Item = (Option<u64>, Offset<f64>)> + '_ {
    let samples = match event {
        PointerEvent::Move(movement) => movement.coalesced(),
        _ => &[],
    };
    samples.iter().map(|sample| {
        let position = sample.position.get();
        (
            Some(sample.time.as_nanos()),
            Offset::new(position.x, position.y),
        )
    })
}

/// Places device production timestamps on the arena clock.
///
/// Velocity uses sample production time, because queued events can arrive back
/// to back. An unstamped event uses dispatch time. When that dispatch time runs
/// ahead of the stamped timeline, the next stamp reanchors at the last instant;
/// later hardware samples retain their spacing without running backwards.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EventTimeline {
    anchor: Option<(u64, Instant)>,
    last: Option<Instant>,
}

impl EventTimeline {
    pub(crate) fn instant(&mut self, event_nanos: Option<u64>, now: Instant) -> Instant {
        let raw = match (event_nanos, self.anchor) {
            (None, _) => now,
            (Some(event_nanos), None) => {
                self.anchor = Some((event_nanos, now));
                now
            }
            (Some(event_nanos), Some((anchor_nanos, anchor))) => {
                let offset = Duration::from_nanos(event_nanos.saturating_sub(anchor_nanos));
                anchor.checked_add(offset).unwrap_or(now)
            }
        };
        let instant = self.last.map_or(raw, |last| last.max(raw));
        if instant != raw
            && let Some(event_nanos) = event_nanos
        {
            self.anchor = Some((event_nanos, instant));
        }
        self.last = Some(instant);
        instant
    }
}

/// Canonical recognition progress; concrete recognizers may keep richer states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GestureRecognizerState {
    /// Ready to admit a new sequence.
    #[default]
    Ready,
    /// Current pointer events remain consistent with recognition.
    Possible,
    /// Recognition is impossible until tracked pointers terminate.
    Defunct,
}

/// Default gesture tolerances.
pub mod constants {
    /// Maximum tap movement in logical pixels.
    pub const TAP_SLOP: f64 = 18.0;
    /// Maximum distance between taps in logical pixels.
    pub const DOUBLE_TAP_SLOP: f64 = 100.0;
    /// Maximum interval between taps in milliseconds.
    pub const DOUBLE_TAP_TIMEOUT_MS: u64 = 300;
    /// Long-press duration in milliseconds.
    pub const LONG_PRESS_DURATION_MS: u64 = 500;
    /// Drag movement tolerance in logical pixels.
    pub const DRAG_SLOP: f64 = 18.0;
    /// Pan movement tolerance in logical pixels.
    pub const PAN_SLOP: f64 = 18.0;
    /// Scale movement tolerance in logical pixels.
    pub const SCALE_SLOP: f64 = 18.0;
    /// Minimum fling velocity in logical pixels per second.
    pub const MIN_FLING_VELOCITY: f64 = 50.0;
    /// Minimum fling distance in logical pixels.
    pub const MIN_FLING_DISTANCE: f64 = 50.0;
}

#[cfg(test)]
mod event_timeline_tests {
    use super::EventTimeline;
    use std::time::{Duration, Instant};

    #[test]
    fn mixed_stamped_and_unstamped_events_stay_monotonic() {
        let start = Instant::now();
        let mut timeline = EventTimeline::default();
        assert_eq!(timeline.instant(Some(0), start), start);
        let unstamped = timeline.instant(None, start + Duration::from_millis(100));
        let stamped = timeline.instant(Some(10_000_000), start + Duration::from_millis(101));
        assert!(
            stamped >= unstamped,
            "{stamped:?} ran back before {unstamped:?}"
        );
        let next = timeline.instant(Some(20_000_000), start + Duration::from_millis(102));
        assert_eq!(
            next - stamped,
            Duration::from_millis(10),
            "the timeline did not collapse"
        );
    }
}
