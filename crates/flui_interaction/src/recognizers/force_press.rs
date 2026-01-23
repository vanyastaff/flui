//! Force press gesture recognizer
//!
//! Recognizes force press (3D Touch / Force Touch) gestures based on pressure.
//!
//! A force press is defined as:
//! - Pointer down with pressure sensing support
//! - Pressure increases past the start threshold (0.4 by default)
//! - Optional pressure updates as finger presses harder/softer
//! - Pressure decreases below end threshold or pointer up/cancel
//!
//! Flutter reference: https://api.flutter.dev/flutter/gestures/ForcePressGestureRecognizer-class.html

use super::recognizer::{GestureRecognizer, GestureRecognizerState};
use flui_types::geometry::Pixels;

use crate::arena::GestureArenaMember;
use crate::events::PointerEvent;
use crate::ids::PointerId;
use crate::settings::GestureSettings;
use flui_types::gestures::ForcePressDetails;
use flui_types::Offset;
use parking_lot::Mutex;
use std::sync::Arc;

/// Default pressure threshold to start force press (40%)
pub const FORCE_PRESS_START_PRESSURE: f32 = 0.4;

/// Default pressure threshold for peak force press (85%)
pub const FORCE_PRESS_PEAK_PRESSURE: f32 = 0.85;

/// Callback for force press start events
pub type ForcePressStartCallback = Arc<dyn Fn(ForcePressDetails) + Send + Sync>;

/// Callback for force press update events
pub type ForcePressUpdateCallback = Arc<dyn Fn(ForcePressDetails) + Send + Sync>;

/// Callback for force press peak events
pub type ForcePressPeakCallback = Arc<dyn Fn(ForcePressDetails) + Send + Sync>;

/// Callback for force press end events
pub type ForcePressEndCallback = Arc<dyn Fn(ForcePressDetails) + Send + Sync>;

/// Recognizes force press gestures based on pressure sensitivity
///
/// Force press detection requires a device that supports pressure sensing
/// (touch screens with 3D Touch, Force Touch trackpads, styluses with
/// pressure support).
///
/// # Pressure Thresholds
///
/// - **Start threshold** (default 0.4): Pressure level to begin force press
/// - **Peak threshold** (default 0.85): Pressure level for "peak" callback
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = ForcePressGestureRecognizer::new(arena)
///     .with_on_start(|details| {
///         println!("Force press started at {:?}", details.global_position);
///     })
///     .with_on_peak(|details| {
///         println!("Force press peaked! Pressure: {}", details.pressure);
///     })
///     .with_on_end(|details| {
///         println!("Force press ended");
///     });
///
/// // Add to arena and handle events
/// recognizer.add_pointer(pointer_id, position);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct ForcePressGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: GestureRecognizerState,

    /// Callbacks
    callbacks: Arc<Mutex<ForcePressCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ForcePressState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

    /// Pressure threshold to start force press (0.0 to 1.0)
    start_pressure: f32,

    /// Pressure threshold for peak force press (0.0 to 1.0)
    peak_pressure: f32,
}

#[derive(Default)]
struct ForcePressCallbacks {
    on_start: Option<ForcePressStartCallback>,
    on_update: Option<ForcePressUpdateCallback>,
    on_peak: Option<ForcePressPeakCallback>,
    on_end: Option<ForcePressEndCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForcePressPhase {
    /// Ready to start
    Ready,
    /// Pointer down, waiting for pressure to exceed threshold
    Possible,
    /// Force press started (pressure > start threshold)
    Started,
    /// Peak pressure reached
    Peaked,
    /// Force press ended
    Ended,
}

#[derive(Debug, Clone)]
struct ForcePressState {
    /// Current phase
    phase: ForcePressPhase,
    /// Current position
    current_position: Offset<Pixels>,
    /// Current pressure (0.0 to 1.0)
    current_pressure: f32,
    /// Maximum pressure for the device (always 1.0 for ui-events)
    max_pressure: f32,
    /// Whether peak callback has been called
    peak_triggered: bool,
}

impl Default for ForcePressState {
    fn default() -> Self {
        Self {
            phase: ForcePressPhase::Ready,
            current_position: Offset::new(Pixels::ZERO, Pixels::ZERO),
            current_pressure: 0.0,
            max_pressure: 1.0,
            peak_triggered: false,
        }
    }
}

impl ForcePressGestureRecognizer {
    /// Create a new force press recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(ForcePressCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ForcePressState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            start_pressure: FORCE_PRESS_START_PRESSURE,
            peak_pressure: FORCE_PRESS_PEAK_PRESSURE,
        })
    }

    /// Create a new force press recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(ForcePressCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ForcePressState::default())),
            settings: Arc::new(Mutex::new(settings)),
            start_pressure: FORCE_PRESS_START_PRESSURE,
            peak_pressure: FORCE_PRESS_PEAK_PRESSURE,
        })
    }

    /// Get the current gesture settings
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update gesture settings
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Set the start pressure threshold (0.0 to 1.0)
    ///
    /// Default is 0.4 (40% of max pressure).
    pub fn with_start_pressure(mut self: Arc<Self>, pressure: f32) -> Arc<Self> {
        // Safe because Arc::new just created this and we have the only reference
        Arc::get_mut(&mut self).unwrap().start_pressure = pressure.clamp(0.0, 1.0);
        self
    }

    /// Set the peak pressure threshold (0.0 to 1.0)
    ///
    /// Default is 0.85 (85% of max pressure).
    pub fn with_peak_pressure(mut self: Arc<Self>, pressure: f32) -> Arc<Self> {
        Arc::get_mut(&mut self).unwrap().peak_pressure = pressure.clamp(0.0, 1.0);
        self
    }

    /// Set the force press start callback
    ///
    /// Called when pressure first exceeds the start threshold.
    pub fn with_on_start(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_start = Some(Arc::new(callback));
        self
    }

    /// Set the force press update callback
    ///
    /// Called whenever pressure changes while force press is active.
    pub fn with_on_update(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_update = Some(Arc::new(callback));
        self
    }

    /// Set the force press peak callback
    ///
    /// Called once when pressure first exceeds the peak threshold.
    pub fn with_on_peak(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_peak = Some(Arc::new(callback));
        self
    }

    /// Set the force press end callback
    ///
    /// Called when pressure drops below start threshold or pointer is released.
    pub fn with_on_end(
        self: Arc<Self>,
        callback: impl Fn(ForcePressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_end = Some(Arc::new(callback));
        self
    }

    /// Get the current start pressure threshold
    pub fn start_pressure(&self) -> f32 {
        self.start_pressure
    }

    /// Get the current peak pressure threshold
    pub fn peak_pressure(&self) -> f32 {
        self.peak_pressure
    }

    /// Create force press details from current state
    fn create_details(&self, state: &ForcePressState) -> ForcePressDetails {
        ForcePressDetails::new(
            state.current_position,
            state.current_position, // local_position (updated during hit testing)
            state.current_pressure,
            state.max_pressure,
        )
    }

    /// Handle pointer down event
    fn handle_down(&self, position: Offset<Pixels>, pressure: f32) {
        let mut state = self.gesture_state.lock();

        // Check if device supports pressure (pressure > 0 indicates support)
        // In ui-events, pressure of 0.0 typically means no pressure support
        if pressure == 0.0 {
            // No pressure support - reject immediately
            state.phase = ForcePressPhase::Ended;
            drop(state);
            self.state.reject();
            return;
        }

        state.phase = ForcePressPhase::Possible;
        state.current_position = position;
        state.current_pressure = pressure;
        state.max_pressure = 1.0; // ui-events uses normalized pressure
        state.peak_triggered = false;

        // Check if already past start threshold
        if state.current_pressure >= self.start_pressure {
            state.phase = ForcePressPhase::Started;
            let details = self.create_details(&state);
            drop(state);

            // Call on_start callback
            if let Some(callback) = self.callbacks.lock().on_start.clone() {
                callback(details);
            }
        }
    }

    /// Handle pointer move event (pressure may change)
    fn handle_move(&self, position: Offset<Pixels>, pressure: f32) {
        // Cache settings to avoid nested locks
        let settings = self.settings.lock().clone();
        let mut state = self.gesture_state.lock();

        // Check slop - if moved too far, cancel
        if let Some(initial_pos) = self.state.initial_position() {
            let delta = position - initial_pos;
            if settings.exceeds_touch_slop(delta.distance()) {
                // Moved too far, end the gesture
                if state.phase == ForcePressPhase::Started || state.phase == ForcePressPhase::Peaked
                {
                    state.phase = ForcePressPhase::Ended;
                    let details = self.create_details(&state);
                    drop(state);

                    if let Some(callback) = self.callbacks.lock().on_end.clone() {
                        callback(details);
                    }

                    self.state.stop_tracking();
                }
                return;
            }
        }

        state.current_position = position;
        state.current_pressure = pressure;

        match state.phase {
            ForcePressPhase::Possible => {
                // Check if pressure now exceeds start threshold
                if state.current_pressure >= self.start_pressure {
                    state.phase = ForcePressPhase::Started;
                    let details = self.create_details(&state);
                    drop(state);

                    if let Some(callback) = self.callbacks.lock().on_start.clone() {
                        callback(details);
                    }
                }
            }
            ForcePressPhase::Started => {
                let details = self.create_details(&state);

                // Check for peak
                if !state.peak_triggered && state.current_pressure >= self.peak_pressure {
                    state.peak_triggered = true;
                    state.phase = ForcePressPhase::Peaked;
                    drop(state);

                    if let Some(callback) = self.callbacks.lock().on_peak.clone() {
                        callback(details);
                    }
                } else {
                    drop(state);
                }

                // Call update callback
                if let Some(callback) = self.callbacks.lock().on_update.clone() {
                    callback(details);
                }

                // Check if pressure dropped below start threshold
                let state = self.gesture_state.lock();
                if state.current_pressure < self.start_pressure {
                    drop(state);
                    self.handle_end();
                }
            }
            ForcePressPhase::Peaked => {
                let details = self.create_details(&state);
                drop(state);

                // Call update callback
                if let Some(callback) = self.callbacks.lock().on_update.clone() {
                    callback(details);
                }

                // Check if pressure dropped below start threshold
                let state = self.gesture_state.lock();
                if state.current_pressure < self.start_pressure {
                    drop(state);
                    self.handle_end();
                }
            }
            _ => {}
        }
    }

    /// Handle pointer up event
    fn handle_up(&self, position: Offset<Pixels>) {
        let mut state = self.gesture_state.lock();
        state.current_position = position;
        state.current_pressure = 0.0;

        if state.phase == ForcePressPhase::Started || state.phase == ForcePressPhase::Peaked {
            state.phase = ForcePressPhase::Ended;
            let details = self.create_details(&state);
            drop(state);

            if let Some(callback) = self.callbacks.lock().on_end.clone() {
                callback(details);
            }

            self.state.stop_tracking();
        } else {
            drop(state);
            self.reset();
        }
    }

    /// Handle end of force press
    fn handle_end(&self) {
        let mut state = self.gesture_state.lock();
        state.phase = ForcePressPhase::Ended;
        let details = self.create_details(&state);
        drop(state);

        if let Some(callback) = self.callbacks.lock().on_end.clone() {
            callback(details);
        }

        self.state.stop_tracking();
    }

    /// Handle cancel event
    fn handle_cancel(&self) {
        let mut state = self.gesture_state.lock();

        if state.phase == ForcePressPhase::Started || state.phase == ForcePressPhase::Peaked {
            state.phase = ForcePressPhase::Ended;
            let details = self.create_details(&state);
            drop(state);

            if let Some(callback) = self.callbacks.lock().on_end.clone() {
                callback(details);
            }

            self.reset();
        } else {
            drop(state);
            self.reset();
        }
    }

    /// Reset the recognizer state
    fn reset(&self) {
        let mut state = self.gesture_state.lock();
        state.phase = ForcePressPhase::Ready;
        state.current_position = Offset::new(Pixels::ZERO, Pixels::ZERO);
        state.current_pressure = 0.0;
        state.peak_triggered = false;
        drop(state);

        self.state.stop_tracking();
    }
}

impl GestureRecognizer for ForcePressGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        // Start tracking this pointer
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // Only process if we're tracking a pointer
        if self.state.primary_pointer().is_none() {
            return;
        }

        match event {
            PointerEvent::Down(data) => {
                let pos = data.state.position;
                let position = Offset::new(pos.x as f32, pos.y as f32);
                self.handle_down(position, data.state.pressure);
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(pos.x as f32, pos.y as f32);
                self.handle_move(position, data.current.pressure);
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(pos.x as f32, pos.y as f32);
                self.handle_up(position);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel();
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        let mut callbacks = self.callbacks.lock();
        callbacks.on_start = None;
        callbacks.on_update = None;
        callbacks.on_peak = None;
        callbacks.on_end = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl GestureArenaMember for ForcePressGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        // NOTE: We don't call handle_cancel here because it would call reset()
        // which calls stop_tracking() which calls arena.sweep() - and we're
        // already inside arena.resolve() so that would deadlock.
        // Just reset the state without touching the arena.
        let mut state = self.gesture_state.lock();
        if state.phase == ForcePressPhase::Started || state.phase == ForcePressPhase::Peaked {
            state.phase = ForcePressPhase::Ended;
            let details = self.create_details(&state);
            drop(state);

            if let Some(callback) = self.callbacks.lock().on_end.clone() {
                callback(details);
            }
        } else {
            state.phase = ForcePressPhase::Ready;
        }
    }
}

impl std::fmt::Debug for ForcePressGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForcePressGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("settings", &self.settings.lock())
            .field("start_pressure", &self.start_pressure)
            .field("peak_pressure", &self.peak_pressure)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

    #[test]
    fn test_force_press_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = ForcePressGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
        assert_eq!(recognizer.start_pressure(), FORCE_PRESS_START_PRESSURE);
        assert_eq!(recognizer.peak_pressure(), FORCE_PRESS_PEAK_PRESSURE);
    }

    #[test]
    fn test_force_press_custom_thresholds() {
        let arena = GestureArena::new();
        let recognizer = ForcePressGestureRecognizer::new(arena)
            .with_start_pressure(0.3)
            .with_peak_pressure(0.9);

        assert_eq!(recognizer.start_pressure(), 0.3);
        assert_eq!(recognizer.peak_pressure(), 0.9);
    }

    #[test]
    fn test_force_press_start() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let started_clone = started.clone();

        let recognizer = ForcePressGestureRecognizer::new(arena).with_on_start(move |_details| {
            *started_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, position);

        // Directly call handle_down with pressure above threshold
        recognizer.handle_down(position, 0.5);

        assert!(*started.lock());
    }

    #[test]
    fn test_force_press_no_pressure_support() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let started_clone = started.clone();

        let recognizer = ForcePressGestureRecognizer::new(arena).with_on_start(move |_details| {
            *started_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, position);

        // Directly call handle_down without pressure (mouse)
        recognizer.handle_down(position, 0.0);

        // Should not start - no pressure support
        assert!(!*started.lock());
    }

    #[test]
    fn test_force_press_peak() {
        let arena = GestureArena::new();
        let peaked = Arc::new(Mutex::new(false));
        let peaked_clone = peaked.clone();

        let recognizer = ForcePressGestureRecognizer::new(arena).with_on_peak(move |_details| {
            *peaked_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, position);

        // Down with moderate pressure
        recognizer.handle_down(position, 0.5);

        // Peak not reached yet
        assert!(!*peaked.lock());

        // Move with high pressure
        recognizer.handle_move(position, 0.9);

        // Peak should be triggered
        assert!(*peaked.lock());
    }

    #[test]
    fn test_force_press_update() {
        let arena = GestureArena::new();
        let update_count = Arc::new(Mutex::new(0));
        let update_clone = update_count.clone();

        let recognizer = ForcePressGestureRecognizer::new(arena).with_on_update(move |_details| {
            *update_clone.lock() += 1;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, position);

        // Down with pressure above threshold
        recognizer.handle_down(position, 0.5);

        // Move with changing pressure
        recognizer.handle_move(position, 0.6);
        recognizer.handle_move(position, 0.7);

        assert_eq!(*update_count.lock(), 2);
    }

    #[test]
    fn test_force_press_end_on_release() {
        let arena = GestureArena::new();
        let ended = Arc::new(Mutex::new(false));
        let ended_clone = ended.clone();

        let recognizer = ForcePressGestureRecognizer::new(arena).with_on_end(move |_details| {
            *ended_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, position);

        // Down with pressure
        recognizer.handle_down(position, 0.5);

        // Up
        recognizer.handle_up(position);

        assert!(*ended.lock());
    }

    #[test]
    fn test_force_press_normalized_pressure() {
        let details = ForcePressDetails::new(Offset::ZERO, Offset::ZERO, 0.5, 1.0);
        assert_eq!(details.normalized_pressure(), 0.5);

        let details = ForcePressDetails::new(Offset::ZERO, Offset::ZERO, 1.0, 2.0);
        assert_eq!(details.normalized_pressure(), 0.5);
    }
}
