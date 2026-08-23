//! Scripted pointer input, replayed on the virtual clock.
//!
//! A gesture is a *shape in time*: a long press is a contact that outlives a
//! deadline, a fling is a contact whose samples are close enough together to
//! build velocity, a double tap is two taps inside a window. Replaying one
//! therefore means replaying its timing, not just its events.
//!
//! This module supersedes the recorder/player that used to live in
//! `flui_interaction::testing::recording`, which could not do that and had no
//! consumers as a result. Three things were wrong with it, and all three are
//! about time or fidelity:
//!
//! - it stamped each recorded event with `Instant::now()`, so a "recording"
//!   encoded whatever the test process's scheduling happened to be — the one
//!   thing a deterministic harness must never depend on;
//! - its player was an `Iterator<Item = PointerEvent>` that dropped the
//!   timing entirely: nothing advanced a clock between events, so a
//!   `long_press` recording replayed as an instant tap and a `double_tap` as
//!   two taps with no gap between them;
//! - it hand-built `PointerEventData` with `pressure: 1.0` and a
//!   `PointerId::PRIMARY` hardcoded into capture, so multi-touch recordings
//!   were corrupted on the way in and single-touch ones asserted against a
//!   contract no real wire meets.
//!
//! Here, a [`PointerScript`] is authored data with explicit virtual-time
//! offsets, and [`HeadlessBinding::replay`] walks it by advancing the binding's
//! virtual clock to each offset — pumping real frames, so deadlines fire and
//! animations tick exactly as they would on screen — and dispatching through
//! the same [`GestureBinding`](flui_interaction::GestureBinding) input pipeline
//! production uses.
//!
//! # Example
//!
//! ```rust,ignore
//! let mut binding = HeadlessBinding::new();
//! // …register a long-press recognizer on the binding's arena…
//!
//! // 600ms of virtual time elapse inside the replay, so the 500ms deadline
//! // fires — the thing the old iterator-based player could not express.
//! binding.replay(&PointerScript::long_press(
//!     Offset::new(px(10.0), px(10.0)),
//!     Duration::from_millis(600),
//! ));
//! ```

use std::time::Duration;

use flui_interaction::events::{
    PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    make_up_event_for_id,
};
use flui_interaction::{HitTestResult, PointerEvent, PointerId};
use flui_types::geometry::{Offset, Pixels, px};

use crate::HeadlessBinding;

/// Which pointer phase a scripted event carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    /// A contact begins.
    Down,
    /// A tracked contact moves.
    Move,
    /// A contact ends normally.
    Up,
    /// A contact is taken away by the system (a system gesture, a lost window).
    Cancel,
}

/// One scripted pointer event, at an explicit offset from the script's start.
///
/// The offset is *virtual* time. Nothing here samples a wall clock: a script is
/// authored, so replaying it twice produces the same timing on any machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedPointer {
    /// Virtual time from the start of the script at which this event is
    /// dispatched.
    pub at: Duration,
    /// Which contact this event belongs to. Distinct contacts must use
    /// distinct ids — a platform never recycles an id into a still-tracked
    /// gesture, and neither should a script.
    pub pointer: PointerId,
    /// The phase.
    pub phase: PointerPhase,
    /// Position in logical pixels.
    pub position: Offset<Pixels>,
    /// Device kind. Slop thresholds and velocity policy differ per kind, so a
    /// script that means "touch" must say so.
    pub device: PointerType,
}

impl ScriptedPointer {
    /// A scripted event with the touch device kind.
    #[must_use]
    pub fn new(
        at: Duration,
        pointer: PointerId,
        phase: PointerPhase,
        position: Offset<Pixels>,
    ) -> Self {
        Self {
            at,
            pointer,
            phase,
            position,
            device: PointerType::Touch,
        }
    }

    /// Same event, attributed to a different device kind.
    #[must_use]
    pub fn with_device(mut self, device: PointerType) -> Self {
        self.device = device;
        self
    }

    /// The production [`PointerEvent`] this scripts.
    ///
    /// Built through `flui_interaction`'s own event constructors — the same
    /// ones every synthetic-input test uses — rather than by assembling
    /// `PointerEventData` by hand, so a script cannot drift from the shape a
    /// real platform translation produces.
    #[must_use]
    pub fn to_event(self) -> PointerEvent {
        match self.phase {
            PointerPhase::Down => make_down_event_for_id(self.pointer, self.position, self.device),
            PointerPhase::Move => make_move_event_for_id(self.pointer, self.position, self.device),
            PointerPhase::Up => make_up_event_for_id(self.pointer, self.position, self.device),
            PointerPhase::Cancel => make_cancel_event_for_id(self.pointer, self.device),
        }
    }
}

/// A time-ordered script of pointer events.
///
/// Build one from the presets below, or event by event with
/// [`push`](Self::push). Events must be pushed in non-decreasing time order;
/// [`HeadlessBinding::replay`] asserts it rather than silently replaying a
/// script backwards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointerScript {
    name: String,
    events: Vec<ScriptedPointer>,
}

impl PointerScript {
    /// An empty script under `name`. The name appears in replay assertion
    /// failures, so make it say what the gesture is.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            events: Vec::new(),
        }
    }

    /// Append an event.
    ///
    /// # Panics
    ///
    /// If `event.at` precedes the previous event's offset. A script is a
    /// timeline; an out-of-order push is an authoring mistake that would
    /// otherwise surface as an unexplainable replay.
    pub fn push(&mut self, event: ScriptedPointer) {
        if let Some(last) = self.events.last() {
            assert!(
                event.at >= last.at,
                "script {:?}: event at {:?} precedes the previous event at {:?}; \
                 a script must be in non-decreasing time order",
                self.name,
                event.at,
                last.at,
            );
        }
        self.events.push(event);
    }

    /// [`push`](Self::push), chainable.
    #[must_use]
    pub fn with(mut self, event: ScriptedPointer) -> Self {
        self.push(event);
        self
    }

    /// This script's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The scripted events, in time order.
    #[must_use]
    pub fn events(&self) -> &[ScriptedPointer] {
        &self.events
    }

    /// Number of events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the script has no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Virtual time spanned, i.e. the last event's offset.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.events.last().map_or(Duration::ZERO, |e| e.at)
    }

    /// Re-attribute every event to `device`.
    #[must_use]
    pub fn on_device(mut self, device: PointerType) -> Self {
        for event in &mut self.events {
            event.device = device;
        }
        self
    }

    // ---------------------------------------------------------------- presets

    /// Down then up at `position`, 50 ms apart — inside any tap window.
    #[must_use]
    pub fn tap(position: Offset<Pixels>) -> Self {
        Self::new("tap")
            .with(ScriptedPointer::new(
                Duration::ZERO,
                PointerId::PRIMARY,
                PointerPhase::Down,
                position,
            ))
            .with(ScriptedPointer::new(
                Duration::from_millis(50),
                PointerId::PRIMARY,
                PointerPhase::Up,
                position,
            ))
    }

    /// Two taps at `position` separated by `gap` between the first up and the
    /// second down.
    ///
    /// The gap is a parameter, not a constant, because it is exactly what a
    /// double-tap test is about: the same script decides the recognizer's
    /// verdict on either side of its window.
    #[must_use]
    pub fn double_tap(position: Offset<Pixels>, gap: Duration) -> Self {
        let second = Duration::from_millis(50) + gap;
        Self::new("double_tap")
            .with(ScriptedPointer::new(
                Duration::ZERO,
                PointerId::PRIMARY,
                PointerPhase::Down,
                position,
            ))
            .with(ScriptedPointer::new(
                Duration::from_millis(50),
                PointerId::PRIMARY,
                PointerPhase::Up,
                position,
            ))
            // A platform never recycles a contact id into a still-tracked
            // gesture, so the second tap is its own contact.
            .with(ScriptedPointer::new(
                second,
                secondary_pointer(),
                PointerPhase::Down,
                position,
            ))
            .with(ScriptedPointer::new(
                second + Duration::from_millis(50),
                secondary_pointer(),
                PointerPhase::Up,
                position,
            ))
    }

    /// A stationary contact held for `hold` before lifting.
    ///
    /// `hold` is the whole point: whether the long-press deadline fires is
    /// decided by how much virtual time the replay spends here.
    #[must_use]
    pub fn long_press(position: Offset<Pixels>, hold: Duration) -> Self {
        Self::new("long_press")
            .with(ScriptedPointer::new(
                Duration::ZERO,
                PointerId::PRIMARY,
                PointerPhase::Down,
                position,
            ))
            .with(ScriptedPointer::new(
                hold,
                PointerId::PRIMARY,
                PointerPhase::Up,
                position,
            ))
    }

    /// A drag from `start` to `end` in `steps` linearly-interpolated moves,
    /// each `sample` apart.
    ///
    /// `sample` sets the velocity the gesture carries: the same path replayed
    /// with a 4 ms sample flings and with a 200 ms sample does not. Real
    /// hardware reports around 8 ms, which is what [`fling`](Self::fling) and
    /// [`swipe`](Self::swipe) use.
    ///
    /// # Panics
    ///
    /// If `steps` is zero — a drag with no move samples is a tap with a
    /// misleading name.
    #[must_use]
    pub fn drag(
        start: Offset<Pixels>,
        end: Offset<Pixels>,
        steps: usize,
        sample: Duration,
    ) -> Self {
        assert!(steps > 0, "a drag needs at least one move sample");
        let mut script = Self::new("drag").with(ScriptedPointer::new(
            Duration::ZERO,
            PointerId::PRIMARY,
            PointerPhase::Down,
            start,
        ));
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let position = Offset::new(
                start.dx + (end.dx - start.dx) * t,
                start.dy + (end.dy - start.dy) * t,
            );
            script.push(ScriptedPointer::new(
                sample * step as u32,
                PointerId::PRIMARY,
                PointerPhase::Move,
                position,
            ));
        }
        script.push(ScriptedPointer::new(
            sample * (steps + 1) as u32,
            PointerId::PRIMARY,
            PointerPhase::Up,
            end,
        ));
        script
    }

    /// A drag sampled at hardware cadence (8 ms), fast enough to build the
    /// velocity a fling recognizer needs.
    #[must_use]
    pub fn fling(start: Offset<Pixels>, end: Offset<Pixels>) -> Self {
        let mut script = Self::drag(start, end, 6, Duration::from_millis(8));
        script.name = "fling".to_string();
        script
    }

    /// Alias for [`fling`](Self::fling) under the name a UI test usually gives
    /// it.
    #[must_use]
    pub fn swipe(start: Offset<Pixels>, end: Offset<Pixels>) -> Self {
        let mut script = Self::fling(start, end);
        script.name = "swipe".to_string();
        script
    }

    /// A two-finger pinch about `center`, from `start_distance` to
    /// `end_distance` along the horizontal axis in `steps` samples.
    ///
    /// # Panics
    ///
    /// If `steps` is zero.
    #[must_use]
    pub fn pinch(
        center: Offset<Pixels>,
        start_distance: f32,
        end_distance: f32,
        steps: usize,
    ) -> Self {
        assert!(steps > 0, "a pinch needs at least one move sample");
        let sample = Duration::from_millis(8);
        let first = PointerId::PRIMARY;
        let second = secondary_pointer();
        let pair = |distance: f32| {
            let half = px(distance / 2.0);
            (
                Offset::new(center.dx - half, center.dy),
                Offset::new(center.dx + half, center.dy),
            )
        };

        let (start_left, start_right) = pair(start_distance);
        let mut script = Self::new("pinch")
            .with(ScriptedPointer::new(
                Duration::ZERO,
                first,
                PointerPhase::Down,
                start_left,
            ))
            // The second finger lands after the first: two simultaneous downs
            // is a shape no real digitizer produces.
            .with(ScriptedPointer::new(
                sample,
                second,
                PointerPhase::Down,
                start_right,
            ));

        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let distance = start_distance + (end_distance - start_distance) * t;
            let (left, right) = pair(distance);
            let at = sample * (step + 1) as u32;
            script.push(ScriptedPointer::new(at, first, PointerPhase::Move, left));
            script.push(ScriptedPointer::new(at, second, PointerPhase::Move, right));
        }

        let (end_left, end_right) = pair(end_distance);
        let at = sample * (steps + 2) as u32;
        script.push(ScriptedPointer::new(at, first, PointerPhase::Up, end_left));
        script.push(ScriptedPointer::new(
            at,
            second,
            PointerPhase::Up,
            end_right,
        ));
        script
    }
}

/// The second contact id used by the multi-contact presets.
fn secondary_pointer() -> PointerId {
    PointerId::new(2).expect("2 is a valid pointer id")
}

/// Records the pointer events a test dispatches, stamped on a binding's
/// **virtual** clock, into a replayable [`PointerScript`].
///
/// Round-trip fidelity is the point: a script recorded from one run replays to
/// the same timing, because the offsets come from the same clock the replay
/// advances — not from `Instant::now()`, which made the predecessor's
/// recordings unreproducible by construction.
#[derive(Debug)]
pub struct GestureRecorder {
    script: PointerScript,
    start: Option<Duration>,
}

impl GestureRecorder {
    /// A recorder building a script under `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            script: PointerScript::new(name),
            start: None,
        }
    }

    /// Record `event` at `binding`'s current virtual time.
    ///
    /// The first recorded event anchors the script at offset zero.
    pub fn record(
        &mut self,
        binding: &HeadlessBinding,
        pointer: PointerId,
        phase: PointerPhase,
        position: Offset<Pixels>,
        device: PointerType,
    ) {
        let now = binding.clock().elapsed();
        let start = *self.start.get_or_insert(now);
        self.script.push(
            ScriptedPointer::new(now.saturating_sub(start), pointer, phase, position)
                .with_device(device),
        );
    }

    /// The script recorded so far.
    #[must_use]
    pub fn script(&self) -> &PointerScript {
        &self.script
    }

    /// Finish and take the script.
    #[must_use]
    pub fn finish(self) -> PointerScript {
        self.script
    }
}

impl HeadlessBinding {
    /// Replay `script` against this binding's own tree.
    ///
    /// Hit-testing routes through the bound pipeline owner; on a gesture-only
    /// binding every position misses, which is the right answer for driving a
    /// recognizer registered directly on the arena.
    ///
    /// See [`replay_with`](Self::replay_with) for the mechanics — this is that
    /// method over [`hit_test`](Self::hit_test).
    pub fn replay(&mut self, script: &PointerScript) {
        self.replay_with(script, Self::hit_test);
    }

    /// [`replay`](Self::replay) with a caller-supplied hit-test source, for a
    /// harness that hit-tests through its own view of the render tree.
    ///
    /// For each scripted event, in order:
    ///
    /// 1. [`pump_frame`](Self::pump_frame) by the gap since the previous event,
    ///    so virtual time genuinely passes — deadlines fire, controllers tick,
    ///    and the tree rebuilds, exactly as between two real events;
    /// 2. dispatch the event through
    ///    [`dispatch_pointer`](Self::dispatch_pointer), the same
    ///    `GestureBinding` pipeline production routes through.
    ///
    /// Note what step 1 means: the replay's cost in virtual time is the
    /// script's own duration, and a script whose events share an offset (both
    /// fingers of a pinch sample) pumps a zero-length frame between them rather
    /// than collapsing them into one dispatch.
    ///
    /// The frame *after* the last event is left to the caller: whether a
    /// gesture's effect should be observed before or after one more frame is
    /// the assertion's business, not the replay's.
    ///
    /// # Panics
    ///
    /// If `script` is not in non-decreasing time order.
    pub fn replay_with(
        &mut self,
        script: &PointerScript,
        mut hit_test: impl FnMut(&Self, Offset<Pixels>) -> HitTestResult,
    ) {
        let mut previous = Duration::ZERO;
        for event in script.events() {
            assert!(
                event.at >= previous,
                "script {:?}: event at {:?} precedes the previous event at {:?}",
                script.name(),
                event.at,
                previous,
            );
            // Non-negative by the assertion above; `saturating_sub` keeps that
            // fact total rather than relying on an unwrap.
            self.pump_frame(event.at.saturating_sub(previous));
            previous = event.at;

            let pointer_event = event.to_event();
            // One immutable reborrow serves both the dispatch and the hit-test
            // closure; `pump_frame` above already had its exclusive turn.
            let binding: &Self = self;
            binding.dispatch_pointer(&pointer_event, |position| hit_test(binding, position));
        }
    }

    /// Hit-test `position` against this binding's bound render tree.
    ///
    /// An empty result on a gesture-only binding: there is no tree to hit.
    #[must_use]
    pub fn hit_test(&self, position: Offset<Pixels>) -> HitTestResult {
        let mut result = HitTestResult::new();
        if let Some(pipeline_owner) = self.pipeline_owner() {
            pipeline_owner.with(|owner| owner.hit_test(position, &mut result));
        }
        result
    }
}
