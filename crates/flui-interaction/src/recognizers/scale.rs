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
    cell::{Cell, RefCell},
    f64::consts::{PI, TAU},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use web_time::Instant;

use super::{
    callback_containment::{
        finish_containment, invoke_callback, retire_callback, withdraw_cancelled,
    },
    contact::{ArenaMembership, ContactId},
    recognizer::{
        CancelOutcome, EventTimeline, GestureRecognizer, event_time, is_primary_down,
        motion_history,
    },
};
use crate::{
    arena::{GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel},
    events::{PointerEvent, PointerEventExt, PointerType},
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
/// ends with the configured end callback; losing a contact's arena or
/// a pointer cancel ends it with the configured cancel callback
/// instead. A cancel ends the whole sequence.
///
/// Callbacks run after the recognizer has committed its state, with no
/// borrow held, so a callback may cancel and reuse the recognizer.
/// Prefer `Weak` when referring to the owner in a callback. A panic propagates
/// to the dispatcher; the next gesture starts clean. Dropping the last owner
/// silently withdraws each membership before retiring callback captures.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let recognizer = ScaleGestureRecognizer::builder(binding.arena().clone())
///     .on_update(|details| println!("Scale: {:.2}x", details.scale)).build();
/// ```
pub struct ScaleGestureRecognizer {
    membership: ArenaMembership,
    next_contact: Cell<u64>,
    gesture_state: RefCell<ScaleState>,
    settings: GestureSettings,
    callbacks: ScaleCallbacks,
}

impl std::fmt::Debug for ScaleGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizer")
            .field("gesture_state", &*self.gesture_state.borrow())
            .field("settings", &self.settings)
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

impl Drop for ScaleCallbacks {
    fn drop(&mut self) {
        let mut first = None;
        retire_callback(self.on_start.take(), &mut first);
        retire_callback(self.on_update.take(), &mut first);
        retire_callback(self.on_end.take(), &mut first);
        retire_callback(self.on_cancel.take(), &mut first);
        finish_containment(first, std::thread::panicking());
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
    id: ContactId,
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
    /// Identity of the current contact sequence, never reused after retirement.
    sequence: Option<ContactId>,
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
    timeline: EventTimeline,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            sequence: None,
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
            timeline: EventTimeline::default(),
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

    /// Forget the retired sequence.
    fn reset(&mut self) {
        *self = Self::default();
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
        self.sequence = self.contacts.last().map(|contact| contact.id);
        Some(ScaleStartDetails {
            focal_point: self.focal_point,
            local_focal_point: self.focal_point,
            pointer_count: self.contacts.len(),
        })
    }

    fn end_details(&mut self, now: Instant) -> ScaleEndDetails {
        let velocity = self
            .scale_velocity_tracker
            .velocity_at(now)
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
    fn after_contact_removed(&mut self, now: Instant) -> Option<ScaleEndDetails> {
        let ended = (self.phase == ScalePhase::Started && self.contacts.len() < 2)
            .then(|| self.end_details(now));
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

/// Immutable configuration for a shared scale recognizer.
#[must_use]
pub struct ScaleGestureRecognizerBuilder {
    arena: crate::arena::GestureArena,
    settings: GestureSettings,
    callbacks: ScaleCallbacks,
}

impl std::fmt::Debug for ScaleGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizerBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl ScaleGestureRecognizerBuilder {
    /// Freeze device-specific tolerances before contact admission.
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Configure the callback delivered once the scale wins its arenas.
    pub fn on_start(mut self, callback: impl Fn(ScaleStartDetails) + 'static) -> Self {
        self.callbacks.on_start = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered for finite scale samples.
    pub fn on_update(mut self, callback: impl Fn(ScaleUpdateDetails) + 'static) -> Self {
        self.callbacks.on_update = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered when fewer than two contacts remain.
    pub fn on_end(mut self, callback: impl Fn(ScaleEndDetails) + 'static) -> Self {
        self.callbacks.on_end = Some(Rc::new(callback));
        self
    }
    /// Configure the callback delivered when the contact sequence is cancelled.
    pub fn on_cancel(mut self, callback: impl Fn() + 'static) -> Self {
        self.callbacks.on_cancel = Some(Rc::new(callback));
        self
    }
    /// Build the owner; membership keeps only a weak reference to it.
    pub fn build(self) -> Rc<ScaleGestureRecognizer> {
        Rc::new_cyclic(|this: &std::rc::Weak<ScaleGestureRecognizer>| {
            let member: std::rc::Weak<dyn GestureArenaMember> = this.clone();
            ScaleGestureRecognizer {
                membership: ArenaMembership::new(self.arena, member),
                next_contact: Cell::new(0),
                gesture_state: RefCell::new(ScaleState::default()),
                settings: self.settings,
                callbacks: self.callbacks,
            }
        })
    }
}

impl Drop for ScaleGestureRecognizer {
    fn drop(&mut self) {
        for contact in self.gesture_state.get_mut().contacts.drain(..) {
            contact.entry.withdraw_deferred();
        }
    }
}

impl ScaleGestureRecognizer {
    /// Assemble immutable callbacks and gesture policy before sharing the owner.
    pub fn builder(arena: crate::arena::GestureArena) -> ScaleGestureRecognizerBuilder {
        ScaleGestureRecognizerBuilder {
            arena,
            settings: GestureSettings::default(),
            callbacks: ScaleCallbacks::default(),
        }
    }

    /// Deliver one outcome to user code. Called with no lock or borrow held.
    fn deliver(&self, outcome: Outcome) {
        match outcome {
            Outcome::Nothing => {}
            Outcome::Start(details) => {
                let callback = self.callbacks.on_start.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::Update(details) => {
                let callback = self.callbacks.on_update.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::End(details) => {
                let callback = self.callbacks.on_end.clone();
                invoke_callback(callback, || {}, |cb| cb(details));
            }
            Outcome::Cancel => {
                let callback = self.callbacks.on_cancel.clone();
                invoke_callback(callback, || {}, |cb| cb());
            }
        }
    }

    /// Give up arena memberships, then deliver `outcome`. The first panic —
    /// from a competitor's arena callback or from the outcome — is resumed
    /// after both have run.
    fn withdraw_then_deliver(&self, entries: Vec<GestureArenaEntry>, outcome: Outcome) {
        let self_driven = self.membership.arena().sweep_model() == SweepModel::SelfDriven;
        let mut first = None;
        for entry in entries {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    entry.reject_without_self();
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
        let claim_sequence = self.gesture_state.borrow().sequence;
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
        let (start, update, sequence) = {
            let mut state = self.gesture_state.borrow_mut();
            if state.sequence != claim_sequence {
                drop(state);
                finish_containment(first, std::thread::panicking());
                return;
            }
            state.claiming = false;
            let start = state.try_start();
            let update = start.is_some().then(|| state.update_details());
            (start, update, state.sequence)
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
            let state = self.gesture_state.borrow();
            state.phase == ScalePhase::Started && state.sequence == sequence
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
        let settings = &self.settings;
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
    fn handle_pointer_move(
        &self,
        pointer: PointerId,
        position: Offset<f64>,
        kind: PointerType,
        stamp: Option<u64>,
        history: &[(Option<u64>, Offset<f64>)],
    ) {
        if !position.is_finite() {
            return;
        }
        let id = {
            let state = self.gesture_state.borrow();
            let Some(index) = state.index_of(pointer) else {
                return;
            };
            state.contacts[index].id
        };
        let now = self.membership.now();
        let mut state = self.gesture_state.borrow_mut();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        if state.contacts[index].id != id {
            return;
        }
        for &(stamp, position) in history {
            let timestamp = state.timeline.instant(stamp, now);
            state.contacts[index].position = position;
            if state.sample().is_some() && state.phase == ScalePhase::Started {
                let scale = state.current.scale;
                state
                    .scale_velocity_tracker
                    .add_position(timestamp, Offset::new(scale, 0.0));
            }
        }
        let now = state.timeline.instant(stamp, now);
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
    fn handle_pointer_up(&self, pointer: PointerId, stamp: Option<u64>) {
        let id = {
            let state = self.gesture_state.borrow();
            let Some(index) = state.index_of(pointer) else {
                return;
            };
            state.contacts[index].id
        };
        let now = self.membership.now();
        let mut state = self.gesture_state.borrow_mut();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        if state.contacts[index].id != id {
            return;
        }
        let now = state.timeline.instant(stamp, now);
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
            .after_contact_removed(now)
            .map_or(Outcome::Nothing, Outcome::End);
        drop(state);
        // An accepted contact's arena in a self-driven arena has no binding to
        // sweep it on Up; its entry does, so the slot does not outlive it.
        let mut first = None;
        if let Some(entry) = accepted
            && self.membership.arena().sweep_model() == SweepModel::SelfDriven
        {
            first = RoutePanic::capture(|| entry.sweep());
        }
        RoutePanic::preserve_first(
            &mut first,
            RoutePanic::capture(|| {
                self.withdraw_then_deliver(withdraw.into_iter().collect(), outcome)
            }),
            "scale terminal delivery",
        );
        finish_containment(first, std::thread::panicking());
    }

    /// Handle a cancel for a tracked contact: the whole sequence ends.
    fn handle_cancel(&self, pointer: PointerId) {
        let mut state = self.gesture_state.borrow_mut();
        if state.index_of(pointer).is_none() {
            return;
        }
        let started = state.phase == ScalePhase::Started;
        let entries: Vec<_> = std::mem::take(&mut state.contacts)
            .into_iter()
            .map(|c| (c.pointer, c.entry))
            .collect();
        state.reset();
        drop(state);
        let outcome = if started {
            Outcome::Cancel
        } else {
            Outcome::Nothing
        };
        // Only the cancelled contact ends without a winner; other contacts retain rivals.
        let mut first = None;
        for (contact, entry) in &entries {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| {
                    if *contact == pointer {
                        withdraw_cancelled(entry, self.membership.arena());
                    } else {
                        entry.reject_without_self();
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
}

impl GestureRecognizer for ScaleGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        if !is_primary_down(down.local) || down.local.pointer_id() != down.global.pointer_id() {
            return;
        }
        let pointer = down.local.pointer_id();
        let position = down.local.position();
        if !position.is_finite()
            || !down.global.position().is_finite()
            || self.gesture_state.borrow().index_of(pointer).is_some()
        {
            return;
        }
        let Some(id) = ContactId::next(&self.next_contact) else {
            return;
        };
        let Some(entry) = self.membership.join(pointer) else {
            return;
        };

        let mut state = self.gesture_state.borrow_mut();
        // A contact added while this recognizer owns the gesture is claimed
        // with it.
        let claim = state.won.then(|| entry.clone());
        state.contacts.push(Contact {
            id,
            pointer,
            position,
            entry,
        });
        if state.phase == ScalePhase::Idle {
            state.phase = ScalePhase::Possible;
            state.sequence = Some(id);
        }
        state.rebaseline();
        let start = state.try_start();
        let sequence = state.sequence;
        drop(state);

        let mut first = None;
        if let Some(entry) = claim {
            RoutePanic::preserve_first(
                &mut first,
                RoutePanic::capture(|| entry.resolve(GestureDisposition::Accepted)),
                "scale arena claim",
            );
        }
        let live = {
            let state = self.gesture_state.borrow();
            state.phase == ScalePhase::Started
                && state.sequence == sequence
                && state.contacts.iter().any(|contact| contact.id == id)
        };
        if let Some(details) = start.filter(|_| live) {
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
        // Route by the event's own pointer id: a secondary finger's events
        // belong to that finger's contact.
        let pointer = crate::events::extract_pointer_id(event);
        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let history = motion_history(event);
                self.handle_pointer_move(
                    pointer,
                    Offset::new(pos.x, pos.y),
                    data.pointer.pointer_type,
                    event_time(event),
                    &history,
                );
            }
            PointerEvent::Up(_) => self.handle_pointer_up(pointer, event_time(event)),
            PointerEvent::Cancel(_) => self.handle_cancel(pointer),
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        let pointer = self
            .gesture_state
            .borrow()
            .contacts
            .first()
            .map(|contact| contact.pointer);
        let Some(pointer) = pointer else {
            return CancelOutcome::Idle;
        };
        self.handle_cancel(pointer);
        CancelOutcome::Cancelled
    }
}

impl GestureArenaMember for ScaleGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let mut state = self.gesture_state.borrow_mut();
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
        let id = {
            let state = self.gesture_state.borrow();
            let Some(index) = state.index_of(pointer) else {
                return;
            };
            state.contacts[index].id
        };
        let now = self.membership.now();
        let mut state = self.gesture_state.borrow_mut();
        let Some(index) = state.index_of(pointer) else {
            return;
        };
        if state.contacts[index].id != id {
            return;
        }
        if state.phase == ScalePhase::Started {
            // A started scale that loses one of its contacts is over: it is
            // cancelled, and the remaining contacts are released, exactly as
            // a pointer cancel ends it.
            drop(state);
            self.handle_cancel(pointer);
            return;
        }
        state.contacts.remove(index);
        let now = state.timeline.instant(None, now);
        let outcome = if state.after_contact_removed(now).is_some() {
            Outcome::Cancel
        } else {
            Outcome::Nothing
        };
        drop(state);
        self.deliver(outcome);
    }
}
