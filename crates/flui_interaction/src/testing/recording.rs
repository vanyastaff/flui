//! Gesture recording and replay for testing
//!
//! This module provides utilities for recording pointer event sequences
//! and replaying them for deterministic testing of gesture recognizers.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recording::{GestureRecorder, GesturePlayer};
//! use flui_interaction::prelude::*;
//!
//! // Record a gesture
//! let mut recorder = GestureRecorder::new();
//! recorder.record_down(PointerId::new(0), Offset::new(Pixels(100.0), Pixels(100.0)));
//! recorder.record_move(PointerId::new(0), Offset::new(Pixels(150.0), Pixels(100.0)));
//! recorder.record_up(PointerId::new(0), Offset::new(Pixels(200.0), Pixels(100.0)));
//!
//! // Save/export the recording
//! let recording = recorder.finish();
//!
//! // Replay the gesture
//! let player = GesturePlayer::new(recording);
//! for event in player {
//!     recognizer.handle_event(&event);
//! }
//! ```

use crate::events::{PointerEvent, PointerEventData, PointerType};
use flui_types::geometry::Pixels;

use crate::ids::PointerId;
use flui_types::Offset;
use std::time::{Duration, Instant};
use ui_events::pointer::{PointerButton, PointerButtons};

/// A recorded pointer event with timing information
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    /// Time offset from start of recording
    pub time_offset: Duration,
    /// The pointer ID
    pub pointer: PointerId,
    /// The event type
    pub event_type: RecordedEventType,
    /// Position of the event
    pub position: Offset<Pixels>,
    /// Device kind
    pub device_kind: PointerType,
    /// Pressure (if available)
    pub pressure: Option<f32>,
    /// Tilt X (if available)
    pub tilt_x: Option<f32>,
    /// Tilt Y (if available)
    pub tilt_y: Option<f32>,
    /// Rotation (if available)
    pub rotation: Option<f32>,
}

/// Type of recorded event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedEventType {
    /// Pointer down
    Down,
    /// Pointer move
    Move,
    /// Pointer up
    Up,
    /// Pointer cancel
    Cancel,
}

impl RecordedEvent {
    /// Create a new recorded event
    pub fn new(
        time_offset: Duration,
        pointer: PointerId,
        event_type: RecordedEventType,
        position: Offset<Pixels>,
    ) -> Self {
        Self {
            time_offset,
            pointer,
            event_type,
            position,
            device_kind: PointerType::Touch,
            pressure: None,
            tilt_x: None,
            tilt_y: None,
            rotation: None,
        }
    }

    /// Set device kind
    pub fn with_device_kind(mut self, kind: PointerType) -> Self {
        self.device_kind = kind;
        self
    }

    /// Set pressure
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = Some(pressure);
        self
    }

    /// Set tilt
    pub fn with_tilt(mut self, tilt_x: f32, tilt_y: f32) -> Self {
        self.tilt_x = Some(tilt_x);
        self.tilt_y = Some(tilt_y);
        self
    }

    /// Set rotation
    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = Some(rotation);
        self
    }

    /// Convert to a PointerEventData (for internal use)
    fn to_pointer_event_data(&self) -> PointerEventData {
        let pressure = self.pressure.unwrap_or(1.0);

        PointerEventData {
            position: self.position,
            local_position: self.position,
            device_kind: self.device_kind,
            device: self.pointer.get(),
            buttons: if matches!(
                self.event_type,
                RecordedEventType::Down | RecordedEventType::Move
            ) {
                PointerButtons::from(PointerButton::Primary)
            } else {
                PointerButtons::new()
            },
            pressure,
            time_stamp: self.time_offset.as_nanos() as u64,
        }
    }

    /// Convert to a PointerEvent
    pub fn to_pointer_event(&self) -> PointerEvent {
        let data = self.to_pointer_event_data();
        crate::events::make_pointer_event(self.event_type.into(), data)
    }
}

impl From<RecordedEventType> for crate::events::PointerEventKind {
    fn from(event_type: RecordedEventType) -> Self {
        match event_type {
            RecordedEventType::Down => crate::events::PointerEventKind::Down,
            RecordedEventType::Move => crate::events::PointerEventKind::Move,
            RecordedEventType::Up => crate::events::PointerEventKind::Up,
            RecordedEventType::Cancel => crate::events::PointerEventKind::Cancel,
        }
    }
}

/// A complete gesture recording
#[derive(Debug, Clone, Default)]
pub struct GestureRecording {
    /// Name/description of the recording
    pub name: String,
    /// List of recorded events
    pub events: Vec<RecordedEvent>,
    /// Total duration of the recording
    pub duration: Duration,
}

impl GestureRecording {
    /// Create a new empty recording
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a recording with a name
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            events: Vec::new(),
            duration: Duration::ZERO,
        }
    }

    /// Get the number of events
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the recording is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Add an event to the recording
    pub fn push(&mut self, event: RecordedEvent) {
        if event.time_offset > self.duration {
            self.duration = event.time_offset;
        }
        self.events.push(event);
    }

    /// Iterate over events
    pub fn iter(&self) -> impl Iterator<Item = &RecordedEvent> {
        self.events.iter()
    }
}

/// Records pointer events for later playback
#[derive(Debug)]
pub struct GestureRecorder {
    /// The recording being built
    recording: GestureRecording,
    /// Start time of the recording
    start_time: Option<Instant>,
    /// Device kind to use for all events
    device_kind: PointerType,
}

impl GestureRecorder {
    /// Create a new recorder
    pub fn new() -> Self {
        Self {
            recording: GestureRecording::new(),
            start_time: None,
            device_kind: PointerType::Touch,
        }
    }

    /// Create a recorder with a name
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            recording: GestureRecording::with_name(name),
            start_time: None,
            device_kind: PointerType::Touch,
        }
    }

    /// Set the device kind for all subsequent events
    pub fn set_device_kind(&mut self, kind: PointerType) {
        self.device_kind = kind;
    }

    /// Get the current time offset from start
    fn time_offset(&mut self) -> Duration {
        let now = Instant::now();
        match self.start_time {
            Some(start) => now.duration_since(start),
            None => {
                self.start_time = Some(now);
                Duration::ZERO
            }
        }
    }

    /// Record a pointer down event
    pub fn record_down(&mut self, pointer: PointerId, position: Offset<Pixels>) {
        let time_offset = self.time_offset();
        let event = RecordedEvent::new(time_offset, pointer, RecordedEventType::Down, position)
            .with_device_kind(self.device_kind);
        self.recording.push(event);
    }

    /// Record a pointer move event
    pub fn record_move(&mut self, pointer: PointerId, position: Offset<Pixels>) {
        let time_offset = self.time_offset();
        let event = RecordedEvent::new(time_offset, pointer, RecordedEventType::Move, position)
            .with_device_kind(self.device_kind);
        self.recording.push(event);
    }

    /// Record a pointer up event
    pub fn record_up(&mut self, pointer: PointerId, position: Offset<Pixels>) {
        let time_offset = self.time_offset();
        let event = RecordedEvent::new(time_offset, pointer, RecordedEventType::Up, position)
            .with_device_kind(self.device_kind);
        self.recording.push(event);
    }

    /// Record a pointer cancel event
    pub fn record_cancel(&mut self, pointer: PointerId, position: Offset<Pixels>) {
        let time_offset = self.time_offset();
        let event = RecordedEvent::new(time_offset, pointer, RecordedEventType::Cancel, position)
            .with_device_kind(self.device_kind);
        self.recording.push(event);
    }

    /// Record a raw PointerEvent
    pub fn record_event(&mut self, event: &PointerEvent) {
        let time_offset = self.time_offset();

        // Extract event info using pattern matching
        let (event_type, position, device_kind, pressure) = match event {
            PointerEvent::Down(data) => {
                let pos = data.state.position;
                (
                    RecordedEventType::Down,
                    Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32)),
                    data.pointer.pointer_type,
                    Some(data.state.pressure),
                )
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                (
                    RecordedEventType::Up,
                    Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32)),
                    data.pointer.pointer_type,
                    Some(data.state.pressure),
                )
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                (
                    RecordedEventType::Move,
                    Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32)),
                    data.pointer.pointer_type,
                    Some(data.current.pressure),
                )
            }
            PointerEvent::Cancel(info) => (
                RecordedEventType::Cancel,
                Offset::ZERO,
                info.pointer_type,
                None,
            ),
            _ => return, // Skip Enter, Leave, Scroll, Gesture events
        };

        let mut recorded = RecordedEvent::new(
            time_offset,
            PointerId::new(0), // Default to primary pointer
            event_type,
            position,
        )
        .with_device_kind(device_kind);

        if let Some(p) = pressure {
            recorded = recorded.with_pressure(p);
        }

        self.recording.push(recorded);
    }

    /// Finish recording and return the completed recording
    pub fn finish(mut self) -> GestureRecording {
        if let Some(last_event) = self.recording.events.last() {
            self.recording.duration = last_event.time_offset;
        }
        self.recording
    }

    /// Get a reference to the current recording
    pub fn recording(&self) -> &GestureRecording {
        &self.recording
    }
}

impl Default for GestureRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Plays back a recorded gesture
#[derive(Debug, Clone)]
pub struct GesturePlayer {
    /// The recording to play
    recording: GestureRecording,
    /// Current index in the recording
    current_index: usize,
}

impl GesturePlayer {
    /// Create a new player for the given recording
    pub fn new(recording: GestureRecording) -> Self {
        Self {
            recording,
            current_index: 0,
        }
    }

    /// Reset the player to the beginning
    pub fn reset(&mut self) {
        self.current_index = 0;
    }

    /// Get the next event without advancing
    pub fn peek(&self) -> Option<&RecordedEvent> {
        self.recording.events.get(self.current_index)
    }

    /// Get the next event and advance
    pub fn next_event(&mut self) -> Option<&RecordedEvent> {
        let event = self.recording.events.get(self.current_index);
        if event.is_some() {
            self.current_index += 1;
        }
        event
    }

    /// Get the next PointerEvent and advance
    pub fn next_pointer_event(&mut self) -> Option<PointerEvent> {
        self.next_event().map(|e| e.to_pointer_event())
    }

    /// Check if there are more events
    pub fn has_more(&self) -> bool {
        self.current_index < self.recording.events.len()
    }

    /// Get the total number of events
    pub fn len(&self) -> usize {
        self.recording.events.len()
    }

    /// Check if the recording is empty
    pub fn is_empty(&self) -> bool {
        self.recording.events.is_empty()
    }

    /// Get the current position in the recording
    pub fn position(&self) -> usize {
        self.current_index
    }

    /// Get the underlying recording
    pub fn recording(&self) -> &GestureRecording {
        &self.recording
    }

    /// Collect all events as PointerEvents
    pub fn all_events(&self) -> Vec<PointerEvent> {
        self.recording
            .events
            .iter()
            .map(|e| e.to_pointer_event())
            .collect()
    }
}

impl Iterator for GesturePlayer {
    type Item = PointerEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_pointer_event()
    }
}

// ============================================================================
// Gesture Builders - Pre-built gesture patterns for testing
// ============================================================================

/// Utility for building common gesture patterns
pub struct GestureBuilder;

impl GestureBuilder {
    /// Create a simple tap gesture
    pub fn tap(position: Offset<Pixels>) -> GestureRecording {
        let mut recording = GestureRecording::with_name("tap");
        let pointer = PointerId::new(0);

        recording.push(RecordedEvent::new(
            Duration::ZERO,
            pointer,
            RecordedEventType::Down,
            position,
        ));
        recording.push(RecordedEvent::new(
            Duration::from_millis(50),
            pointer,
            RecordedEventType::Up,
            position,
        ));

        recording
    }

    /// Create a double tap gesture
    pub fn double_tap(position: Offset<Pixels>) -> GestureRecording {
        let mut recording = GestureRecording::with_name("double_tap");
        let pointer = PointerId::new(0);

        // First tap
        recording.push(RecordedEvent::new(
            Duration::ZERO,
            pointer,
            RecordedEventType::Down,
            position,
        ));
        recording.push(RecordedEvent::new(
            Duration::from_millis(50),
            pointer,
            RecordedEventType::Up,
            position,
        ));

        // Second tap
        recording.push(RecordedEvent::new(
            Duration::from_millis(150),
            pointer,
            RecordedEventType::Down,
            position,
        ));
        recording.push(RecordedEvent::new(
            Duration::from_millis(200),
            pointer,
            RecordedEventType::Up,
            position,
        ));

        recording
    }

    /// Create a long press gesture
    pub fn long_press(position: Offset<Pixels>, duration_ms: u64) -> GestureRecording {
        let mut recording = GestureRecording::with_name("long_press");
        let pointer = PointerId::new(0);

        recording.push(RecordedEvent::new(
            Duration::ZERO,
            pointer,
            RecordedEventType::Down,
            position,
        ));
        recording.push(RecordedEvent::new(
            Duration::from_millis(duration_ms),
            pointer,
            RecordedEventType::Up,
            position,
        ));

        recording
    }

    /// Create a horizontal drag gesture
    pub fn horizontal_drag(
        start: Offset<Pixels>,
        end: Offset<Pixels>,
        steps: usize,
    ) -> GestureRecording {
        Self::drag(start, end, steps, "horizontal_drag")
    }

    /// Create a vertical drag gesture
    pub fn vertical_drag(
        start: Offset<Pixels>,
        end: Offset<Pixels>,
        steps: usize,
    ) -> GestureRecording {
        Self::drag(start, end, steps, "vertical_drag")
    }

    /// Create a drag gesture with intermediate steps
    pub fn drag(
        start: Offset<Pixels>,
        end: Offset<Pixels>,
        steps: usize,
        name: &str,
    ) -> GestureRecording {
        let mut recording = GestureRecording::with_name(name);
        let pointer = PointerId::new(0);

        // Down
        recording.push(RecordedEvent::new(
            Duration::ZERO,
            pointer,
            RecordedEventType::Down,
            start,
        ));

        // Intermediate moves
        let steps = steps.max(1);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let pos = Offset::new(
                start.dx + (end.dx - start.dx) * t,
                start.dy + (end.dy - start.dy) * t,
            );
            recording.push(RecordedEvent::new(
                Duration::from_millis(16 * i as u64), // ~60fps
                pointer,
                RecordedEventType::Move,
                pos,
            ));
        }

        // Up
        recording.push(RecordedEvent::new(
            Duration::from_millis(16 * (steps + 1) as u64),
            pointer,
            RecordedEventType::Up,
            end,
        ));

        recording
    }

    /// Create a pinch/scale gesture with two fingers
    pub fn pinch(
        center: Offset<Pixels>,
        start_distance: f32,
        end_distance: f32,
        steps: usize,
    ) -> GestureRecording {
        let mut recording = GestureRecording::with_name("pinch");
        let pointer1 = PointerId::new(0);
        let pointer2 = PointerId::new(1);

        let steps = steps.max(1);

        // Calculate start positions
        let start_offset = start_distance / 2.0;
        let start1 = Offset::new(center.dx - Pixels(start_offset), center.dy);
        let start2 = Offset::new(center.dx + Pixels(start_offset), center.dy);

        // Down for both fingers
        recording.push(RecordedEvent::new(
            Duration::ZERO,
            pointer1,
            RecordedEventType::Down,
            start1,
        ));
        recording.push(RecordedEvent::new(
            Duration::from_millis(10),
            pointer2,
            RecordedEventType::Down,
            start2,
        ));

        // Intermediate moves
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let current_distance = start_distance + (end_distance - start_distance) * t;
            let offset = current_distance / 2.0;

            let pos1 = Offset::new(center.dx - Pixels(offset), center.dy);
            let pos2 = Offset::new(center.dx + Pixels(offset), center.dy);

            let time = Duration::from_millis(20 + 16 * i as u64);
            recording.push(RecordedEvent::new(
                time,
                pointer1,
                RecordedEventType::Move,
                pos1,
            ));
            recording.push(RecordedEvent::new(
                time,
                pointer2,
                RecordedEventType::Move,
                pos2,
            ));
        }

        // Up for both fingers
        let end_offset = end_distance / 2.0;
        let end1 = Offset::new(center.dx - Pixels(end_offset), center.dy);
        let end2 = Offset::new(center.dx + Pixels(end_offset), center.dy);

        let final_time = Duration::from_millis(20 + 16 * (steps + 1) as u64);
        recording.push(RecordedEvent::new(
            final_time,
            pointer1,
            RecordedEventType::Up,
            end1,
        ));
        recording.push(RecordedEvent::new(
            final_time,
            pointer2,
            RecordedEventType::Up,
            end2,
        ));

        recording
    }

    /// Create a swipe gesture (fast drag)
    pub fn swipe(start: Offset<Pixels>, end: Offset<Pixels>) -> GestureRecording {
        Self::drag(start, end, 5, "swipe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recorder_basic() {
        let mut recorder = GestureRecorder::new();
        let pointer = PointerId::new(0);
        let pos = Offset::new(Pixels(100.0), Pixels(100.0));

        recorder.record_down(pointer, pos);
        recorder.record_move(pointer, Offset::new(Pixels(110.0), Pixels(100.0)));
        recorder.record_up(pointer, Offset::new(Pixels(120.0), Pixels(100.0)));

        let recording = recorder.finish();

        assert_eq!(recording.len(), 3);
        assert_eq!(recording.events[0].event_type, RecordedEventType::Down);
        assert_eq!(recording.events[1].event_type, RecordedEventType::Move);
        assert_eq!(recording.events[2].event_type, RecordedEventType::Up);
    }

    #[test]
    fn test_player_iteration() {
        let recording = GestureBuilder::tap(Offset::new(Pixels(50.0), Pixels(50.0)));
        let player = GesturePlayer::new(recording);

        let events: Vec<_> = player.collect();
        assert_eq!(events.len(), 2);

        // First should be Down
        assert!(matches!(events[0], PointerEvent::Down(_)));
        // Second should be Up
        assert!(matches!(events[1], PointerEvent::Up(_)));
    }

    #[test]
    fn test_player_reset() {
        let recording = GestureBuilder::tap(Offset::new(Pixels(0.0), Pixels(0.0)));
        let mut player = GesturePlayer::new(recording);

        // Consume all events
        while player.next_event().is_some() {}
        assert!(!player.has_more());

        // Reset and replay
        player.reset();
        assert!(player.has_more());
        assert_eq!(player.position(), 0);
    }

    #[test]
    fn test_double_tap_builder() {
        let recording = GestureBuilder::double_tap(Offset::new(Pixels(100.0), Pixels(100.0)));

        assert_eq!(recording.len(), 4);
        assert_eq!(recording.events[0].event_type, RecordedEventType::Down);
        assert_eq!(recording.events[1].event_type, RecordedEventType::Up);
        assert_eq!(recording.events[2].event_type, RecordedEventType::Down);
        assert_eq!(recording.events[3].event_type, RecordedEventType::Up);
    }

    #[test]
    fn test_drag_builder() {
        let recording = GestureBuilder::horizontal_drag(
            Offset::new(Pixels(0.0), Pixels(0.0)),
            Offset::new(Pixels(100.0), Pixels(0.0)),
            5,
        );

        // 1 down + 5 moves + 1 up = 7 events
        assert_eq!(recording.len(), 7);
        assert_eq!(recording.events[0].event_type, RecordedEventType::Down);
        assert_eq!(recording.events[6].event_type, RecordedEventType::Up);

        // Check intermediate positions
        let mid_event = &recording.events[3];
        assert_eq!(mid_event.event_type, RecordedEventType::Move);
        // Position should be around 60% of the way (event 3 of 5 moves)
        assert!(mid_event.position.dx > Pixels(50.0) && mid_event.position.dx < Pixels(70.0));
    }

    #[test]
    fn test_pinch_builder() {
        let center = Offset::new(Pixels(200.0), Pixels(200.0));
        let recording = GestureBuilder::pinch(center, 100.0, 200.0, 5);

        // 2 downs + 5*2 moves + 2 ups = 14 events
        assert_eq!(recording.len(), 14);

        // First two should be downs for different pointers
        assert_eq!(recording.events[0].pointer, PointerId::new(0));
        assert_eq!(recording.events[1].pointer, PointerId::new(1));
    }

    #[test]
    fn test_recorded_event_with_pressure() {
        let event = RecordedEvent::new(
            Duration::ZERO,
            PointerId::new(0),
            RecordedEventType::Down,
            Offset::new(Pixels(0.0), Pixels(0.0)),
        )
        .with_pressure(0.5)
        .with_device_kind(PointerType::Pen);

        let pointer_event = event.to_pointer_event();

        match pointer_event {
            PointerEvent::Down(data) => {
                assert_eq!(data.pointer.pointer_type, PointerType::Pen);
                assert!((data.state.pressure - 0.5).abs() < 0.01);
            }
            _ => panic!("Expected Down event"),
        }
    }

    #[test]
    fn test_recording_name() {
        let recording = GestureRecording::with_name("test_gesture");
        assert_eq!(recording.name, "test_gesture");
    }

    #[test]
    fn test_player_all_events() {
        let recording = GestureBuilder::tap(Offset::new(Pixels(0.0), Pixels(0.0)));
        let player = GesturePlayer::new(recording);

        let events = player.all_events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_long_press_builder() {
        let recording = GestureBuilder::long_press(Offset::new(Pixels(50.0), Pixels(50.0)), 600);

        assert_eq!(recording.len(), 2);
        assert!(recording.duration >= Duration::from_millis(600));
    }

    #[test]
    fn test_swipe_builder() {
        let recording = GestureBuilder::swipe(
            Offset::new(Pixels(0.0), Pixels(0.0)),
            Offset::new(Pixels(300.0), Pixels(0.0)),
        );

        // Swipe is a fast drag with 5 steps
        assert_eq!(recording.len(), 7); // 1 down + 5 moves + 1 up
        assert_eq!(recording.name, "swipe");
    }
}
