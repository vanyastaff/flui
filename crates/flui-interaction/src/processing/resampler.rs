//! Event resampler for smooth pointer event processing
//!
//! The resampler enables smoother touch/pointer event processing by:
//! - Buffering incoming pointer events
//! - Resampling at a caller-determined frequency
//! - Interpolating positions between events for smooth motion
//! - Removing duplicate events
//!
//! This is particularly beneficial for:
//! - Devices with low-frequency sensors
//! - Mismatched input/display refresh rates (e.g., 120Hz input, 90Hz display)
//! - High-precision stylus input
//!
//! # Architecture
//!
//! ```text
//! Platform Events → Resampler → Resampled Events → GestureRecognizers
//!                      ↓
//!                 Event Queue
//!                      ↓
//!              Interpolation Logic
//! ```
//!
//! # Type System Features
//!
//! - **Newtype pattern**: Uses `PointerId` for type-safe pointer identification
//!
//! # Example
//!
//! ```rust
//! use std::time::{Duration, Instant};
//!
//! use flui_interaction::ids::PointerId;
//! use flui_interaction::processing::PointerEventResampler;
//!
//! let resampler = PointerEventResampler::new(PointerId::PRIMARY);
//! // The 60 Hz frame tick. The resampler enforces a 1 ms minimum
//! // interval between samples, so back-to-back ticks coalesce.
//! let now = Instant::now();
//! let next = now + Duration::from_millis(16);
//! resampler.sample(now, next, |_event| {
//!     // dispatch to recognisers here
//! });
//! let _has_pending = resampler.has_pending_events();
//! ```

use std::{collections::VecDeque, sync::Arc};

use web_time::{Duration, Instant};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;
use smallvec::SmallVec;

use crate::{
    events::{PointerEvent, PointerEventExt},
    ids::PointerId,
};

/// Recommended lookback from the presentation/frame time to the resample
/// target: `sample_time = frame_time - DEFAULT_RESAMPLE_LOOKBACK`.
///
/// 38 ms is Flutter's `GestureBinding.samplingOffset` default
/// (`gestures/binding.dart`, `_defaultSamplingOffset = -38 ms`): two 60 Hz
/// frames of worst-case input latency (33.3 ms) plus a 4.7 ms input-driver
/// margin. On 120 Hz input/display pipelines a ~22 ms lookback is
/// appropriate instead — pass your own offset to [`PointerEventResampler::sample`].
pub const DEFAULT_RESAMPLE_LOOKBACK: Duration = Duration::from_millis(38);

/// Maximum number of events to buffer (prevents unbounded memory growth)
const MAX_BUFFERED_EVENTS: usize = 100;

/// Minimum time between samples to prevent excessive resampling
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(1); // 1ms

/// Callback for handling resampled events
#[expect(dead_code)] // Future public API
pub type HandleEventCallback = Box<dyn FnMut(PointerEvent) + Send>;

/// Buffered pointer event with timestamp
#[derive(Debug, Clone)]
struct BufferedEvent {
    /// The pointer event
    event: PointerEvent,
    /// Time when the event was received
    timestamp: Instant,
}

/// Pointer event resampler for smooth motion
///
/// Maintains a queue of pointer events and generates resampled events
/// at caller-determined frequencies for smoother gesture recognition.
///
/// # Thread Safety
///
/// This type is thread-safe using `Arc<Mutex<_>>` internally.
#[derive(Debug, Clone)]
pub struct PointerEventResampler {
    inner: Arc<Mutex<ResamplerInner>>,
}

#[derive(Debug)]
struct ResamplerInner {
    /// Pointer ID this resampler tracks
    pointer_id: PointerId,
    /// Queue of buffered events
    event_queue: VecDeque<BufferedEvent>,
    /// Whether the pointer is currently down
    is_down: bool,
    /// Whether the pointer is being tracked
    is_tracked: bool,
    /// Last sampled position (for interpolation)
    last_position: Option<Offset<f64>>,
    /// Last sample time
    last_sample_time: Option<Instant>,
}

impl PointerEventResampler {
    /// Creates a new resampler for the given pointer ID
    pub fn new(pointer_id: PointerId) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ResamplerInner {
                pointer_id,
                event_queue: VecDeque::with_capacity(16),
                is_down: false,
                is_tracked: false,
                last_position: None,
                last_sample_time: None,
            })),
        }
    }

    /// Marks the pointer as down and tracked WITHOUT queueing an event.
    ///
    /// The binding dispatches the `Down` event directly (synchronously, to
    /// close the arena and cache the hit test), so the resampler must not also
    /// re-emit it on the next `sample()`. This establishes the tracking state
    /// that `sample()` requires (`is_tracked`) so subsequent moves are paced
    /// instead of silently dropped.
    pub fn start_tracking(&self) {
        let mut inner = self.inner.lock();
        inner.is_down = true;
        inner.is_tracked = true;
    }

    /// Adds a pointer event to the resampling queue
    ///
    /// Events are buffered and will be processed during the next `sample()`
    /// call.
    pub fn add_event(&self, event: PointerEvent) {
        let mut inner = self.inner.lock();

        // Update tracking state
        match &event {
            PointerEvent::Down(..) => {
                inner.is_down = true;
                inner.is_tracked = true;
            }
            PointerEvent::Up(..) | PointerEvent::Cancel(..) => {
                inner.is_down = false;
            }
            PointerEvent::Leave(..) => {
                inner.is_tracked = false;
            }
            _ => {}
        }

        // Add to queue (with size limit)
        if inner.event_queue.len() < MAX_BUFFERED_EVENTS {
            inner.event_queue.push_back(BufferedEvent {
                event,
                timestamp: Instant::now(),
            });
        } else {
            tracing::warn!(
                pointer_id = ?inner.pointer_id,
                "Event queue full, dropping event"
            );
        }
    }

    /// Samples events at the specified time and invokes callback with resampled
    /// events
    ///
    /// # Arguments
    ///
    /// * `sample_time` - Current sample time (typically current frame time)
    /// * `next_sample_time` - Next expected sample time (for interpolation)
    /// * `callback` - Function to call with each resampled event
    ///
    /// # Resampling Strategy
    ///
    /// - Events are sorted by timestamp
    /// - Duplicate positions are removed
    /// - Positions are interpolated for smooth motion
    /// - Move events are only generated if position changed
    pub fn sample<F>(&self, sample_time: Instant, next_sample_time: Instant, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        let Some(sample_duration) = next_sample_time.checked_duration_since(sample_time) else {
            tracing::warn!("ignoring a non-advancing pointer sampling window");
            return;
        };
        if sample_duration.is_zero() {
            tracing::warn!("ignoring a zero-width pointer sampling window");
            return;
        }

        let emitted = {
            let mut inner = self.inner.lock();

            // Skip if not tracking or no events
            if !inner.is_tracked || inner.event_queue.is_empty() {
                return;
            }

            // Enforce minimum sample interval
            if let Some(last_time) = inner.last_sample_time {
                let Some(elapsed) = sample_time.checked_duration_since(last_time) else {
                    tracing::warn!(
                        pointer_id = ?inner.pointer_id,
                        "ignoring a regressed pointer sample time"
                    );
                    return;
                };
                if elapsed < MIN_SAMPLE_INTERVAL {
                    return;
                }
            }

            inner.last_sample_time = Some(sample_time);
            let mut emitted = SmallVec::<[PointerEvent; 4]>::new();

            // Process all events up to sample_time
            while let Some(front) = inner.event_queue.front() {
                if front.timestamp > sample_time {
                    break; // Future event, wait for next sample
                }

                // The queue cannot change between `front` and `pop_front`
                // while the state lock is held.
                let buffered = inner
                    .event_queue
                    .pop_front()
                    .expect("event_queue front returned Some in the loop guard");
                let event = buffered.event;

                // Update last position for interpolation
                let position = event.position();
                inner.last_position = Some(position);

                emitted.push(event);
            }

            // Interpolate if we have move events pending (Flutter parity:
            // `resampler.dart _samplePointerPosition` synthesizes a Move at
            // the interpolated position).
            if !inner.event_queue.is_empty()
                && inner.last_position.is_some()
                && let Some(next_event) = inner.event_queue.front()
                && matches!(next_event.event, PointerEvent::Move(..))
                && let Some(last_pos) = inner.last_position
            {
                let next_pos = next_event.event.position();
                let total_duration = next_event.timestamp.duration_since(sample_time);
                if total_duration > Duration::ZERO {
                    let t = sample_duration.as_secs_f64() / total_duration.as_secs_f64();
                    let t = t.clamp(0.0, 1.0);

                    let interpolated_pos = Offset::new(
                        last_pos.dx + (next_pos.dx - last_pos.dx) * t,
                        last_pos.dy + (next_pos.dy - last_pos.dy) * t,
                    );

                    // Only emit if position actually changed
                    if interpolated_pos != last_pos {
                        // Synthesize the interpolated Move from the pending
                        // event: same pointer/buttons/pressure state, position
                        // replaced.
                        let mut interpolated = next_event.event.clone();
                        if let PointerEvent::Move(update) = &mut interpolated {
                            update.current.position = dpi::PhysicalPosition::new(
                                interpolated_pos.dx,
                                interpolated_pos.dy,
                            );
                        }
                        inner.last_position = Some(interpolated_pos);
                        emitted.push(interpolated);
                    }
                }
            }

            emitted
        };

        // User dispatch happens only after the state transaction is complete.
        // Re-entrant callbacks may therefore add/clear/stop without deadlocking
        // or mutating the batch currently being emitted.
        for event in emitted {
            callback(event);
        }
    }

    /// Stops resampling and flushes all remaining events
    ///
    /// Invokes the callback with any buffered events and clears the queue.
    pub fn stop<F>(&self, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        let emitted: SmallVec<[PointerEvent; 4]> = {
            let mut inner = self.inner.lock();
            let emitted = inner
                .event_queue
                .drain(..)
                .map(|buffered| buffered.event)
                .collect();

            // Reset state before callbacks can re-enter this resampler.
            inner.is_tracked = false;
            inner.is_down = false;
            inner.last_position = None;
            inner.last_sample_time = None;
            emitted
        };

        for event in emitted {
            callback(event);
        }
    }

    /// Checks if the pointer is currently down
    pub fn is_down(&self) -> bool {
        self.inner.lock().is_down
    }

    /// Checks if the pointer is being tracked
    pub fn is_tracked(&self) -> bool {
        self.inner.lock().is_tracked
    }

    /// Checks if there are pending events in the queue
    pub fn has_pending_events(&self) -> bool {
        !self.inner.lock().event_queue.is_empty()
    }

    /// Number of input events waiting for a sampling window.
    #[inline]
    #[must_use]
    pub fn pending_event_count(&self) -> usize {
        self.inner.lock().event_queue.len()
    }

    /// Returns the pointer ID this resampler tracks
    pub fn pointer_id(&self) -> PointerId {
        self.inner.lock().pointer_id
    }

    /// Clears all buffered events
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.event_queue.clear();
        inner.last_position = None;
    }
}
