//! Scale gesture recognizer
//!
//! Recognizes scale gestures (pinch to zoom and rotate with two or more
//! contacts).
//!
//! A scale gesture:
//! - joins the gesture arena for **every** contact it tracks, and claims all
//!   of them together once the contacts have moved past the scale or pan slop;
//! - starts when it has won an arena and at least two contacts are down;
//! - reports scale, per-axis scale, rotation and focal point measured from
//!   where the contacts were placed, carried continuously across contacts
//!   being added or lifted;
//! - ends when fewer than two contacts remain.

use std::{
    cell::RefCell,
    f64::consts::{PI, TAU},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{
    GestureRecognizer, RecognizerBase, finish_containment, invoke_callback, retire_callback,
    withdraw_cancelled,
};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel},
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::{PointerDispatch, RoutePanic},
    settings::GestureSettings,
};

// ============================================================================
// Public surface
// ============================================================================

/// Callback for scale start events
pub type ScaleStartCallback = Rc<dyn Fn(ScaleStartDetails)>;

/// Callback for scale update events
pub type ScaleUpdateCallback = Rc<dyn Fn(ScaleUpdateDetails)>;

/// Callback for scale end events
pub type ScaleEndCallback = Rc<dyn Fn(ScaleEndDetails)>;

/// Callback for scale cancel events
pub type ScaleCancelCallback = Rc<dyn Fn()>;

/// Details about scale gesture start
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleStartDetails {
    /// Focal point (centroid of the tracked contacts).
    pub focal_point: Offset<f64>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<f64>,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture update.
///
/// Every value is finite. A sample whose arithmetic would produce a
/// non-finite value is not published.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleUpdateDetails {
    /// Focal point: the centroid of the contacts currently down.
    ///
    /// It is the centroid by definition, so it moves when a contact is added
    /// or lifted; scale and rotation do not.
    pub focal_point: Offset<f64>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<f64>,
    /// Scale factor (1.0 = no change, >1.0 = zoom in, <1.0 = zoom out).
    ///
    /// The ratio of the contacts' span (mean distance from the focal point)
    /// to the span where they were placed. Adding or lifting a contact
    /// re-measures the baseline and carries the factor reached so far, so the
    /// value does not jump. While the baseline span is zero (coincident
    /// contacts) the factor holds, and the first non-zero span becomes the
    /// new baseline.
    pub scale: f64,
    /// Horizontal scale factor, measured on the horizontal span alone.
    ///
    /// Holds at its current value while the baseline horizontal span is zero
    /// (contacts placed on one vertical line, for example), and re-measures
    /// from the first non-zero horizontal span.
    pub horizontal_scale: f64,
    /// Vertical scale factor, with the same degenerate rule as
    /// [`horizontal_scale`](Self::horizontal_scale).
    pub vertical_scale: f64,
    /// Rotation in radians since the contacts were placed (positive =
    /// clockwise in a y-down space).
    ///
    /// Measured on the line between the two earliest contacts still down, by
    /// accumulating each sample's change wrapped into `(-π, π]`, so turning
    /// past half a revolution keeps counting instead of jumping by `2π`. While
    /// those two contacts coincide the angle is undefined and the rotation
    /// holds.
    pub rotation: f64,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture end
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleEndDetails {
    /// Last published focal point
    pub focal_point: Offset<f64>,
    /// Final scale factor (the last published one)
    pub scale: f64,
    /// Final rotation angle in radians (the last published one)
    pub rotation: f64,
    /// Velocity of scale change (scale units per second); `0.0` when it
    /// cannot be estimated.
    pub velocity: f64,
}

/// Recognizes scale (pinch/zoom/rotate) gestures.
///
/// Requires at least two contacts. Every contact passed to
/// [`add_pointer`](GestureRecognizer::add_pointer) joins that contact's
/// gesture arena. The recognizer claims all of its arenas once the contacts
/// move past the span or focal-point slop (or the scale ratio slop), and
/// starts as soon as it has won an arena with two contacts down — so a scale
/// that wins its first contact by default, as a lone arena member does,
/// starts when the second contact lands. Contacts added while the recognizer
/// owns the gesture are claimed too.
///
/// A contact that lifts before the scale starts gives its arena up, so a
/// competing tap can still win it. A scale that drops below two contacts
/// ends with [`on_end`](Self::with_on_scale_end); losing a contact's arena or
/// a pointer cancel ends it with [`on_cancel`](Self::with_on_scale_cancel)
/// instead. A cancel ends the whole sequence.
///
/// Callbacks run after the recognizer has committed its state, with no
/// borrow held, so a callback may dispose the recognizer or replace a
/// callback. A panicking callback propagates to the dispatcher; the next
/// gesture starts clean.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let recognizer = ScaleGestureRecognizer::new(binding.arena().clone())
///     .with_on_scale_update(|details| println!("Scale: {:.2}x", details.scale));
/// ```
#[derive(Clone)]
pub struct ScaleGestureRecognizer {
    /// Base state (arena, disposal, primary pointer).
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<ScaleCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ScaleState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for ScaleGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &*self.gesture_state.lock())
            .field("settings", &*self.settings.lock())
            .finish_non_exhaustive()
    }
}

// Field names keep the `on_start`/`on_update`-style callback names.
#[expect(clippy::struct_field_names)]
#[derive(Default)]
struct ScaleCallbacks {
    on_start: Option<ScaleStartCallback>,
    on_update: Option<ScaleUpdateCallback>,
    on_end: Option<ScaleEndCallback>,
    on_cancel: Option<ScaleCancelCallback>,
}

impl ScaleCallbacks {
    fn retire(self, first: &mut Option<RoutePanic>) {
        retire_callback(self.on_start, first);
        retire_callback(self.on_update, first);
        retire_callback(self.on_end, first);
        retire_callback(self.on_cancel, first);
    }
}

/// A span at or below this many logical pixels is treated as zero: the ratio
/// against it is undefined, so it is never used as a divisor.
const DEGENERATE_SPAN: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalePhase {
    /// No contact is tracked.
    Idle,
    /// Contacts are tracked; the scale has not started.
    Possible,
    /// The scale started and is publishing updates.
    Started,
}

#[derive(Debug)]
struct Contact {
    pointer: PointerId,
    position: Offset<f64>,
    /// This contact's arena membership.
    entry: GestureArenaEntry,
}

/// Spans and focal point of a set of contacts.
#[derive(Debug, Clone, Copy)]
struct Measure {
    span: f64,
    horizontal: f64,
    vertical: f64,
    focal: Offset<f64>,
}

impl Measure {
    /// Mean deviation from the focal point (the span definition `kScaleSlop`
    /// is calibrated against), per axis and overall. `None` for fewer than two
    /// contacts or when the arithmetic overflows.
    fn of(contacts: &[Contact]) -> Option<Self> {
        if contacts.len() < 2 {
            return None;
        }
        // Work in coordinates divided by the largest magnitude, so neither the
        // centroid sum nor a contact's deviation from it (up to twice the
        // largest coordinate) overflows; only the final means are scaled back.
        let scale = contacts.iter().fold(0.0_f64, |largest, c| {
            largest.max(c.position.dx.abs()).max(c.position.dy.abs())
        });
        if !scale.is_finite() {
            return None;
        }
        let unit = if scale > 0.0 { scale } else { 1.0 };
        let count = contacts.len() as f64;
        let scaled = || contacts.iter().map(|c| c.position / unit);
        let mut focal_scaled = Offset::ZERO;
        for position in scaled() {
            focal_scaled += position / count;
        }
        let (mut span, mut horizontal, mut vertical) = (0.0, 0.0, 0.0);
        for position in scaled() {
            let delta = position - focal_scaled;
            span += delta.distance() / count;
            horizontal += delta.dx.abs() / count;
            vertical += delta.dy.abs() / count;
        }
        let focal = focal_scaled * unit;
        let measure = Self {
            span: span * unit,
            horizontal: horizontal * unit,
            vertical: vertical * unit,
            focal,
        };
        (measure.span.is_finite()
            && measure.horizontal.is_finite()
            && measure.vertical.is_finite()
            && focal.is_finite())
        .then_some(measure)
    }
}

/// The published scale factors.
#[derive(Debug, Clone, Copy)]
struct Ratios {
    scale: f64,
    horizontal: f64,
    vertical: f64,
}

impl Ratios {
    const IDENTITY: Self = Self {
        scale: 1.0,
        horizontal: 1.0,
        vertical: 1.0,
    };
}

#[derive(Debug)]
struct ScaleState {
    /// Counts starts and survives resets, so an update queued with a start is
    /// dropped when a callback ended (or restarted) that gesture reentrantly.
    starts: u64,
    phase: ScalePhase,
    /// Tracked contacts in arrival order.
    contacts: Vec<Contact>,
    /// An arena accepted this recognizer for one of the tracked contacts.
    won: bool,
    /// Set while this recognizer resolves its own arenas, so the resulting
    /// `accept_gesture` calls record the win without starting the gesture
    /// halfway through the claim.
    claiming: bool,
    /// Spans measured when the contact set last changed.
    baseline: Option<Measure>,
    /// Factors reached when the contact set last changed.
    carried: Ratios,
    /// Last committed factors.
    current: Ratios,
    /// Last committed rotation.
    rotation: f64,
    /// Angle of the earliest-pair line at the last committed sample.
    rotation_reference: Option<f64>,
    /// Last committed focal point.
    focal_point: Offset<f64>,
    /// The focal point the last start or update published, which `End` reports;
    /// a re-measure after a contact leaves changes `focal_point` silently.
    published_focal: Offset<f64>,
    /// Velocity tracker for scale changes.
    scale_velocity_tracker: VelocityTracker,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            starts: 0,
            phase: ScalePhase::Idle,
            contacts: Vec::new(),
            won: false,
            claiming: false,
            baseline: None,
            carried: Ratios::IDENTITY,
            current: Ratios::IDENTITY,
            rotation: 0.0,
            rotation_reference: None,
            focal_point: Offset::ZERO,
            published_focal: Offset::ZERO,
            scale_velocity_tracker: VelocityTracker::new(),
        }
    }
}

/// Wrap an angle difference into `(-π, π]`.
fn wrap_angle(angle: f64) -> f64 {
    let wrapped = (angle + PI).rem_euclid(TAU) - PI;
    if wrapped <= -PI {
        wrapped + TAU
    } else {
        wrapped
    }
}

/// The ratio of `current` to `baseline`, or `None` while the baseline is
/// degenerate.
/// `carried * current / baseline`, the factor reached so far times the ratio
/// since the last baseline, or `None` for a degenerate baseline. The two
/// orders of evaluation are tried so an intermediate quotient or product that
/// overflows does not hide a finite result.
fn carried_ratio(carried: f64, current: f64, baseline: f64) -> Option<f64> {
    if baseline <= DEGENERATE_SPAN {
        return None;
    }
    let direct = carried * (current / baseline);
    if direct.is_finite() {
        return Some(direct);
    }
    Some((carried * current) / baseline)
}

impl ScaleState {
    fn index_of(&self, pointer: PointerId) -> Option<usize> {
        self.contacts.iter().position(|c| c.pointer == pointer)
    }

    /// The update the current factors publish.
    fn update_details(&mut self) -> ScaleUpdateDetails {
        self.published_focal = self.focal_point;
        ScaleUpdateDetails {
            focal_point: self.focal_point,
            local_focal_point: self.focal_point,
            scale: self.current.scale,
            horizontal_scale: self.current.horizontal,
            vertical_scale: self.current.vertical,
            rotation: self.rotation,
            pointer_count: self.contacts.len(),
        }
    }

    /// Angle of the line between the two earliest contacts, or `None` while
    /// they coincide.
    fn pair_angle(&self) -> Option<f64> {
        let [first, second, ..] = self.contacts.as_slice() else {
            return None;
        };
        // In coordinates divided by the largest magnitude, so contacts at
        // opposite extremes do not overflow the difference; the angle is
        // unchanged by the scaling.
        let unit = [first.position, second.position]
            .iter()
            .fold(0.0_f64, |largest, p| {
                largest.max(p.dx.abs()).max(p.dy.abs())
            });
        if !unit.is_finite() || unit == 0.0 {
            return None;
        }
        let delta = second.position / unit - first.position / unit;
        (delta.distance() > DEGENERATE_SPAN / unit).then(|| delta.dy.atan2(delta.dx))
    }

    /// Re-measure the baseline after the contact set changed, carrying the
    /// factors reached so far so the published values stay continuous.
    fn rebaseline(&mut self) {
        self.carried = self.current;
        self.baseline = Measure::of(&self.contacts);
        self.rotation_reference = self.pair_angle();
        if let Some(measure) = self.baseline {
            self.focal_point = measure.focal;
        }
    }

    /// Recompute the factors from the current positions. Commits and returns
    /// the measure only when every resulting value is finite.
    fn sample(&mut self) -> Option<Measure> {
        let measure = Measure::of(&self.contacts)?;
        let mut baseline = self.baseline.unwrap_or(measure);
        // A degenerate baseline axis is re-measured from the first usable
        // span, so the factor holds there instead of dividing by zero.
        for (base, now) in [
            (&mut baseline.span, measure.span),
            (&mut baseline.horizontal, measure.horizontal),
            (&mut baseline.vertical, measure.vertical),
        ] {
            if *base <= DEGENERATE_SPAN && now > DEGENERATE_SPAN {
                *base = now;
            }
        }
        let current = Ratios {
            scale: carried_ratio(self.carried.scale, measure.span, baseline.span)
                .unwrap_or(self.current.scale),
            horizontal: carried_ratio(
                self.carried.horizontal,
                measure.horizontal,
                baseline.horizontal,
            )
            .unwrap_or(self.current.horizontal),
            vertical: carried_ratio(self.carried.vertical, measure.vertical, baseline.vertical)
                .unwrap_or(self.current.vertical),
        };
        let angle = self.pair_angle();
        let rotation = match (angle, self.rotation_reference) {
            (Some(angle), Some(reference)) => self.rotation + wrap_angle(angle - reference),
            _ => self.rotation,
        };
        if !(current.scale.is_finite()
            && current.horizontal.is_finite()
            && current.vertical.is_finite()
            && rotation.is_finite())
        {
            return None;
        }
        self.baseline = Some(baseline);
        self.current = current;
        self.rotation = rotation;
        if angle.is_some() {
            self.rotation_reference = angle;
        }
        self.focal_point = measure.focal;
        Some(measure)
    }

    /// Forget the sequence, keeping the start count.
    fn reset(&mut self) {
        *self = Self {
            starts: self.starts,
            ..Self::default()
        };
    }

    /// Start if the recognizer owns the gesture and has two contacts.
    fn try_start(&mut self) -> Option<ScaleStartDetails> {
        if self.phase != ScalePhase::Possible
            || !self.won
            || self.claiming
            || self.contacts.len() < 2
        {
            return None;
        }
        // Never start from a stale focal point: the contacts must measure.
        let measure = Measure::of(&self.contacts)?;
        self.focal_point = measure.focal;
        self.published_focal = measure.focal;
        self.phase = ScalePhase::Started;
        // Only compared for change; wrapping after 2^64 starts is harmless.
        self.starts = self.starts.wrapping_add(1);
        Some(ScaleStartDetails {
            focal_point: self.focal_point,
            local_focal_point: self.focal_point,
            pointer_count: self.contacts.len(),
        })
    }

    fn end_details(&mut self) -> ScaleEndDetails {
        let velocity = self
            .scale_velocity_tracker
            .get_velocity()
            .pixels_per_second
            .dx;
        ScaleEndDetails {
            focal_point: self.published_focal,
            scale: self.current.scale,
            rotation: self.rotation,
            velocity: if velocity.is_finite() { velocity } else { 0.0 },
        }
    }

    /// After a contact left: end a started scale that fell below two
    /// contacts, re-measure otherwise, and go idle once no contact remains.
    /// Returns the final details when a started scale ended.
    fn after_contact_removed(&mut self) -> Option<ScaleEndDetails> {
        let ended = (self.phase == ScalePhase::Started && self.contacts.len() < 2)
            .then(|| self.end_details());
        if self.contacts.is_empty() {
            self.reset();
        } else if ended.is_some() {
            // The next scale on the remaining contacts measures from here.
            self.phase = ScalePhase::Possible;
            self.current = Ratios::IDENTITY;
            self.rotation = 0.0;
            self.scale_velocity_tracker.reset();
            self.rebaseline();
        } else {
            self.rebaseline();
        }
        ended
    }
}

/// What a state transition asks the recognizer to do once the state lock is
/// released.
enum Outcome {
    Nothing,
    Start(ScaleStartDetails),
    Update(ScaleUpdateDetails),
    End(ScaleEndDetails),
    Cancel,
}

impl ScaleGestureRecognizer {
    /// Create a new scale recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(ScaleCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ScaleState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a scale recognizer with custom settings.
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        let recognizer = Self::new(arena);
        *recognizer.settings.lock() = settings;
        recognizer
    }

    /// Replace the gesture settings.
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Set the scale start callback. The replaced callback is released after
    /// the new one is installed.
    pub fn with_on_scale_start(
        self: Arc<Self>,
        callback: impl Fn(ScaleStartDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_start
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the scale update callback. The replaced callback is released after
    /// the new one is installed.
    pub fn with_on_scale_update(
        self: Arc<Self>,
        callback: impl Fn(ScaleUpdateDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_update
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the scale end callback. The replaced callback is released after
    /// the new one is installed.
    pub fn with_on_scale_end(
        self: Arc<Self>,
        callback: impl Fn(ScaleEndDetails) + 'static,
    ) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_end
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Set the scale cancel callback. The replaced callback is released after
    /// the new one is installed.
    pub fn with_on_scale_cancel(self: Arc<Self>, callback: impl Fn() + 'static) -> Arc<Self> {
        let old = self
            .callbacks
            .borrow_mut()
            .on_cancel
            .replace(Rc::new(callback));
        drop(old);
        self
    }

    /// Deliver one outcome to user code. Called with no lock or borrow held.
    fn deliver(&self, outcome: Outcome) {
        match outcome {
            Outcome::Nothing => {}
            Outcome::Start(details) => {
                let callback = self.callbacks.borrow().on_start.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::Update(details) => {
                let callback = self.callbacks.borrow().on_update.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::End(details) => {
                let callback = self.callbacks.borrow().on_end.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::Cancel => {
                let callback = self.callbacks.borrow().on_cancel.clone();
                invoke_callback(callback, || {}, |cb| cb());
            }
        }
    }

    /// Publish the primary (earliest) contact through the base.
    fn sync_primary(&self, primary: Option<PointerId>) {
        self.state.set_primary_pointer(primary);
    }

    /// Give up arena memberships, then deliver `outcome`. The first panic —
    /// from a competitor's arena callback or from the outcome — is resumed
    /// after both have run.
    fn withdraw_then_deliver(&self, entries: Vec<GestureArenaEntry>, outcome: Outcome) {
        let self_driven = self.state.arena().sweep_model() == SweepModel::SelfDriven;
        let mut first = None;
        for entry in entries {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    entry.resolve(GestureDisposition::Rejected);
                    if self_driven {
                        entry.sweep();
                    }
                }),
                "scale arena withdrawal",
            );
        }
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| self.deliver(outcome)),
            "scale callback",
        );
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    /// Claim every listed arena, then start if that won the gesture.
    fn claim(&self, entries: Vec<GestureArenaEntry>) {
        let mut first = None;
        for entry in entries {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted)),
                "scale arena claim",
            );
        }
        // The move that crossed the slop already committed its factors; after the
        // start, an update publishes them, so `End` never reports a factor no
        // update showed.
        let (start, update, starts) = {
            let mut state = self.gesture_state.lock();
            state.claiming = false;
            let start = state.try_start();
            let update = start.is_some().then(|| state.update_details());
            (start, update, state.starts)
        };
        if let Some(details) = start {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(Outcome::Start(details))),
                "scale start callback",
            );
        }
        // `on_start` may have ended (or ended and restarted) the gesture
        // reentrantly; its update then belongs to a gesture that is over.
        let live = || {
            let state = self.gesture_state.lock();
            state.phase == ScalePhase::Started && state.starts == starts
        };
        if let Some(details) = update.filter(|_| live()) {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(Outcome::Update(details))),
                "scale update callback",
            );
        }
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    /// Whether the gesture has moved enough to claim the arena.
    ///
    /// A scale is accepted on absolute span change, **or** focal-point
    /// movement, **or** the scale ratio — any one of them, not all three.
    ///
    /// The arms are not redundant, and dropping any of them loses a whole
    /// class of gesture rather than a little precision:
    ///
    /// - **Ratio alone** cannot see a pinch that starts wide. Fingers 1000 px
    ///   apart moving 40 px further apart are 4% — under the 5% tier — while
    ///   having moved twice any slop.
    /// - **Span arms alone** cannot see a two-finger *pan*. Fingers moving
    ///   together hold both the span and the ratio exactly constant; only the
    ///   focal point moves. This is the arm a two-finger pan rides.
    ///
    /// The span and focal tiers are per-kind (`computeScaleSlop` /
    /// `computePanSlop`); the ratio tier is dimensionless and so has no kind.
    fn should_accept(&self, baseline: Measure, current: Measure, kind: PointerType) -> bool {
        let settings = self.settings.lock();
        if (current.span - baseline.span).abs() > settings.span_slop_for(kind) {
            return true;
        }
        // A degenerate initial span means the pointers started coincident;
        // the ratio is undefined there, so leave that arm to the two distance
        // tiers.
        if let Some(ratio) = carried_ratio(1.0, current.span, baseline.span)
            && settings.exceeds_scale_slop(ratio)
        {
            return true;
        }
        (current.focal - baseline.focal).distance() > settings.pan_slop_for(kind)
    }

    /// Handle a tracked contact's move.
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<f64>, kind: PointerType) {
        if !position.is_finite() {
            return;
        }
        let mut state = self.gesture_state.lock();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        let baseline = state.baseline;
        state.contacts[index].position = position;
        let Some(measure) = state.sample() else {
            return;
        };
        match state.phase {
            ScalePhase::Possible => {
                // Crossing a tier is a request to win, not permission to
                // invoke callbacks: `accept_gesture` (or `claim`) is the start
                // transition, so an observer never sees `on_start` for a
                // gesture a competitor then takes.
                let crossed = baseline.is_some_and(|b| self.should_accept(b, measure, kind));
                if crossed && !state.won {
                    state.claiming = true;
                    let entries = state.contacts.iter().map(|c| c.entry.clone()).collect();
                    drop(state);
                    self.claim(entries);
                }
            }
            ScalePhase::Started => {
                // Read the arena's clock: a headless frame driver binds a
                // `ManualClock`, so a replayed gesture's own sample spacing
                // decides the velocity.
                let now = self.state.now();
                let scale = state.current.scale;
                state
                    .scale_velocity_tracker
                    .add_position(now, Offset::new(scale, 0.0));
                let details = state.update_details();
                drop(state);
                self.deliver(Outcome::Update(details));
            }
            ScalePhase::Idle => {}
        }
    }

    /// Handle a tracked contact lifting.
    fn handle_pointer_up(&self, pointer: PointerId) {
        let mut state = self.gesture_state.lock();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        let contact = state.contacts.remove(index);
        // A contact that lifts before the scale started gives its arena up,
        // so a competitor (a tap) wins it on the sweep instead of this
        // recognizer as the front member. An accepted entry ignores this.
        let started = state.phase == ScalePhase::Started;
        let (withdraw, accepted) = if started {
            (None, Some(contact.entry))
        } else {
            (Some(contact.entry), None)
        };
        let outcome = state
            .after_contact_removed()
            .map_or(Outcome::Nothing, Outcome::End);
        let primary = state.contacts.first().map(|c| c.pointer);
        drop(state);
        self.sync_primary(primary);
        // An accepted contact's arena in a self-driven arena has no binding to
        // sweep it on Up; its entry does, so the slot does not outlive it.
        if let Some(entry) = accepted
            && self.state.arena().sweep_model() == SweepModel::SelfDriven
        {
            entry.sweep();
        }
        self.withdraw_then_deliver(withdraw.into_iter().collect(), outcome);
    }

    /// Handle a cancel for a tracked contact: the whole sequence ends.
    fn handle_cancel(&self, pointer: PointerId) {
        let mut state = self.gesture_state.lock();
        if state.index_of(pointer).is_none() {
            return;
        }
        let started = state.phase == ScalePhase::Started;
        let entries: Vec<_> = std::mem::take(&mut state.contacts)
            .into_iter()
            .map(|c| c.entry)
            .collect();
        state.reset();
        drop(state);
        self.sync_primary(None);
        let outcome = if started {
            Outcome::Cancel
        } else {
            Outcome::Nothing
        };
        // A cancelled sequence's arenas end without a winner.
        let mut first = None;
        for entry in &entries {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| withdraw_cancelled(entry, self.state.arena())),
                "scale arena withdrawal",
            );
        }
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| self.deliver(outcome)),
            "scale callback",
        );
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }
}

impl GestureRecognizer for ScaleGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        // Scale reports a focal point derived from every tracked contact, in
        // the recogniser's own space; a single contact's global position
        // cannot produce it.
        _global_position: Offset<f64>,
    ) {
        if !self.state.assert_not_disposed("add_pointer") || !position.is_finite() {
            return;
        }
        if self.gesture_state.lock().index_of(pointer).is_some() {
            // The same pointer going down again means its Up or Cancel never
            // arrived: that sequence ends here, and the new contact joins the
            // new arena instead of standing in for the old one.
            self.handle_cancel(pointer);
            // The cancel callback may have disposed this recognizer or admitted
            // contacts of its own; either way this admission is void.
            if self.state.is_disposed() || !self.gesture_state.lock().contacts.is_empty() {
                return;
            }
        }
        let member: Arc<dyn GestureArenaMember> = self.clone();
        let entry = self.state.arena().add(pointer, member);

        let mut state = self.gesture_state.lock();
        // A contact added while this recognizer owns the gesture is claimed
        // with it.
        let claim = state.won.then(|| entry.clone());
        state.contacts.push(Contact {
            pointer,
            position,
            entry,
        });
        if state.phase == ScalePhase::Idle {
            state.phase = ScalePhase::Possible;
        }
        state.rebaseline();
        let start = state.try_start();
        let primary = state.contacts.first().map(|c| c.pointer);
        drop(state);
        self.sync_primary(primary);

        let mut first = None;
        if let Some(entry) = claim {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted)),
                "scale arena claim",
            );
        }
        if let Some(details) = start {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| self.deliver(Outcome::Start(details))),
                "scale start callback",
            );
        }
        // Entered while the thread is already unwinding, the failure is retained
        // rather than resumed: a second unwind would abort.
        finish_containment(first, std::thread::panicking());
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Route by the event's own pointer id: a secondary finger's events
        // belong to that finger's contact.
        let pointer = crate::events::extract_pointer_id(event);
        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                self.handle_pointer_move(
                    pointer,
                    Offset::new(pos.x, pos.y),
                    data.pointer.pointer_type,
                );
            }
            PointerEvent::Up(_) => self.handle_pointer_up(pointer),
            PointerEvent::Cancel(_) => self.handle_cancel(pointer),
            _ => {}
        }
    }

    fn dispose(&self) {
        let incoming_failure = std::thread::panicking();
        self.state.mark_disposed();
        let callbacks = std::mem::take(&mut *self.callbacks.borrow_mut());
        let contacts = {
            let mut state = self.gesture_state.lock();
            let contacts = std::mem::take(&mut state.contacts);
            state.reset();
            contacts
        };
        self.sync_primary(None);
        let mut first = None;
        for contact in contacts {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| contact.entry.resolve(GestureDisposition::Rejected)),
                "scale disposal withdrawal",
            );
        }
        callbacks.retire(&mut first);
        finish_containment(first, incoming_failure);
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================

impl crate::recognizers::OneSequenceGestureRecognizer for ScaleGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.gesture_state
            .lock()
            .contacts
            .iter()
            .map(|c| c.pointer)
            .collect()
    }

    fn resolve_pointer(&self, pointer: PointerId, disposition: GestureDisposition) {
        let entry = {
            let state = self.gesture_state.lock();
            state
                .index_of(pointer)
                .map(|i| state.contacts[i].entry.clone())
        };
        if let Some(entry) = entry {
            entry.resolve(disposition);
        }
    }

    fn stop_tracking_pointer(&self, pointer: PointerId) {
        self.handle_pointer_up(pointer);
    }
}

impl GestureArenaMember for ScaleGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let mut state = self.gesture_state.lock();
        if state.index_of(pointer).is_none() {
            return;
        }
        let first_win = !state.won;
        state.won = true;
        if state.claiming {
            // `claim` is resolving the arenas and starts afterwards.
            return;
        }
        if first_win {
            // Winning one contact (a lone member's default win, say) makes
            // the gesture this recognizer's: claim the other contacts too, so
            // a competitor cannot take one of the pinch's fingers.
            state.claiming = true;
            let others = state
                .contacts
                .iter()
                .filter(|c| c.pointer != pointer)
                .map(|c| c.entry.clone())
                .collect();
            drop(state);
            self.claim(others);
        } else {
            let start = state.try_start();
            drop(state);
            if let Some(details) = start {
                self.deliver(Outcome::Start(details));
            }
        }
    }

    fn reject_gesture(&self, pointer: PointerId) {
        // The contact's arena went to a competitor: it no longer belongs to
        // this scale.
        let mut state = self.gesture_state.lock();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        if state.phase == ScalePhase::Started {
            // A started scale that loses one of its contacts is over: it is
            // cancelled, and the remaining contacts are released, exactly as
            // a pointer cancel ends it.
            drop(state);
            self.handle_cancel(pointer);
            return;
        }
        state.contacts.remove(index);
        let outcome = if state.after_contact_removed().is_some() {
            Outcome::Cancel
        } else {
            Outcome::Nothing
        };
        let primary = state.contacts.first().map(|c| c.pointer);
        drop(state);
        self.sync_primary(primary);
        self.deliver(outcome);
    }
}
