//! Drag gesture recognizer
//!
//! Recognizes drag gestures (pointer down + move).
//!
//! Supports three types of drag:
//! - **Vertical**: Movement constrained to vertical axis
//! - **Horizontal**: Movement constrained to horizontal axis
//! - **Pan**: Free movement in any direction
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/DragGestureRecognizer-class.html>

use std::{cell::RefCell, rc::Rc, sync::Arc};

use web_time::Instant;

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::PointerDispatch,
    settings::GestureSettings,
    traits::{DragAxis, PointerEventExtTrait},
};

/// Configures when the drag's initial position is reported.
///
/// Flutter parity: `gestures/recognizer.dart:48` `DragStartBehavior`.
///
/// - [`Down`](Self::Down): the initial position reported in
///   [`DragStartDetails`] is the pointer's position at the down event.
/// - [`Start`](Self::Start): the initial position is the pointer's position
///   when the recognizer wins the arena. With competitors this is usually the
///   slop-crossing position; a lone recognizer can win the deferred default
///   while still at the Down position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DragStartBehavior {
    /// Use the pointer's down position as the drag's initial position.
    Down,
    /// Use the position at arena acceptance as the drag's initial position.
    /// Flutter default — matches `DragGestureRecognizer.dragStartBehavior`.
    #[default]
    Start,
}

/// Details about drag down (pointer contact before drag starts)
#[derive(Debug, Clone, PartialEq)]
pub struct DragDownDetails {
    /// Global position where pointer contacted the screen
    pub global_position: Offset<f64>,
    /// Local position (relative to widget)
    pub local_position: Offset<f64>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag start
#[derive(Debug, Clone)]
pub struct DragStartDetails {
    /// Global position where drag started
    pub global_position: Offset<f64>,
    /// Local position (relative to widget)
    pub local_position: Offset<f64>,
    /// Pointer device kind
    pub kind: PointerType,
    /// When the drag started
    pub timestamp: Instant,
}

/// Details about drag update
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdateDetails {
    /// Current global position
    pub global_position: Offset<f64>,
    /// Current local position
    pub local_position: Offset<f64>,
    /// Delta since last update
    pub delta: Offset<f64>,
    /// `delta` projected onto the recognizer's primary axis. Flutter parity:
    /// `DragUpdateDetails.primaryDelta` — "the amount the pointer has moved
    /// along the primary axis **since the previous call to onUpdate**", i.e.
    /// per-event, not cumulative since the drag started.
    pub primary_delta: f64,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag end
#[derive(Debug, Clone, PartialEq)]
pub struct DragEndDetails {
    /// Velocity at end of drag (pixels per second)
    pub velocity: Velocity,
    /// Final global position
    pub global_position: Offset<f64>,
    /// Final local position
    pub local_position: Offset<f64>,
    /// Primary velocity (axis-aligned)
    pub primary_velocity: f64,
}

// Re-export Velocity from the velocity module
pub use crate::processing::Velocity;

/// Callback fired when a pointer contacts the screen and might begin a drag.
pub type DragDownCallback = Rc<dyn Fn(DragDownDetails)>;
/// Callback fired when the drag is recognized by the gesture arena.
pub type DragStartCallback = Rc<dyn Fn(DragStartDetails)>;
/// Callback fired for each pointer move while the drag is in progress.
pub type DragUpdateCallback = Rc<dyn Fn(DragUpdateDetails)>;
/// Callback fired when the pointer lifts and the drag completes.
pub type DragEndCallback = Rc<dyn Fn(DragEndDetails)>;
/// Callback fired when the gesture is cancelled (e.g. the arena rejects it).
pub type DragCancelCallback = Rc<dyn Fn()>;

/// Recognizes drag gestures
///
/// A drag begins when this recognizer wins its pointer's arena. Movement past
/// the device slop explicitly claims victory when other recognizers are still
/// competing; a lone recognizer can win by the arena's deferred default.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = DragGestureRecognizer::new(arena, DragAxis::Vertical)
///     .with_on_start(|details| {
///         println!("Drag started at {:?}", details.global_position);
///     })
///     .with_on_update(|details| {
///         println!("Dragged by {:?}", details.delta);
///     })
///     .with_on_end(|details| {
///         println!("Drag ended with velocity: {}", details.velocity.magnitude());
///     });
/// ```
#[derive(Clone)]
pub struct DragGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Drag axis constraint
    axis: DragAxis,

    /// When to fix the drag's initial position.
    ///
    /// - [`DragStartBehavior::Down`]: position is the down-event position.
    /// - [`DragStartBehavior::Start`]: position is where arena acceptance
    ///   happens (Flutter default).
    start_behavior: DragStartBehavior,

    /// Callbacks
    callbacks: Rc<RefCell<DragCallbacks>>,

    /// Current drag state
    drag_state: Arc<Mutex<DragState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for DragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizer")
            .field("state", &self.state)
            .field("axis", &self.axis)
            .field("start_behavior", &self.start_behavior)
            .field("drag_state", &*self.drag_state.lock())
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

// Field names keep Flutter's `onDragStart`-style callback names (parity).
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct DragCallbacks {
    on_down: Option<DragDownCallback>,
    on_start: Option<DragStartCallback>,
    on_update: Option<DragUpdateCallback>,
    on_end: Option<DragEndCallback>,
    on_cancel: Option<DragCancelCallback>,
}

#[derive(Debug, Clone)]
struct DragState {
    /// Current state
    state: DragPhase,
    /// When drag started
    start_time: Option<Instant>,
    /// Position reported in [`DragStartDetails`] — depends on
    /// `start_behavior` (down position or slop-crossing position).
    start_position: Option<Offset<f64>>,
    /// The same contact as `start_position`, in the root's space.
    ///
    /// Stored rather than derived: dispatch localises the event before a
    /// recognizer sees it, so the global position exists only at the moment
    /// the event arrives (issue #908).
    start_global_position: Option<Offset<f64>>,
    /// The contact position at Down, in the root's space — the global
    /// counterpart of the shared state's `initial_position`.
    down_global_position: Option<Offset<f64>>,
    /// Last update position
    last_position: Option<Offset<f64>>,
    /// The same contact as `last_position`, in the root's space.
    last_global_position: Option<Offset<f64>>,
    /// Last update time (for velocity calculation)
    last_time: Option<Instant>,
    /// Device kind captured at Down, needed when arena acceptance arrives
    /// without another pointer event.
    device_kind: Option<PointerType>,
    /// Velocity tracker
    velocity_tracker: VelocityTracker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragPhase {
    Ready,
    Possible, // Pointer down but haven't moved beyond slop yet
    Started,  // Drag in progress
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            state: DragPhase::Ready,
            start_time: None,
            start_position: None,
            start_global_position: None,
            down_global_position: None,
            last_position: None,
            last_global_position: None,
            last_time: None,
            device_kind: None,
            velocity_tracker: VelocityTracker::new(),
        }
    }
}

impl DragGestureRecognizer {
    /// Create a new drag recognizer with gesture arena and axis constraint
    pub fn new(arena: crate::arena::GestureArena, axis: DragAxis) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            start_behavior: DragStartBehavior::default(),
            callbacks: Rc::new(RefCell::new(DragCallbacks::default())),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a new drag recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        axis: DragAxis,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            start_behavior: DragStartBehavior::default(),
            callbacks: Rc::new(RefCell::new(DragCallbacks::default())),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Configure when the drag's initial position is reported.
    ///
    /// See [`DragStartBehavior`] for the semantics. Default is
    /// [`DragStartBehavior::Start`].
    pub fn with_drag_start_behavior(self: Arc<Self>, behavior: DragStartBehavior) -> Arc<Self> {
        // Re-construct with the new behavior — fields are all `Copy`/Arc so
        // this is a cheap move, and it keeps the constructor pattern uniform.
        Arc::new(Self {
            start_behavior: behavior,
            ..(*self).clone()
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

    /// Drag axis this recogniser is bound to.
    pub fn axis(&self) -> DragAxis {
        self.axis
    }

    /// Currently-configured [`DragStartBehavior`].
    pub fn drag_start_behavior(&self) -> DragStartBehavior {
        self.start_behavior
    }

    /// Minimum drag distance for the current axis and pointer `kind`.
    ///
    /// Flutter parity: `gestures/events.dart` `computeHitSlop`/`computePanSlop`
    /// (tag `3.44.0`) special-case exactly `PointerDeviceKind.mouse` as
    /// "precise" — stylus, trackpad, and unknown all still resolve through
    /// the configured settings profile alongside touch. A precise (mouse)
    /// pointer always gets the fixed, much smaller constant, unconditionally
    /// — [`with_settings`](Self::with_settings) customization has no effect
    /// on it, matching `computeHitSlop`'s mouse arm never consulting
    /// `gestureSettings`. For every other kind, per-axis slop:
    /// - [`DragAxis::Vertical`][]: [`GestureSettings::pan_slop_vertical`]
    /// - [`DragAxis::Horizontal`][]: [`GestureSettings::pan_slop_horizontal`]
    /// - [`DragAxis::Free`][]: [`GestureSettings::pan_slop`]
    ///
    /// The kind split itself lives in [`GestureSettings::hit_slop`] and
    /// [`GestureSettings::pan_slop_for`] — this method picks the tier and
    /// then applies FLUI's per-axis narrowing on top.
    fn min_drag_distance(&self, kind: PointerType) -> f64 {
        let s = self.settings.lock();
        match self.axis {
            // PanGestureRecognizer resolves through `computePanSlop`, which is
            // what the shared accessor is — both arms of it.
            DragAxis::Free => s.pan_slop_for(kind),
            // Vertical/HorizontalDragGestureRecognizer resolve through
            // `computeHitSlop`. Its mouse arm is the shared accessor exactly;
            // for every other kind FLUI narrows further, to a per-axis value
            // the reference has no equivalent of.
            DragAxis::Vertical | DragAxis::Horizontal if kind == PointerType::Mouse => {
                s.hit_slop(kind)
            }
            DragAxis::Vertical => s.pan_slop_vertical(),
            DragAxis::Horizontal => s.pan_slop_horizontal(),
        }
    }

    /// Get the minimum fling velocity from settings
    fn min_fling_velocity(&self) -> f64 {
        self.settings.lock().min_fling_velocity()
    }

    /// Set the drag down callback (called on pointer contact before drag
    /// starts)
    ///
    /// This is called when a pointer contacts the screen with a primary button
    /// and might begin to move. Unlike `on_start`, this is called before any
    /// movement threshold is met.
    pub fn with_on_down(
        self: Arc<Self>,
        callback: impl Fn(DragDownDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_down = Some(Rc::new(callback));
        self
    }

    /// Set the drag start callback
    pub fn with_on_start(
        self: Arc<Self>,
        callback: impl Fn(DragStartDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_start = Some(Rc::new(callback));
        self
    }

    /// Set the drag update callback
    pub fn with_on_update(
        self: Arc<Self>,
        callback: impl Fn(DragUpdateDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_update = Some(Rc::new(callback));
        self
    }

    /// Set the drag end callback
    pub fn with_on_end(self: Arc<Self>, callback: impl Fn(DragEndDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_end = Some(Rc::new(callback));
        self
    }

    /// Set the drag cancel callback
    pub fn with_on_cancel(self: Arc<Self>, callback: impl Fn() + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_cancel = Some(Rc::new(callback));
        self
    }

    /// Handle pointer down - start tracking
    fn handle_down(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        // Read the arena's clock, not the OS clock directly: production binds
        // it to `SystemClock` (so this is `Instant::now()` either way), but a
        // headless frame driver binds it to a `ManualClock` it advances
        // explicitly — the same mechanism `LongPressGestureRecognizer` and
        // `DoubleTapGestureRecognizer` already use — so a test controls
        // velocity-sample spacing instead of inheriting the wall clock.
        let now = self.state.now();
        let mut state = self.drag_state.lock();
        state.state = DragPhase::Possible;
        state.start_time = Some(now);
        state.start_position = None;
        state.start_global_position = None;
        state.down_global_position = Some(global_position);
        state.last_position = Some(position);
        state.last_global_position = Some(global_position);
        state.last_time = Some(now);
        state.device_kind = Some(kind);
        state.velocity_tracker.reset();
        state.velocity_tracker.add_position(now, position);
        drop(state); // Release lock before callback

        // Call on_down callback (pointer contact before drag starts)
        if let Some(callback) = self.callbacks.borrow().on_down.clone() {
            let details = DragDownDetails {
                global_position,
                local_position: position,
                kind,
            };
            callback(details);
        }
    }

    /// Handle pointer move - check slop and start/update drag
    fn handle_move(&self, position: Offset<f64>, global_position: Offset<f64>, kind: PointerType) {
        let mut state = self.drag_state.lock();

        match state.state {
            DragPhase::Possible => {
                let Some(initial_pos) = self.state.initial_position() else {
                    return;
                };
                let distance = self.calculate_primary_delta(position - initial_pos);
                let now = self.state.now();
                state.last_position = Some(position);
                state.last_global_position = Some(global_position);
                state.last_time = Some(now);
                state.device_kind = Some(kind);
                state.velocity_tracker.add_position(now, position);
                let should_accept = distance.abs() > self.min_drag_distance(kind);
                drop(state);

                // Crossing slop is a request to win, not permission to invoke
                // callbacks. Arena acceptance can be delayed by competitors;
                // `accept_gesture` is the sole start transition.
                if should_accept {
                    self.state.accept_tracked();
                }
            }
            DragPhase::Started => {
                // Update drag
                if let Some(last_pos) = state.last_position {
                    let now = self.state.now();
                    let delta = self.project_delta(position - last_pos);
                    state.last_position = Some(position);
                    state.last_global_position = Some(global_position);
                    state.last_time = Some(now);
                    state.velocity_tracker.add_position(now, position);

                    // Per-event, matching `delta` above — not accumulated
                    // across the whole drag. Flutter's `primaryDelta` reports
                    // movement "since the previous call to onUpdate"; an
                    // earlier revision passed a running total here, making
                    // every update after the first report the wrong
                    // magnitude (and, once the drag reverses direction, the
                    // wrong sign) for any drag with 3+ move events.
                    let primary_delta = self.calculate_primary_delta(delta);

                    drop(state); // Release lock before calling callback

                    if let Some(callback) = self.callbacks.borrow().on_update.clone() {
                        let details = DragUpdateDetails {
                            global_position,
                            local_position: position,
                            delta,
                            primary_delta,
                            kind,
                        };
                        callback(details);
                    }
                }
            }
            DragPhase::Ready => {}
        }
    }

    /// Transition a possible drag after the arena has accepted it.
    ///
    /// State is committed before application code so a reentrant callback
    /// observes `Started`, and the callback never runs under a recognizer lock.
    fn begin_accepted_drag(&self) {
        let (start_details, initial_update) = {
            let mut state = self.drag_state.lock();
            if state.state != DragPhase::Possible {
                return;
            }
            let Some(initial) = self.state.initial_position() else {
                return;
            };
            let accepted_position = state.last_position.unwrap_or(initial);
            // The global counterparts of the same two anchors. Both are
            // OBSERVED values, never re-derived: a recognizer holds no
            // transform, so a local delta cannot be mapped into the root's
            // space here.
            let initial_global = state.down_global_position.unwrap_or(initial);
            let accepted_global = state.last_global_position.unwrap_or(initial_global);
            let start_position = match self.start_behavior {
                DragStartBehavior::Down => initial,
                DragStartBehavior::Start => accepted_position,
            };
            let start_global = match self.start_behavior {
                DragStartBehavior::Down => initial_global,
                DragStartBehavior::Start => accepted_global,
            };
            let timestamp = state.last_time.unwrap_or_else(|| self.state.now());
            let kind = state.device_kind.unwrap_or(PointerType::Touch);
            state.state = DragPhase::Started;
            state.start_position = Some(start_position);
            state.start_global_position = Some(start_global);
            state.last_position = Some(accepted_position);
            state.last_global_position = Some(accepted_global);
            let start_details = DragStartDetails {
                global_position: start_global,
                local_position: start_position,
                kind,
                timestamp,
            };

            // Flutter's `_checkDrag`: `Down` preserves the contact position
            // for onStart and immediately flushes movement accumulated while
            // the arena was unresolved. `Start` re-anchors at acceptance and
            // deliberately emits no synthetic first update.
            let initial_update = (self.start_behavior == DragStartBehavior::Down)
                .then(|| self.project_delta(accepted_position - initial))
                .filter(|delta| delta.dx != 0.0 || delta.dy != 0.0)
                .map(|delta| {
                    let corrected_position = initial + delta;
                    // `corrected_position` is SYNTHESIZED — the down anchor
                    // plus an axis-projected delta — so it has no global
                    // counterpart a recognizer can compute: mapping a local
                    // delta into the root's space needs the transform, which
                    // dispatch applied and did not hand over. The observed
                    // global at acceptance is reported instead: it is where
                    // the pointer actually is, which is what a consumer of
                    // this field wants, while the local half stays projected
                    // for the widget's own axis maths.
                    DragUpdateDetails {
                        global_position: accepted_global,
                        local_position: corrected_position,
                        primary_delta: self.calculate_primary_delta(delta),
                        delta,
                        kind,
                    }
                });
            (start_details, initial_update)
        };

        if let Some(callback) = self.callbacks.borrow().on_start.clone() {
            callback(start_details);
        }
        if let (Some(details), Some(callback)) =
            (initial_update, self.callbacks.borrow().on_update.clone())
        {
            callback(details);
        }
    }

    /// Handle pointer up - end drag
    fn handle_up(&self, position: Offset<f64>, global_position: Offset<f64>, _kind: PointerType) {
        let mut state = self.drag_state.lock();

        if state.state == DragPhase::Started {
            // Calculate final velocity
            let velocity = state.velocity_tracker.get_velocity();
            let primary_velocity = self.calculate_primary_velocity(velocity.pixels_per_second);

            let callback = self.callbacks.borrow().on_end.clone();
            *state = DragState::default();
            drop(state);

            // Retire tracking before application code can unwind or start
            // another pointer sequence on this recognizer.
            self.state.stop_tracking();
            if let Some(callback) = callback {
                callback(DragEndDetails {
                    velocity,
                    global_position,
                    local_position: position,
                    primary_velocity,
                });
            }
        } else {
            // A pointer that lifts before this recognizer wins is no longer a
            // candidate. Flutter's didStopTrackingLastPointer resolves
            // rejected and emits onCancel; leaving the entry live lets sweep
            // incorrectly choose this drag over a competing tap.
            let callback = self.callbacks.borrow().on_cancel.clone();
            *state = DragState::default();
            drop(state);
            self.state.reject();
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.drag_state.lock();

        match state.state {
            DragPhase::Ready => {}
            DragPhase::Possible => {
                let callback = self.callbacks.borrow().on_cancel.clone();
                *state = DragState::default();
                drop(state);

                self.state.reject();
                if let Some(callback) = callback {
                    callback();
                }
            }
            DragPhase::Started => {
                // Flutter's `didStopTrackingLastPointer` ends an accepted
                // drag even when the terminal event is PointerCancel.
                let position = state.last_position.unwrap_or(Offset::ZERO);
                let global_position = state.last_global_position.unwrap_or(position);
                let velocity = state.velocity_tracker.get_velocity();
                let primary_velocity = self.calculate_primary_velocity(velocity.pixels_per_second);
                let callback = self.callbacks.borrow().on_end.clone();
                *state = DragState::default();
                drop(state);

                self.state.stop_tracking();
                if let Some(callback) = callback {
                    callback(DragEndDetails {
                        velocity,
                        global_position,
                        local_position: position,
                        primary_velocity,
                    });
                }
            }
        }
    }

    /// Project movement onto the recognizer's configured axis.
    ///
    /// Flutter's horizontal and vertical recognizers report an axis-pure
    /// `DragUpdateDetails.delta`; only a pan recognizer retains both axes.
    fn project_delta(&self, delta: Offset<f64>) -> Offset<f64> {
        match self.axis {
            DragAxis::Vertical => Offset::new(0.0, delta.dy),
            DragAxis::Horizontal => Offset::new(delta.dx, 0.0),
            DragAxis::Free => delta.to_delta(),
        }
    }

    /// Calculate primary delta based on axis
    fn calculate_primary_delta(&self, delta: Offset<f64>) -> f64 {
        match self.axis {
            DragAxis::Vertical => delta.dy,
            DragAxis::Horizontal => delta.dx,
            DragAxis::Free => delta.distance(),
        }
    }

    /// Calculate primary velocity based on axis
    fn calculate_primary_velocity(&self, velocity: Offset<f64>) -> f64 {
        match self.axis {
            DragAxis::Vertical => velocity.dy,
            DragAxis::Horizontal => velocity.dx,
            DragAxis::Free => velocity.distance(),
        }
    }

    /// Check if velocity is sufficient for a fling gesture
    pub fn is_fling(&self, velocity: &Velocity) -> bool {
        let speed = velocity.pixels_per_second.distance();
        speed >= self.min_fling_velocity()
    }

    /// Extract position and pointer type from a PointerEvent
    fn extract_event_data(event: &PointerEvent) -> (Offset<f64>, PointerType) {
        let position = event.position();
        let pointer_type = match event {
            PointerEvent::Down(e) | PointerEvent::Up(e) => e.pointer.pointer_type,
            PointerEvent::Move(e) => e.pointer.pointer_type,
            PointerEvent::Cancel(info) | PointerEvent::Enter(info) | PointerEvent::Leave(info) => {
                info.pointer_type
            }
            PointerEvent::Scroll(e) => e.pointer.pointer_type,
            PointerEvent::Gesture(e) => e.pointer.pointer_type,
        };
        (position, pointer_type)
    }
}

impl GestureRecognizer for DragGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        global_position: Offset<f64>,
    ) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // Start tracking this pointer
        self.state
            .start_tracking(pointer, position, global_position, self);

        // Handle pointer down
        self.handle_down(position, global_position, PointerType::Touch);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only process if we're tracking a pointer
        let Some(primary) = self.state.primary_pointer() else {
            return;
        };
        let event = dispatch.local;
        // Filter to the primary pointer we are tracking.
        if event.pointer_id() != primary {
            return;
        }

        let (position, pointer_type) = Self::extract_event_data(event);
        // Read here and threaded on, never re-derived: this is the only point
        // at which the untransformed position is available at all.
        let global_position = dispatch.global.position();

        match event {
            PointerEvent::Move(_) => {
                self.handle_move(position, global_position, pointer_type);
            }
            PointerEvent::Up(_) => {
                self.handle_up(position, global_position, pointer_type);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel();
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear tracked pointer (Flutter parity:
        // gestures/recognizer.dart:485-493 disposing GestureRecognizer
        // clears arena state for tracked pointers).
        self.state.reject();
        let mut callbacks = self.callbacks.borrow_mut();
        callbacks.on_down = None;
        callbacks.on_start = None;
        callbacks.on_update = None;
        callbacks.on_end = None;
        callbacks.on_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Flutter parity: `monodrag.dart:81 sealed class DragGestureRecognizer
// extends OneSequenceGestureRecognizer`. Drag is OneSequence (NOT
// PrimaryPointer) — it tracks a single sequence but doesn't have the
// pre-acceptance deadline semantics of PrimaryPointer recognizers.

impl crate::recognizers::OneSequenceGestureRecognizer for DragGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                self.begin_accepted_drag();
            }
            crate::arena::GestureDisposition::Rejected => {
                self.reject_gesture(pointer);
            }
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl GestureArenaMember for DragGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        if self.state.primary_pointer() == Some(pointer) {
            self.begin_accepted_drag();
        }
    }

    fn reject_gesture(&self, pointer: PointerId) {
        if self.state.primary_pointer() != Some(pointer) {
            return;
        }
        let callback = {
            let mut state = self.drag_state.lock();
            let callback = (state.state != DragPhase::Ready)
                .then(|| self.callbacks.borrow().on_cancel.clone())
                .flatten();
            *state = DragState::default();
            callback
        };
        // The arena already resolved this entry. Clear only local tracking;
        // resolving it again is unnecessary re-entrancy.
        self.state.stop_tracking();
        if let Some(callback) = callback {
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{arena::GestureArena, events::make_move_event};

    struct PassiveCompetitor;

    impl crate::sealed::arena_member::Sealed for PassiveCompetitor {}

    impl GestureArenaMember for PassiveCompetitor {
        fn accept_gesture(&self, _pointer: PointerId) {}

        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    fn close_with_competitor(arena: &GestureArena, pointer: PointerId) {
        arena.add(pointer, Arc::new(PassiveCompetitor));
        arena.close(pointer);
    }

    #[test]
    fn up_before_acceptance_rejects_drag_and_preserves_the_competitor() {
        struct Winner(Arc<Mutex<u32>>);

        impl crate::sealed::arena_member::Sealed for Winner {}

        impl GestureArenaMember for Winner {
            fn accept_gesture(&self, _pointer: PointerId) {
                *self.0.lock() += 1;
            }

            fn reject_gesture(&self, _pointer: PointerId) {}
        }

        let arena = GestureArena::new();
        let cancels = Arc::new(Mutex::new(0_u32));
        let callback_cancels = Arc::clone(&cancels);
        let recognizer = DragGestureRecognizer::new(arena.clone(), DragAxis::Horizontal)
            .with_on_cancel(move || *callback_cancels.lock() += 1);
        let accepted = Arc::new(Mutex::new(0_u32));
        let pointer = PointerId::PRIMARY;
        let position = Offset::new(10.0, 20.0);

        recognizer.add_pointer(pointer, position, position);
        arena.add(pointer, Arc::new(Winner(Arc::clone(&accepted))));
        arena.close(pointer);
        recognizer.handle_event(PointerDispatch::at_root(&crate::events::make_up_event(
            position,
            PointerType::Touch,
        )));
        arena.drain_deferred_resolutions();

        assert_eq!(*cancels.lock(), 1);
        assert_eq!(
            *accepted.lock(),
            1,
            "the possible drag must withdraw instead of stealing the Up sweep"
        );
        assert_eq!(recognizer.primary_pointer(), None);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_drag_recognizer_vertical() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let updated = Arc::new(Mutex::new(false));

        let started_clone = started.clone();
        let updated_clone = updated.clone();

        let recognizer = DragGestureRecognizer::new(arena.clone(), DragAxis::Vertical)
            .with_on_start(move |_details| {
                *started_clone.lock() = true;
            })
            .with_on_update(move |_details| {
                *updated_clone.lock() = true;
            });

        let pointer = PointerId::PRIMARY;
        let start_pos = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, start_pos, start_pos);
        close_with_competitor(&arena, pointer);

        // Move vertically beyond slop
        let moved_pos = Offset::new(100.0, 130.0); // 30px down
        let move_event = make_move_event(moved_pos, PointerType::Touch);
        recognizer.handle_event(PointerDispatch::at_root(&move_event));

        // Should have started
        assert!(*started.lock());

        // Move more
        let moved_pos2 = Offset::new(100.0, 150.0);
        let move_event2 = make_move_event(moved_pos2, PointerType::Touch);
        recognizer.handle_event(PointerDispatch::at_root(&move_event2));

        // Should have updated
        assert!(*updated.lock());
    }

    // ========================================================================
    // Precise-pointer slop (mouse vs. touch)
    //
    // Flutter parity: `gestures/events.dart` `computeHitSlop` (tag `3.44.0`)
    // returns `kPrecisePointerHitSlop` (1.0 logical px) for
    // `PointerDeviceKind.mouse`, not `kTouchSlop` (18.0) — every other kind
    // (stylus, trackpad, unknown, touch) still resolves through the
    // touch-tier settings. `VerticalDragGestureRecognizer` /
    // `HorizontalDragGestureRecognizer` (`DragAxis::Vertical` /
    // `DragAxis::Horizontal` here) call `computeHitSlop` directly.
    // ========================================================================

    // ========================================================================
    // H/V/Pan split tests
    //
    // Verifies Flutter parity for:
    // - per-axis slop (Vertical/Horizontal pick their own slop, Free uses
    //   the generic `pan_slop`),
    // - `DragStartBehavior::Down` vs `Start` (start_position differs).
    // ========================================================================
}
