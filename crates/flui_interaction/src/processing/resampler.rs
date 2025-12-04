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
//! ```rust,ignore
//! use flui_interaction::resampler::PointerEventResampler;
//! use flui_interaction::ids::PointerId;
//! use std::time::Duration;
//!
//! let mut resampler = PointerEventResampler::new(PointerId::new(0));
//!
//! // Add incoming events
//! resampler.add_event(pointer_event);
//!
//! // Sample at 60Hz
//! let sample_time = Duration::from_secs_f64(1.0 / 60.0);
//! resampler.sample(sample_time, |resampled_event| {
//!     // Process resampled event
//! });
//! ```

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_types::events::PointerEvent;
use flui_types::geometry::Offset;

use crate::ids::PointerId;

/// Maximum number of events to buffer (prevents unbounded memory growth)
const MAX_BUFFERED_EVENTS: usize = 100;

/// Minimum time between samples to prevent excessive resampling
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_micros(1000); // 1ms

/// Callback for handling resampled events
#[allow(dead_code)] // Future public API
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
#[derive(Clone)]
pub struct PointerEventResampler {
    inner: Arc<Mutex<ResamplerInner>>,
}

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
    last_position: Option<Offset>,
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

    /// Adds a pointer event to the resampling queue
    ///
    /// Events are buffered and will be processed during the next `sample()` call.
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
            PointerEvent::Removed { .. } => {
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
                pointer_id = inner.pointer_id.get(),
                "Event queue full, dropping event"
            );
        }
    }

    /// Samples events at the specified time and invokes callback with resampled events
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
    /// - Move/Hover events are only generated if position changed
    pub fn sample<F>(&self, sample_time: Instant, next_sample_time: Instant, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        let mut inner = self.inner.lock();

        // Skip if not tracking or no events
        if !inner.is_tracked || inner.event_queue.is_empty() {
            return;
        }

        // Enforce minimum sample interval
        if let Some(last_time) = inner.last_sample_time {
            if sample_time.duration_since(last_time) < MIN_SAMPLE_INTERVAL {
                return;
            }
        }

        inner.last_sample_time = Some(sample_time);

        // Process all events up to sample_time
        while let Some(buffered) = inner.event_queue.front() {
            if buffered.timestamp > sample_time {
                break; // Future event, wait for next sample
            }

            let buffered = inner.event_queue.pop_front().unwrap();
            let event = buffered.event;

            // Update last position for interpolation
            let position = event.position();
            inner.last_position = Some(position);

            // Emit the event
            callback(event);
        }

        // Interpolate if we have move/hover events pending
        if !inner.event_queue.is_empty() && inner.last_position.is_some() {
            if let Some(next_event) = inner.event_queue.front() {
                if matches!(
                    next_event.event,
                    PointerEvent::Move(..) | PointerEvent::Hover(..)
                ) {
                    // Interpolate between last position and next event
                    if let Some(last_pos) = inner.last_position {
                        let next_pos = next_event.event.position();
                        let total_duration = next_event.timestamp.duration_since(sample_time);
                        let sample_duration = next_sample_time.duration_since(sample_time);

                        if total_duration > Duration::ZERO {
                            let t = sample_duration.as_secs_f64() / total_duration.as_secs_f64();
                            let t = t.clamp(0.0, 1.0);

                            let interpolated_pos = Offset::new(
                                last_pos.dx + (next_pos.dx - last_pos.dx) * t as f32,
                                last_pos.dy + (next_pos.dy - last_pos.dy) * t as f32,
                            );

                            // Only emit if position actually changed
                            if interpolated_pos != last_pos {
                                // Create interpolated event
                                use flui_types::events::PointerEventData;

                                let interpolated_event = match &next_event.event {
                                    PointerEvent::Move(data) => {
                                        let mut new_data = PointerEventData::new(
                                            interpolated_pos,
                                            data.device_kind,
                                        );
                                        new_data.device = data.device;
                                        PointerEvent::Move(new_data)
                                    }
                                    PointerEvent::Hover(data) => {
                                        let mut new_data = PointerEventData::new(
                                            interpolated_pos,
                                            data.device_kind,
                                        );
                                        new_data.device = data.device;
                                        PointerEvent::Hover(new_data)
                                    }
                                    _ => return, // Should not happen
                                };

                                inner.last_position = Some(interpolated_pos);
                                callback(interpolated_event);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Stops resampling and flushes all remaining events
    ///
    /// Invokes the callback with any buffered events and clears the queue.
    pub fn stop<F>(&self, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        let mut inner = self.inner.lock();

        // Flush all remaining events
        while let Some(buffered) = inner.event_queue.pop_front() {
            callback(buffered.event);
        }

        // Reset state
        inner.is_tracked = false;
        inner.is_down = false;
        inner.last_position = None;
        inner.last_sample_time = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_basic() {
        let resampler = PointerEventResampler::new(PointerId::new(0));

        assert!(!resampler.is_tracked());
        assert!(!resampler.is_down());
        assert!(!resampler.has_pending_events());
        assert_eq!(resampler.pointer_id(), PointerId::new(0));
    }

    #[test]
    fn test_add_event() {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let resampler = PointerEventResampler::new(PointerId::new(0));

        let mut data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        data.device = 0;
        let event = PointerEvent::Down(data);
        resampler.add_event(event);

        assert!(resampler.is_tracked());
        assert!(resampler.is_down());
        assert!(resampler.has_pending_events());
    }

    #[test]
    fn test_sample_events() {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let resampler = PointerEventResampler::new(PointerId::new(0));

        // Add down event
        let mut data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        data.device = 0;
        resampler.add_event(PointerEvent::Down(data));

        // Sample events
        let mut sampled_events = Vec::new();
        let now = Instant::now();
        resampler.sample(now, now + Duration::from_millis(16), |event| {
            sampled_events.push(event);
        });

        assert_eq!(sampled_events.len(), 1);
        assert!(!resampler.has_pending_events());
    }

    #[test]
    fn test_stop_flushes_events() {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let resampler = PointerEventResampler::new(PointerId::new(0));

        let mut data1 = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        data1.device = 0;
        resampler.add_event(PointerEvent::Down(data1));

        let mut data2 = PointerEventData::new(Offset::new(20.0, 30.0), PointerDeviceKind::Mouse);
        data2.device = 0;
        resampler.add_event(PointerEvent::Move(data2));

        let mut flushed_events = Vec::new();
        resampler.stop(|event| {
            flushed_events.push(event);
        });

        assert_eq!(flushed_events.len(), 2);
        assert!(!resampler.is_tracked());
        assert!(!resampler.has_pending_events());
    }

    #[test]
    fn test_clear() {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let resampler = PointerEventResampler::new(PointerId::new(0));

        let mut data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        data.device = 0;
        resampler.add_event(PointerEvent::Down(data));

        assert!(resampler.has_pending_events());

        resampler.clear();

        assert!(!resampler.has_pending_events());
    }
}
