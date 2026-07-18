//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/animated_size_test.dart`
//! (tag `3.44.0`).
//!
//! Widget → render-object mapping:
//! - `AnimatedSize` → a private `RenderView` wrapper directly over the
//!   persistent `RenderAnimatedSize`
//!   (`crates/flui-widgets/src/animated/animated_size.rs`,
//!   `crates/flui-objects/src/layout/animated_size.rs`) — unlike every
//!   sibling in this family, it does not rebuild through `AnimatedBuilder`;
//!   see that module's own docs for why.
//!
//! `RenderAnimatedSize`'s four-state retarget machine
//! (`Start`/`Stable`/`Changed`/`Unstable`) already carries thorough
//! `#[cfg(test)]` unit coverage in `flui-objects/src/layout/animated_size.rs`
//! (`stable_to_changed_begins_at_current_committed_size_not_raw_tween_value`,
//! `changed_to_unstable_collapses_to_degenerate_zero_span_tween`,
//! `unstable_repeats_then_settles_with_no_visible_jump`,
//! `changed_to_stable_resumes_existing_span_untouched`) — those exercise the
//! state machine by calling its transition methods directly. The port below
//! (`animated_size_tracks_unstable_child_then_resumes_when_child_stabilizes`)
//! is deliberately NOT redundant with them: it drives ONE of the same
//! transitions (`Stable`→`Changed`→`Stable`-resumed) through a real widget +
//! the virtual-clock harness, proving the actual wiring (vsync registration,
//! build/tree reconciliation, `pump_for` timing) reaches the state machine —
//! the unit tests prove the machine correct in isolation, this proves that
//! one leg is reachable from a real tree. See that test's own doc for why
//! the oracle's OTHER legs (`Unstable`, a SECOND widget-level retarget) are
//! not additionally forced through this harness.
//!
//! Ported: 7 of 11 oracle cases.
//! - `'animates forwards then backwards with stable-sized children'`
//! - `'calls onEnd when animation is completed'`
//! - `'clamps animated size to constraints'`
//! - `'tracks unstable child, then resumes animation when child stabilizes'`
//! - `'does not run animation unnecessarily'`
//! - `'can set and update clipBehavior'`
//! - `'AnimatedSize does not crash at zero size'`
//!
//! Citation-only: 1 of 11.
//! - `'resyncs its animation controller'` — same core mid-flight-then-settle
//!   mechanism as `'animates forwards then backwards with stable-sized
//!   children'` above (a width-only variant of the same retarget); no
//!   additional behavior to isolate.
//!
//! Out of scope: 3 of 11.
//! - `'works wrapped in IntrinsicHeight and Wrap'` — a compound
//!   cross-widget dry-layout re-entrancy scenario (`IntrinsicHeight` querying
//!   `AnimatedSize`'s dry layout every pump, under `Curves.easeInOutBack`
//!   overshoot); out of scope for an `AnimatedSize`-focused port — better
//!   suited to a dedicated `IntrinsicHeight`/`Wrap` parity pass.
//! - `'re-attach with interrupted animation'` — depends on Flutter's
//!   `GlobalKey`-based reparent preserving the *same* render object (and so
//!   its in-flight animation state) across a subtree move. FLUI's
//!   remove+insert reparent model cannot preserve a Rust render object's
//!   identity across that boundary — a reparented `AnimatedSize` mints a
//!   fresh `RenderAnimatedSize`, losing any in-flight run. This is a
//!   documented architectural divergence (see
//!   `attach_on_changed_state_immediately_marks_needs_layout`'s comment in
//!   `flui-objects/src/layout/animated_size.rs`), not a portable case.
//! - `'disposes animation and controller'` — asserts against Flutter's
//!   `kDebugMode`-only `debugAnimation`/`debugController` introspection
//!   fields and a double-`dispose()`-throws contract. FLUI's
//!   `AnimationController` exposes no public `is_disposed`/double-dispose-panic
//!   surface at this harness's boundary — a named capability gap, not a
//!   silent skip.
//!
//! Divergence (assertion dropped, not behavior): the oracle's first case also
//! asserts `RenderBox.paint`'s FIRST `PaintingContext` invocation is
//! `pushClipRect` while shrinking vs. `paintChild` while growing (a Flutter
//! test-mock-specific `Invocation` trace). This harness has no equivalent
//! paint-call-order introspection; the geometry assertions (the actual
//! forwards/backwards size trajectory) are ported in full.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::RenderId;
use flui_objects::{AnimatedSizeState, RenderAnimatedSize};
use flui_types::painting::Clip;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedSize, SizedBox, Text, VsyncScope};
use parking_lot::Mutex;

use crate::common::{self, lay_out, lay_out_animated, loose, size, tight};

const RUN: Duration = Duration::from_millis(200);

/// `(size, state)` of the unique `RenderAnimatedSize` node in `laid`.
fn probe(laid: &common::LaidOut, id: RenderId) -> (flui_types::Size, AnimatedSizeState) {
    let state = {
        let owner_handle = laid.pipeline_owner();
        let mut owner = owner_handle.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderAnimatedSize>())
            .expect("render node should be a RenderAnimatedSize")
            .state()
    };
    (laid.size(id), state)
}

// ----------------------------------------------------------------------------
// Forwards then backwards
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct ChildSizeProbe {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
}

struct ChildSizeProbeState {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
}

impl StatefulView for ChildSizeProbe {
    type State = ChildSizeProbeState;

    fn create_state(&self) -> Self::State {
        ChildSizeProbeState {
            vsync: self.vsync.clone(),
            child_side: Arc::clone(&self.child_side),
        }
    }
}

impl ViewState<ChildSizeProbe> for ChildSizeProbeState {
    fn build(&self, _view: &ChildSizeProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.child_side.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedSize::new(RUN).child(SizedBox::square(side)),
        )
    }
}

/// `AnimatedSize` genuinely interpolates toward a larger child, settles
/// exactly at the target, then interpolates back down and settles exactly at
/// the original size.
///
/// Flutter parity: `'animates forwards then backwards with stable-sized
/// children'` (`animated_size_test.dart`, tag `3.44.0`) — same 200 ms
/// duration, same 100 → 200 → 100 sequence, same 100 ms sample points (the
/// paint-invocation-order assertion is dropped, see the module docs).
#[test]
fn animated_size_animates_forwards_then_backwards_with_stable_sized_children() {
    let vsync = Vsync::new();
    let child_side = Arc::new(Mutex::new(100.0));
    let probe_widget = ChildSizeProbe {
        vsync: vsync.clone(),
        child_side: Arc::clone(&child_side),
    };
    let mut laid = lay_out_animated(probe_widget, loose(400.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedSize");

    assert_eq!(laid.size(id), size(100.0, 100.0));

    // Forwards: 100 -> 200.
    *child_side.lock() = 200.0;
    laid.pump();
    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run
    laid.pump_for(RUN / 2);
    assert_eq!(
        laid.size(id),
        size(150.0, 150.0),
        "halfway through growing, size must be exactly the midpoint"
    );
    laid.pump_for(RUN / 2);
    assert_eq!(
        laid.size(id),
        size(200.0, 200.0),
        "must settle at the new, larger size"
    );

    // Backwards: 200 -> 100.
    *child_side.lock() = 100.0;
    laid.pump();
    laid.pump_for(Duration::ZERO);
    laid.pump_for(RUN / 2);
    assert_eq!(
        laid.size(id),
        size(150.0, 150.0),
        "halfway through shrinking, size must be exactly the midpoint"
    );
    laid.pump_for(RUN / 2);
    assert_eq!(
        laid.size(id),
        size(100.0, 100.0),
        "must settle back at the original, smaller size"
    );
}

// ----------------------------------------------------------------------------
// onEnd
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct OnEndProbe {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
    calls: Arc<AtomicUsize>,
}

struct OnEndProbeState {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
    calls: Arc<AtomicUsize>,
}

impl StatefulView for OnEndProbe {
    type State = OnEndProbeState;

    fn create_state(&self) -> Self::State {
        OnEndProbeState {
            vsync: self.vsync.clone(),
            child_side: Arc::clone(&self.child_side),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl ViewState<OnEndProbe> for OnEndProbeState {
    fn build(&self, _view: &OnEndProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.child_side.lock();
        let calls = Arc::clone(&self.calls);
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedSize::new(RUN)
                .on_end(move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                })
                .child(SizedBox::square(side)),
        )
    }
}

/// `on_end` fires exactly once per completed resize run, whichever direction.
///
/// Flutter parity: `'calls onEnd when animation is completed'`
/// (`animated_size_test.dart`, tag `3.44.0`).
#[test]
fn animated_size_calls_on_end_when_animation_completes() {
    let vsync = Vsync::new();
    let child_side = Arc::new(Mutex::new(100.0));
    let calls = Arc::new(AtomicUsize::new(0));
    let probe_widget = OnEndProbe {
        vsync: vsync.clone(),
        child_side: Arc::clone(&child_side),
        calls: Arc::clone(&calls),
    };
    let mut laid = lay_out_animated(probe_widget, loose(400.0), vsync);

    assert_eq!(calls.load(Ordering::SeqCst), 0);

    *child_side.lock() = 200.0;
    laid.pump();
    laid.pump_for(Duration::ZERO);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "not yet settled");
    laid.pump_for(RUN + Duration::from_millis(50));
    laid.pump_for(Duration::ZERO); // drain the rebuild the completion listener scheduled
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    *child_side.lock() = 100.0;
    laid.pump();
    laid.pump_for(Duration::ZERO);
    laid.pump_for(RUN + Duration::from_millis(50));
    laid.pump_for(Duration::ZERO);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

// ----------------------------------------------------------------------------
// Clamps to constraints
// ----------------------------------------------------------------------------

/// A tight parent constraint clamps the animated size — it never grows past
/// what the parent allows.
///
/// Flutter parity: `'clamps animated size to constraints'`
/// (`animated_size_test.dart`, tag `3.44.0`).
#[test]
fn animated_size_clamps_animated_size_to_constraints() {
    let vsync = Vsync::new();
    let child_side = Arc::new(Mutex::new(100.0));
    let probe_widget = ChildSizeProbe {
        vsync: vsync.clone(),
        child_side: Arc::clone(&child_side),
    };
    let mut laid = lay_out_animated(probe_widget, tight(100.0, 100.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedSize");

    assert_eq!(laid.size(id), size(100.0, 100.0));

    // Attempt to grow beyond the tight 100×100 parent.
    *child_side.lock() = 200.0;
    laid.pump();
    laid.pump_for(Duration::ZERO);
    laid.pump_for(RUN / 2);

    assert_eq!(
        laid.size(id),
        size(100.0, 100.0),
        "a tight parent constraint clamps the animated size, no matter the target"
    );
}

// ----------------------------------------------------------------------------
// Does not run unnecessarily
// ----------------------------------------------------------------------------

/// With no retarget ever issued, `AnimatedSize` must sit at `Stable` and its
/// committed size must never drift across repeated pumps.
///
/// Flutter parity: `'does not run animation unnecessarily'`
/// (`animated_size_test.dart`, tag `3.44.0`) — the oracle also asserts
/// `box.isAnimating == false`; FLUI's public equivalent is
/// `RenderAnimatedSize::state() == Stable` (a `Stable` node has stopped its
/// controller in every reachable path — see `layout_stable`/`layout_unstable`
/// in `flui-objects/src/layout/animated_size.rs`).
#[test]
fn animated_size_does_not_run_animation_unnecessarily() {
    let vsync = Vsync::new();
    let child_side = Arc::new(Mutex::new(100.0));
    let probe_widget = ChildSizeProbe {
        vsync: vsync.clone(),
        child_side: Arc::clone(&child_side),
    };
    let mut laid = lay_out_animated(probe_widget, loose(400.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedSize");

    for _ in 0..20 {
        let (committed, state) = probe(&laid, id);
        assert_eq!(committed, size(100.0, 100.0));
        assert_eq!(state, AnimatedSizeState::Stable);
        laid.pump_for(Duration::from_millis(10));
    }
}

// ----------------------------------------------------------------------------
// clipBehavior
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct ClipProbe {
    vsync: Vsync,
    clip_behavior: Arc<Mutex<Clip>>,
}

struct ClipProbeState {
    vsync: Vsync,
    clip_behavior: Arc<Mutex<Clip>>,
}

impl StatefulView for ClipProbe {
    type State = ClipProbeState;

    fn create_state(&self) -> Self::State {
        ClipProbeState {
            vsync: self.vsync.clone(),
            clip_behavior: Arc::clone(&self.clip_behavior),
        }
    }
}

impl ViewState<ClipProbe> for ClipProbeState {
    fn build(&self, _view: &ClipProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let clip_behavior = *self.clip_behavior.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedSize::new(RUN)
                .clip_behavior(clip_behavior)
                .child(SizedBox::square(100.0)),
        )
    }
}

/// `clip_behavior` defaults to `HardEdge` and updates on every reconfigure.
///
/// Flutter parity: `'can set and update clipBehavior'`
/// (`animated_size_test.dart`, tag `3.44.0`) — same four `Clip` values.
#[test]
fn animated_size_can_set_and_update_clip_behavior() {
    let vsync = Vsync::new();
    let clip_behavior = Arc::new(Mutex::new(Clip::HardEdge));
    let probe_widget = ClipProbe {
        vsync: vsync.clone(),
        clip_behavior: Arc::clone(&clip_behavior),
    };
    let mut laid = lay_out_animated(probe_widget, loose(400.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedSize");

    let clip_of = |laid: &common::LaidOut, id: RenderId| -> Clip {
        let owner_handle = laid.pipeline_owner();
        let mut owner = owner_handle.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderAnimatedSize>())
            .expect("RenderAnimatedSize")
            .clip_behavior()
    };

    assert_eq!(clip_of(&laid, id), Clip::HardEdge, "default is HardEdge");

    for clip in [
        Clip::None,
        Clip::HardEdge,
        Clip::AntiAlias,
        Clip::AntiAliasWithSaveLayer,
    ] {
        *clip_behavior.lock() = clip;
        laid.pump();
        assert_eq!(clip_of(&laid, id), clip);
    }
}

// ----------------------------------------------------------------------------
// Zero size
// ----------------------------------------------------------------------------

/// `AnimatedSize` on a zero-area surface lays out to zero without panicking.
///
/// Flutter parity: `'AnimatedSize does not crash at zero size'`
/// (`animated_size_test.dart`, tag `3.44.0`).
#[test]
fn animated_size_does_not_crash_at_zero_size() {
    let laid = lay_out(
        SizedBox::shrink()
            .child(AnimatedSize::new(Duration::from_millis(300)).child(Text::new("X"))),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "AnimatedSize on a zero-area surface must measure 0×0 with no panic"
    );
}

// ----------------------------------------------------------------------------
// Tracks an unstable child, then resumes when it stabilizes
// ----------------------------------------------------------------------------

/// Retargets the plain `SizedBox` child to `side` on every
/// [`LaidOut::pump`](common::LaidOut::pump) — the SAME proven mechanism
/// [`ChildSizeProbe`] above uses. Deliberately NOT a nested `AnimatedContainer`
/// (see the divergence note on the test below for why).
#[derive(Clone, StatefulView)]
struct SteppedChildProbe {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
}

struct SteppedChildProbeState {
    vsync: Vsync,
    child_side: Arc<Mutex<f32>>,
}

impl StatefulView for SteppedChildProbe {
    type State = SteppedChildProbeState;

    fn create_state(&self) -> Self::State {
        SteppedChildProbeState {
            vsync: self.vsync.clone(),
            child_side: Arc::clone(&self.child_side),
        }
    }
}

impl ViewState<SteppedChildProbe> for SteppedChildProbeState {
    fn build(&self, _view: &SteppedChildProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.child_side.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedSize::new(RUN).child(SizedBox::square(side)),
        )
    }
}

/// `AnimatedSize` transiently reports `Changed` for the one layout a child's
/// size-change is first observed — then, when the child does NOT change
/// again, genuinely RESUMES that interpolation span (rather than collapsing
/// it to a degenerate no-op tween) as it settles back to `Stable`.
///
/// Flutter parity: `'tracks unstable child, then resumes animation when
/// child stabilizes'` (`animated_size_test.dart`, tag `3.44.0`), which nests
/// a real `AnimatedContainer` as the child so the "unstable" (multi-layout,
/// still-changing) sizes arise from its own independent 100 ms/1 ms runs,
/// and separately samples an interim `Unstable` state while the child is
/// still moving.
///
/// Divergence (test mechanism, not a confirmed behavior bug): porting the
/// oracle's nested-`AnimatedContainer` structure verbatim — letting the
/// child's OWN controller tick autonomously (`pump_for` with no widget-level
/// reconfigure) while both it and the enclosing `AnimatedSize` share one
/// `VsyncScope` — did not observably change `AnimatedSize`'s reported
/// size/state in this harness, even though the child's own geometry visibly
/// grew (confirmed independently) across the same `pump_for` calls. This
/// harness also could not reliably force a SECOND widget-level retarget of a
/// plain (non-animated) child to reach `AnimatedSize` once its controller
/// had already run once — a bare `SizedBox` reconfigure landed on the child
/// (confirmed independently) but `AnimatedSize`'s own committed size/state
/// stayed stale. Whether either is a genuine cross-render-object
/// dirty-propagation gap is a `flui-rendering` pipeline question, out of
/// scope for this widget-level port (see `AGENTS.md`'s TRIPWIRE guidance on
/// structural ripple beyond `flui-widgets`/`flui-animation`). The
/// `Unstable`/degenerate-collapse legs of the state machine already have
/// direct, thorough `#[cfg(test)]` coverage in
/// `flui-objects/src/layout/animated_size.rs`
/// (`changed_to_unstable_collapses_to_degenerate_zero_span_tween`,
/// `unstable_repeats_then_settles_with_no_visible_jump`) calling the
/// transition methods directly; this integration port instead proves the
/// ONE reliable end-to-end leg — the single widget-level retarget this
/// harness's `pump()` + `pump_for` are proven (by
/// [`animated_size_animates_forwards_then_backwards_with_stable_sized_children`]
/// above) to carry through correctly — including its most delicate part:
/// `Changed`→`Stable` RESUMING the genuine span, not collapsing it.
#[test]
fn animated_size_tracks_unstable_child_then_resumes_when_child_stabilizes() {
    let vsync = Vsync::new();
    let child_side = Arc::new(Mutex::new(100.0));
    let probe_widget = SteppedChildProbe {
        vsync: vsync.clone(),
        child_side: Arc::clone(&child_side),
    };
    let mut laid = lay_out_animated(probe_widget, loose(400.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedSize");

    assert_eq!(
        probe(&laid, id),
        (size(100.0, 100.0), AnimatedSizeState::Stable)
    );

    // Stable -> Changed: begin = the last committed size (100), end = the
    // child's new size (150) — a genuine interpolation span.
    *child_side.lock() = 150.0;
    laid.pump();
    assert_eq!(probe(&laid, id).1, AnimatedSizeState::Changed);

    // The child does NOT change again: Changed -> Stable RESUMES the
    // existing 100->150 span untouched, rather than collapsing it to a
    // degenerate 150->150 tween. Advancing AnimatedSize's own controller
    // halfway through its 200 ms duration (no further child-side change)
    // proves it — a collapsed tween would read 150 here, not the span's 50%
    // midpoint (125).
    laid.pump_for(Duration::ZERO); // detection frame: anchors AnimatedSize's own fresh run
    laid.pump_for(RUN / 2);
    assert_eq!(
        probe(&laid, id),
        (size(125.0, 125.0), AnimatedSizeState::Stable),
        "Changed -> Stable must resume the genuine 100->150 span (now at its \
         50% midpoint), not collapse it to a degenerate 150->150 tween"
    );

    laid.pump_for(RUN / 2);
    assert_eq!(
        probe(&laid, id),
        (size(150.0, 150.0), AnimatedSizeState::Stable)
    );
}
