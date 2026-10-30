//! Scroll-path tests:
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.
//! 5. `ScrollController::animate_to` (ADR-0037) — curve-driven animation,
//!    grab-to-cancel, and jump_to-cancels-in-flight.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out, tight};
use flui_animation::{SharedCurve, Vsync, curves};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::view::ScrollDirection;
use flui_view::{IntoView, SignalWriteExt, ViewExt};
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollController, Scrollable, Scrollbar,
    SemanticsRole, SharedScrollPhysics, SizedBox, VsyncScope,
};

#[derive(Clone, flui_view::prelude::StatelessView)]
pub(crate) struct FlingProfile {
    pub(crate) provider: flui_interaction::gesture::GestureSettingsProvider,
    pub(crate) child: flui_view::BoxedView,
}

impl flui_view::StatelessView for FlingProfile {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        flui_widgets::GestureArenaScope::new(
            flui_widgets::GestureArenaScope::of(ctx),
            self.child.clone(),
        )
        .settings(self.provider.clone())
    }
}

/// A mouse resting over a list a finger scrolls enters nothing while the
/// scroll owns the finger, though the rows move under it every frame; once
/// the finger lifts, the next frame's refresh catches up (ADR-0208 rule 6).
pub(crate) fn hover_holds_while_a_touch_scroll_is_latched() {
    use flui_testing::replay::TouchScript;
    use flui_widgets::{Column, MouseRegion};
    use std::cell::RefCell;

    let entered = Rc::new(RefCell::new(Vec::new()));
    let rows: Vec<_> = (0..10)
        .map(|row| {
            let entered = Rc::clone(&entered);
            MouseRegion::new()
                .on_enter(move |_, _, _| entered.borrow_mut().push(row))
                .child(SizedBox::new(300.0, 100.0))
                .boxed()
        })
        .collect();
    let mut laid = lay_out(
        Scrollable::new().child(Column::new(rows)),
        tight(300.0, 300.0),
    );
    laid.dispatch_pointer_hover(150.0, 50.0);
    laid.pump();
    assert_eq!(*entered.borrow(), [0], "the mouse rests over the first row");

    let sample = Duration::from_millis(16);
    let scroll = TouchScript::new("scroll under a resting mouse")
        .down(5, flui_foundation::geometry::Offset::new(150.0, 280.0))
        .wait(sample)
        .move_to(5, flui_foundation::geometry::Offset::new(150.0, 230.0))
        .wait(sample)
        .move_to(5, flui_foundation::geometry::Offset::new(150.0, 160.0))
        .wait(sample)
        .move_to(5, flui_foundation::geometry::Offset::new(150.0, 100.0))
        .wait(sample * 3)
        .up(5)
        .build()
        .expect("one whole contact");
    let mut latched = Vec::new();
    laid.replay_with(&scroll, |_, _| latched.clone_from(&entered.borrow()));
    assert_eq!(latched, [0], "no row entered while the scroll was latched");
    laid.pump();
    assert_ne!(
        *entered.borrow(),
        [0],
        "the refresh after the latch enters the row now under the mouse"
    );
}

pub(crate) fn cross_axis_sliver_tracks_reconcile_weights_and_publish_child_semantics() {
    use flui_widgets::{
        Semantics, SliverConstrainedCrossAxis, SliverCrossAxisGroup, SliverCrossAxisItem,
        SliverToBoxAdapter, Viewport,
    };
    let child = |label: &str, height| {
        SliverToBoxAdapter::new().child(
            Semantics::new()
                .container(true)
                .label(label)
                .child(SizedBox::new(200.0, height)),
        )
    };
    let content = |weight, limit| {
        Viewport::new((SliverCrossAxisGroup::new((
            SliverCrossAxisItem::content(
                SliverConstrainedCrossAxis::try_new(limit, child("fixed", 30.0))
                    .expect("valid limit"),
            ),
            child("one share", 60.0),
            SliverCrossAxisItem::flex(
                std::num::NonZeroU32::new(weight).expect("positive weight"),
                child("weighted", 90.0),
            ),
        )),))
    };
    let mut laid = lay_out(content(2, 40.0), tight(160.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let group = laid.find_by_render_type("RenderSliverCrossAxisGroup");
    let fixed = laid.child(group, 0);
    let equal = laid.child(group, 1);
    let weighted = laid.child(group, 2);
    for (weight, limit, expected) in [
        (2, 40.0, (40.0, 40.0, 80.0)),
        (3, 20.0, (20.0, 35.0, 105.0)),
        (1, 200.0, (160.0, 0.0, 0.0)),
        (2, 40.0, (40.0, 40.0, 80.0)),
    ] {
        laid.pump_widget(content(weight, limit));
        assert_eq!(
            laid.find_by_render_type("RenderSliverCrossAxisGroup"),
            group
        );
        assert_eq!(laid.child(group, 0), fixed);
        assert_eq!(laid.child(group, 1), equal);
        assert_eq!(laid.child(group, 2), weighted);
        assert_eq!(laid.offset(equal).dx, expected.0);
        assert_eq!(laid.offset(weighted).dx, expected.0 + expected.1);
        let tree = laid.a11y_tree().expect("semantics enabled");
        for (label, width) in [
            ("fixed", expected.0),
            ("one share", expected.1),
            ("weighted", expected.2),
        ] {
            let node = tree.find_by_label(label).expect("accessible child");
            assert_eq!(
                node.bounds().expect("published geometry").width(),
                width,
                "{label}"
            );
        }
        assert_eq!(laid.sliver_geometry(group).scroll_extent, 90.0);
    }
}

pub(crate) fn constrained_sliver_updates_layout_without_replacing_the_child() {
    use flui_widgets::{SliverConstrainedCrossAxis, SliverToBoxAdapter, Viewport};
    let content = |extent| {
        Viewport::new((SliverConstrainedCrossAxis::try_new(
            extent,
            SliverToBoxAdapter::new().child(SizedBox::new(200.0, 30.0)),
        )
        .expect("valid extent"),))
    };
    let mut laid = lay_out(content(40.0), tight(100.0, 100.0));
    let limit = laid.find_by_render_type("RenderSliverConstrainedCrossAxis");
    let child = laid.child(laid.child(limit, 0), 0);
    for extent in [40.0, 15.5, 0.0, 200.0] {
        laid.pump_widget(content(extent));
        assert_eq!(
            laid.find_by_render_type("RenderSliverConstrainedCrossAxis"),
            limit
        );
        assert_eq!(laid.child(laid.child(limit, 0), 0), child);
        assert_eq!(laid.size(child).width, extent.min(100.0));
        assert_eq!(laid.sliver_geometry(limit).scroll_extent, 30.0);
    }
}

pub(crate) fn scrollable_accessibility_ranges_follow_the_actual_axis() {
    use flui_foundation::geometry::Axis;

    for axis in [Axis::Vertical, Axis::Horizontal] {
        let controller = ScrollController::new();
        let mut laid = lay_out(
            Scrollable::new()
                .scroll_direction(axis)
                .controller(controller.clone())
                .child(SizedBox::new(1000.0, 1000.0)),
            tight(200.0, 200.0),
        );
        laid.enable_semantics();
        laid.tick();
        assert_eq!(controller.max_scroll_extent(), 800.0, "measured viewport");
        controller.jump_to(120.0).expect("no build is running");
        laid.tick();
        let tree = laid
            .a11y_tree()
            .expect("mounted scrollable publishes actual AccessKit tree");
        let sources: Vec<_> = tree
            .raw()
            .nodes
            .iter()
            .filter(|(_, node)| node.scroll_x().is_some() || node.scroll_y().is_some())
            .collect();
        assert!(
            !sources.is_empty(),
            "native accessibility receives scroll range"
        );
        for (_, node) in sources {
            let (position, min, max, other_position, other_min, other_max) = match axis {
                Axis::Vertical => (
                    node.scroll_y(),
                    node.scroll_y_min(),
                    node.scroll_y_max(),
                    node.scroll_x(),
                    node.scroll_x_min(),
                    node.scroll_x_max(),
                ),
                Axis::Horizontal => (
                    node.scroll_x(),
                    node.scroll_x_min(),
                    node.scroll_x_max(),
                    node.scroll_y(),
                    node.scroll_y_min(),
                    node.scroll_y_max(),
                ),
            };
            assert_eq!(
                position,
                Some(120.0),
                "{axis:?}: native offset follows actual axis"
            );
            assert_eq!(min, Some(0.0), "{axis:?}: native minimum");
            assert_eq!(max, Some(800.0), "{axis:?}: native maximum");
            assert_eq!(
                (other_position, other_min, other_max),
                (None, None, None),
                "{axis:?}: orthogonal axis has no invented scroll range"
            );
        }
        controller.jump_to(240.0).expect("no build is running");
        laid.tick();
        let tree = laid.a11y_tree().expect("changed native offset republished");
        assert!(
            tree.raw().nodes.iter().any(|(_, node)| match axis {
                Axis::Vertical => node.scroll_y() == Some(240.0),
                Axis::Horizontal => node.scroll_x() == Some(240.0),
            }),
            "{axis:?}: later scrolling remains observable on the same native axis"
        );
    }
}

pub(crate) fn terminal_scroll_motion_uses_the_admitted_fling_profile() {
    let profile = |min, max| {
        flui_interaction::GestureSettings::default()
            .try_with_fling_velocity(min, max)
            .expect("valid fling range")
    };
    let source = flui_interaction::gesture::GestureSettingsSource::new(profile(50.0, 300.0));
    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");
    let child = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0))
        .boxed();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            FlingProfile {
                provider: source.provider(),
                child,
            },
        ),
        tight(300.0, 300.0),
        vsync,
    );
    controller.set_pixels(500.0);
    for attempt in 0..4 {
        laid.dispatch_pointer_down(150.0, 250.0);
        let step = if attempt == 3 { 200.0 } else { 20.0 };
        let end = 250.0 - 5.0 * step;
        for sample in 1..=5 {
            let y = 250.0 - f64::from(sample) * step;
            laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
        }
        if attempt == 0 {
            source.replace(profile(5000.0, 5000.0));
        }
        laid.dispatch_pointer_up(150.0, end);
        let released = controller.pixels();
        // Anchor the controller's first tick, then observe actual pixels.
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        let coast = controller.pixels() - released;
        if attempt == 1 {
            assert_eq!(
                coast, 0.0,
                "next contact below its admitted minimum does not coast"
            );
            source.replace(profile(50.0, 600.0));
        } else if attempt == 3 {
            assert!(
                coast > 200.0 && coast < 250.0,
                "admitted max15000 determines the coasting distance, coast {coast}"
            );
        } else {
            let bound = if attempt == 0 { 10.0 } else { 20.0 };
            assert!(
                coast > 0.0 && coast < bound,
                "attempt {attempt} uses its captured maximum, coast {coast}"
            );
            if attempt == 2 {
                source.replace(profile(50.0, 15000.0));
            }
        }
    }
}

// ============================================================================
// Viewport — Position/Fixed mode switching
// ============================================================================

// ============================================================================
// ListView / GridView — `.position()` passthrough
// ============================================================================

// ============================================================================
// ScrollController — thumb geometry helpers
// ============================================================================

// ============================================================================
// ScrollPhysics — clamping boundary enforcement
// ============================================================================

// ============================================================================
// Scrollable — drag gesture integration
// ============================================================================

/// A drag upward (finger moves toward smaller y-values, delta.dy < 0) must
/// increase the scroll offset because upward drag reveals content below the
/// current viewport position. This test FAILS if the pan callback is not
/// wired: `controller.pixels()` stays 0.0 when no gesture fires.
pub(crate) fn scrollable_drag_up_increases_scroll_offset() {
    let controller = ScrollController::new();
    // 300px viewport, 800px content → 500px scroll extent.
    controller
        .update_dimensions(300.0, 0.0, 500.0)
        .expect("ordered extents");

    let physics: SharedScrollPhysics = Rc::new(ClampingScrollPhysics::default());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Starting position: top of content.
    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // With no competing recognizer, the arena awards the drag after Down.
    // The first 50px upward move is therefore delivered in full.
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);

    assert_eq!(
        controller.pixels(),
        50.0,
        "an upward 50px finger move must increase the scroll offset by exactly 50px"
    );
}

pub(crate) fn a_remaining_touch_continues_scroll_without_an_intermediate_fling() {
    use flui_platform::{
        EventTime,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerInfo, PointerKind, PointerMove,
            PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };
    use flui_testing::PointerPhase;

    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    let event = |id: u64, millis: u64, y: f64, phase| {
        let info = PointerInfo::new(
            flui_interaction::PointerId::try_from(id).expect("nonzero touch identity"),
            PointerKind::Touch,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, y))
                .expect("finite touch position"),
        );
        match phase {
            PointerPhase::Down => PointerEvent::Down(PointerPress::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE.with(PointerButton::PRIMARY),
                sample,
            )),
            PointerPhase::Move => PointerEvent::Move(PointerMove::new(
                info,
                PointerButtons::NONE.with(PointerButton::PRIMARY),
                sample,
            )),
            PointerPhase::Up => PointerEvent::Up(PointerRelease::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            PointerPhase::Cancel | PointerPhase::Hover => {
                unreachable!("this row scripts touch contact and release")
            }
        }
    };
    for (id, millis, y, phase) in [
        (2, 0, 250.0, PointerPhase::Down),
        (2, 10, 200.0, PointerPhase::Move),
        (2, 20, 150.0, PointerPhase::Move),
        (3, 25, 60.0, PointerPhase::Down),
        (3, 30, 70.0, PointerPhase::Move),
        (3, 40, 80.0, PointerPhase::Move),
    ] {
        scoped.dispatch_pointer_event(&event(id, millis, y, phase));
    }
    assert_eq!(
        controller.pixels(),
        100.0,
        "passive touch motion does not move the active drag"
    );
    scoped.dispatch_pointer_event(&event(2, 45, 150.0, PointerPhase::Up));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.pixels(),
        100.0,
        "first touch release must not start a ballistic run while another touch remains"
    );

    scoped.dispatch_pointer_event(&event(3, 55, 90.0, PointerPhase::Move));
    assert_eq!(
        controller.pixels(),
        90.0,
        "handoff rebases to the successor: only its next 10px delta scrolls"
    );
    scoped.dispatch_pointer_event(&event(3, 65, 100.0, PointerPhase::Move));
    assert_eq!(controller.pixels(), 80.0);
    scoped.dispatch_pointer_event(&event(3, 70, 100.0, PointerPhase::Up));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() < 80.0,
        "final fling uses the successor's downward measured history, not the first touch's upward velocity: {}",
        controller.pixels()
    );

    controller.jump_to(200.0).expect("no build is running");
    scoped.dispatch_pointer_event(&event(2, 200, 200.0, PointerPhase::Down));
    // Past the slop: the drag starts from Down and applies all 30px.
    scoped.dispatch_pointer_event(&event(2, 210, 170.0, PointerPhase::Move));
    assert_eq!(
        controller.pixels(),
        230.0,
        "reused touch identity starts a fresh drag after completion"
    );
    scoped.dispatch_pointer_event(&event(2, 220, 170.0, PointerPhase::Up));
}

// ============================================================================
// Scrollable — fling ballistic simulation integration
// ============================================================================

/// Wrap `widget` in a [`VsyncScope`] so its `ScrollableState::init_state` can
/// register the fling controller, then lay it out under `constraints` with a
/// gesture arena. Adopts the same vsync in the tree binding so
/// [`LaidOut::pump_for`] ticks the fling animation deterministically.
fn fling_scoped(widget: Scrollable, vsync: Vsync, constraints: BoxConstraints) -> LaidOut {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, constraints);
    scoped.adopt_vsync(vsync).expect("clock admission");
    scoped
}

/// After a pan gesture ends with sufficient velocity the scroll offset must
/// continue to advance beyond the release position when the binding pumps
/// animation frames — confirming that the fling animation controller is wired
/// to the scroll controller and the vsync is driving it.
pub(crate) fn scrollable_fling_advances_offset_past_release() {
    exercise_fling_registry(false);
}

pub(crate) fn an_active_fling_migrates_between_registries() {
    exercise_fling_registry(true);
}

fn exercise_fling_registry(migrate: bool) {
    let controller = ScrollController::new();
    // Large extent prevents the fling from hitting the boundary on the first
    // frame — we want to observe forward motion, not clamping.
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget.clone(), vsync.clone(), tight(300.0, 300.0));

    // Upward drag well past the 18 px slop to establish a recognizable fling
    // velocity. The first move crosses slop (drag on_start). The second fires
    // drag on_update, advancing the offset. The pointer_up triggers drag on_end
    // which calls animate_with on the fling controller.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 150.0); // 30 px more: drag on_update
    scoped.dispatch_pointer_up(150.0, 150.0);

    let pixels_at_release = controller.pixels();
    assert!(
        pixels_at_release > 0.0,
        "pan drag must advance the offset before release; got {pixels_at_release}"
    );

    // First pump: vsync detects the new run generation from `animate_with` and
    // anchors the run start at t=0. The controller ticks at elapsed=0, which
    // gives x(0) = start (the release position). No net advance yet.
    scoped.pump_for(Duration::from_millis(16));
    // Second pump: advances to t=16 ms. The ballistic simulation gives
    // x(0.016) > start (friction deceleration carries the position forward).
    scoped.pump_for(Duration::from_millis(16));

    assert!(
        controller.pixels() > pixels_at_release,
        "scroll offset must continue past the release position after two fling frames; \
         release={pixels_at_release:.1}, now={:.1}",
        controller.pixels()
    );
    if migrate {
        let before = controller.pixels();
        assert!(vsync.has_running());
        let next = Vsync::new();
        scoped.pump_widget(VsyncScope::new(next.clone(), widget));
        assert!(
            vsync.is_empty(),
            "fling owner withdraws its old registry seat"
        );
        scoped
            .adopt_vsync(next.clone())
            .expect("replacement clock admission");
        scoped.pump_for(Duration::ZERO);
        assert!((controller.pixels() - before).abs() < 1e-8);
        scoped.pump_for(Duration::from_millis(16));
        assert!(
            controller.pixels() > before,
            "the transferred fling continues scrolling"
        );
        assert!(next.has_running());
        scoped.pump_widget(SizedBox::shrink());
        assert!(next.is_empty());
        let retired = controller.pixels();
        scoped.pump_for(Duration::from_secs(1));
        assert_eq!(
            controller.pixels(),
            retired,
            "removed fling stops publishing scroll offsets"
        );
    }
}

/// Bouncing physics allows the drag to carry the scroll position past
/// `max_scroll_extent` with spring damping. On release, a
/// spring springs the position back to the boundary. After
/// enough frames the position must be within 1 px of `max_scroll_extent`.
pub(crate) fn bouncing_physics_fling_springs_back_after_overscroll() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f64;
    controller
        .update_dimensions(300.0, 0.0, max_extent)
        .expect("ordered extents");

    let physics: SharedScrollPhysics = Rc::new(BouncingScrollPhysics::new());
    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Pre-position the scroll just below max_extent so a moderate in-bounds
    // upward drag pushes it past the boundary under bouncing physics.
    controller.set_pixels(480.0);

    // Upward drag past slop, then a further in-bounds move that applies
    // `apply_boundary_conditions` and lets pixels exceed max_extent (damped
    // by the overscroll spring coefficient 0.52):
    //   proposed = 480 − (−60) = 540 → clamped = 500 + 40×0.52 = 520.8
    // drag on_end sees pixels = 520.8 > max_extent and returns a
    // spring simulation that springs the position back to max_extent.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 120.0); // 60 px more upward: drag on_update
    scoped.dispatch_pointer_up(150.0, 120.0);

    // Pump 100 frames (1.6 s) — sufficient for the critically-damped spring
    // (SpringDescription with damping_ratio ≥ 0.75) to settle.
    for _ in 0..100 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let final_pixels = controller.pixels();
    assert!(
        final_pixels <= max_extent + 1.0,
        "bouncing spring-back must return scroll to within 1 px of max_extent ({max_extent}); \
         got {final_pixels:.3}"
    );
}

// ============================================================================
// Scrollable — animate_to (ADR-0037)
// ============================================================================

pub(crate) fn scrollable_retarget_preserves_the_position_velocity() {
    assert_scroll_retarget_preserves_velocity(|_| 100.0);
}

pub(crate) fn scrollable_retarget_at_the_current_position_brakes_continuously() {
    assert_scroll_retarget_preserves_velocity(std::convert::identity);
}

fn assert_scroll_retarget_preserves_velocity(target: fn(f64) -> f64) {
    let controller = ScrollController::new();
    let registry = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut laid = fling_scoped(widget, registry, tight(300.0, 300.0));
    controller
        .animate_to(
            1000.0,
            Duration::from_secs(1),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    laid.pump_for(Duration::from_millis(1));
    laid.pump_for(Duration::from_millis(250));
    let h = Duration::from_micros(100);
    let previous = controller.pixels();
    laid.pump_for(h);
    let seam = controller.pixels();
    let arriving = (seam - previous) / h.as_secs_f64();
    assert!(arriving > 100.0, "the mounted position was moving");
    let target = target(seam);
    controller
        .animate_to(
            target,
            Duration::from_secs(1),
            SharedCurve::new(curves::EASE_IN),
        )
        .expect("no build is running");
    assert!((controller.pixels() - seam).abs() < 1e-12);
    assert!(controller.position().is_scrolling());
    laid.pump_for(h);
    let departing = (controller.pixels() - seam) / h.as_secs_f64();
    assert!(
        (departing - arriving).abs() < 1.0,
        "scroll retarget must inherit velocity: before {arriving}, after {departing}"
    );
    laid.pump_for(Duration::from_secs(2));
    assert_eq!(controller.pixels(), target);
    assert!(!controller.position().is_scrolling());
}

/// Jumping cancels before another frame can publish the displaced run's value.
pub(crate) fn scrollable_jump_to_during_animate_to_cancels_it_synchronously() {
    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller
        .animate_to(
            1000.0,
            Duration::from_millis(300),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    // Three pumps of warm-up, one more than a direct `animate_with` fling:
    // `animate_to` queues a command, and pump 1's rebuild services it after
    // pump 1's own controller tick; pump 2 anchors the new run at `t = 0`;
    // pump 3 is the first tick that advances the value.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() > 0.0 && controller.pixels() < 1000.0,
        "sanity: the animation must be in flight before jump_to"
    );

    controller.jump_to(42.0).expect("no build is running");
    assert_eq!(
        controller.pixels(),
        42.0,
        "jump_to must move to its own target immediately"
    );

    // The very next frame tick must NOT resume the old animation — if the
    // cancellation were only queued (not synchronous), this frame's tick step
    // (which runs BEFORE the rebuild that would service the queued Cancel)
    // would advance the still-running fling controller once more, stomping
    // the 42.0 this assertion checks.
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.pixels(),
        42.0,
        "the frame immediately after jump_to must not have resumed the \
         canceled animate_to even transiently; got {:.2}",
        controller.pixels()
    );

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.pixels(),
        42.0,
        "a jump_to mid-animate_to must cancel the run for good — later frames \
         must not resume driving toward the original 1000.0 target; got {:.2}",
        controller.pixels()
    );
}

// ============================================================================
// Scrollbar — thumb drag
// ============================================================================

pub(crate) fn replacing_pending_scroll_motion_delivers_and_recovers_after_user_failure() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RetiringCurve {
        drops: Arc<AtomicUsize>,
        fails: bool,
    }

    impl flui_animation::Curve for RetiringCurve {
        fn transform(&self, progress: f64) -> f64 {
            curves::LINEAR.transform(progress)
        }
    }

    impl Drop for RetiringCurve {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            assert!(!self.fails, "pending scroll curve retirement failure");
        }
    }

    struct RetiringObserver(Arc<AtomicUsize>);
    impl RetiringObserver {
        fn observe(&self) {
            assert_eq!(self.0.load(Ordering::SeqCst), 0);
        }
    }
    impl Drop for RetiringObserver {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    for jumps in [false, true] {
        for (retirement_fails, notification_fails) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let controller = ScrollController::new();
            controller
                .update_dimensions(300.0, 0.0, 4700.0)
                .expect("ordered extents");
            let drops = Arc::new(AtomicUsize::new(0));
            controller
                .animate_to(
                    100.0,
                    Duration::from_secs(1),
                    SharedCurve::new(RetiringCurve {
                        drops: Arc::clone(&drops),
                        fails: retirement_fails,
                    }),
                )
                .expect("no build is running");
            let attempts = Rc::new(Cell::new(0));
            let calls = Rc::clone(&attempts);
            let listenable = controller.listenable();
            let listener = listenable.add_listener(Rc::new(move || {
                calls.set(calls.get() + 1);
                assert!(!notification_fails, "pending scroll notification failure");
            }));
            let observer_drops = Arc::new(AtomicUsize::new(0));
            let observer = RetiringObserver(Arc::clone(&observer_drops));
            let identity = Rc::new(Cell::new(None));
            let owned_identity = Rc::clone(&identity);
            let removal = Rc::clone(&listenable);
            let self_removing = listenable.add_listener(Rc::new(move || {
                observer.observe();
                removal.remove_listener(owned_identity.take().expect("registered observer"));
            }));
            identity.set(Some(self_removing));
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if jumps {
                    controller.jump_to(200.0).expect("no build is running");
                } else {
                    controller
                        .animate_to(
                            200.0,
                            Duration::from_secs(1),
                            SharedCurve::new(curves::LINEAR),
                        )
                        .expect("no build is running");
                }
            }));
            assert_eq!(
                drops.load(Ordering::SeqCst),
                usize::from(!(jumps && notification_fails)),
                "jump notification failure retains opaque outgoing ownership"
            );
            assert_eq!(attempts.get(), 1, "accepted replacement is delivered");
            assert_eq!(
                observer_drops.load(Ordering::SeqCst),
                usize::from(!notification_fails && (jumps || !retirement_fails)),
                "observer retirement inherits the enclosing failure custody"
            );
            if retirement_fails || notification_fails {
                assert_scroll_failure(
                    &outcome.expect_err("authored failure propagates after delivery"),
                    if retirement_fails && !(jumps && notification_fails) {
                        "pending scroll curve retirement failure"
                    } else {
                        "pending scroll notification failure"
                    },
                );
            } else {
                assert!(outcome.is_ok(), "healthy replacement completes");
            }
            listenable.remove_listener(listener);
            let mut laid = fling_scoped(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(300.0, 5000.0)),
                Vsync::new(),
                tight(300.0, 300.0),
            );
            laid.pump_for(Duration::ZERO);
            laid.pump_for(Duration::from_secs(2));
            assert_eq!(
                controller.pixels(),
                200.0,
                "mounted consumer keeps accepted command"
            );
            controller.jump_to(75.0).expect("no build is running");
            controller
                .animate_to(
                    280.0,
                    Duration::from_secs(1),
                    SharedCurve::new(curves::LINEAR),
                )
                .expect("no build is running");
            laid.pump_for(Duration::ZERO);
            laid.pump_for(Duration::from_millis(400));
            assert!(
                controller.pixels() > 75.0 && controller.pixels() < 280.0,
                "a fresh mounted run advances after containment"
            );
            laid.pump_for(Duration::from_secs(2));
            assert_eq!(controller.pixels(), 280.0, "fresh motion completes");
        }
    }
}

pub(crate) fn stopping_scroll_motion_preserves_commands_admitted_by_activity_observers() {
    for newer_jumps in [false, true] {
        let controller = ScrollController::new();
        let mut laid = fling_scoped(
            Scrollable::new()
                .controller(controller.clone())
                .child(SizedBox::new(300.0, 5000.0)),
            Vsync::new(),
            tight(300.0, 300.0),
        );
        controller
            .animate_to(
                1000.0,
                Duration::from_secs(1),
                SharedCurve::new(curves::LINEAR),
            )
            .expect("no build is running");
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(200));
        assert!(
            controller.pixels() > 0.0 && controller.position().is_scrolling(),
            "mounted driver is active before cancellation"
        );
        let armed = Rc::new(Cell::new(true));
        let once = Rc::clone(&armed);
        let reentry = controller.clone();
        let listener = controller
            .position()
            .add_activity_listener(Rc::new(move || {
                if !reentry.position().is_scrolling() && once.replace(false) {
                    if newer_jumps {
                        reentry.jump_to(325.0).expect("no build is running");
                    } else {
                        reentry
                            .animate_to(
                                275.0,
                                Duration::from_secs(1),
                                SharedCurve::new(curves::LINEAR),
                            )
                            .expect("no build is running");
                    }
                }
            }));
        controller.jump_to(200.0).expect("no build is running");
        assert!(
            !armed.get(),
            "actual cancellation delivered the activity callback"
        );
        if newer_jumps {
            assert_eq!(
                controller.pixels(),
                325.0,
                "newer jump survives outer cancellation"
            );
        }
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_secs(2));
        assert_eq!(
            controller.pixels(),
            if newer_jumps { 325.0 } else { 275.0 },
            "newer mounted command survives the older cancellation tail"
        );
        controller.position().remove_activity_listener(listener);
    }
}

pub(crate) fn retiring_scroll_curves_preserves_newer_reentrant_commands() {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static REENTRY: RefCell<Option<(ScrollController, bool)>> = const { RefCell::new(None) };
    }
    struct ReentrantCurve(Arc<AtomicUsize>);
    impl flui_animation::Curve for ReentrantCurve {
        fn transform(&self, progress: f64) -> f64 {
            curves::LINEAR.transform(progress)
        }
    }
    impl Drop for ReentrantCurve {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            let (controller, jumps) = REENTRY
                .with(|slot| slot.borrow_mut().take())
                .expect("armed owner-local curve retirement");
            if jumps {
                controller.jump_to(325.0).expect("no build is running");
            } else {
                controller
                    .animate_to(
                        275.0,
                        Duration::from_secs(1),
                        SharedCurve::new(curves::LINEAR),
                    )
                    .expect("no build is running");
            }
        }
    }
    for mounted in [false, true] {
        for outer_jumps in [false, true] {
            for newer_jumps in [false, true] {
                let controller = ScrollController::new();
                controller
                    .update_dimensions(300.0, 0.0, 4700.0)
                    .expect("ordered extents");
                let drops = Arc::new(AtomicUsize::new(0));
                let content = || {
                    Scrollable::new()
                        .controller(controller.clone())
                        .child(SizedBox::new(300.0, 5000.0))
                };
                let mut mounted_scene =
                    mounted.then(|| fling_scoped(content(), Vsync::new(), tight(300.0, 300.0)));
                controller
                    .animate_to(
                        100.0,
                        Duration::from_secs(1),
                        SharedCurve::new(ReentrantCurve(Arc::clone(&drops))),
                    )
                    .expect("no build is running");
                if let Some(laid) = &mut mounted_scene {
                    laid.pump_for(Duration::ZERO);
                    laid.pump_for(Duration::from_millis(200));
                    assert!(
                        controller.position().is_scrolling(),
                        "custom curve drives mounted motion"
                    );
                }
                REENTRY.with(|slot| *slot.borrow_mut() = Some((controller.clone(), newer_jumps)));
                if outer_jumps {
                    controller.jump_to(200.0).expect("no build is running");
                } else {
                    controller
                        .animate_to(
                            200.0,
                            Duration::from_secs(1),
                            SharedCurve::new(curves::LINEAR),
                        )
                        .expect("no build is running");
                }
                assert_eq!(
                    drops.load(Ordering::SeqCst),
                    1,
                    "outgoing destruction actually reenters"
                );
                assert!(REENTRY.with(|slot| slot.borrow().is_none()));
                let mut laid = mounted_scene
                    .unwrap_or_else(|| fling_scoped(content(), Vsync::new(), tight(300.0, 300.0)));
                laid.pump_for(Duration::ZERO);
                laid.pump_for(Duration::from_secs(2));
                let expected = if newer_jumps { 325.0 } else { 275.0 };
                assert_eq!(
                    controller.pixels(),
                    expected,
                    "mounted consumer retains the newest command"
                );
                laid.pump_for(Duration::from_secs(2));
                assert_eq!(controller.pixels(), expected, "older command cannot resume");
            }
        }
    }
}

pub(crate) fn scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded() {
    fn short_track() {
        assert_scrollbar_track(10.0);
    }
    fn minimum_track() {
        assert_scrollbar_track(18.0);
    }
    fn ordinary_track() {
        assert_scrollbar_track(100.0);
    }
    crate::common::cases::run_cases(
        "scrollbar track containment",
        &[
            ("short track", short_track as fn()),
            ("minimum track", minimum_track as fn()),
            ("ordinary track", ordinary_track as fn()),
        ],
    );
}

fn assert_scrollbar_track(height: f64) {
    let controller = ScrollController::new();
    controller
        .update_dimensions(height, 0.0, 900.0)
        .expect("ordered extents");
    let mut laid = lay_out(
        Scrollbar::new()
            .controller(controller.clone())
            .child(SizedBox::new(100.0, height)),
        tight(100.0, height),
    );
    // Scrollbar's ColoredBox paints through RenderDecoratedBox; the SizedBox
    // content has no decoration, so this identifies exactly the real thumb.
    let thumbs = laid.find_all_by_render_type("RenderDecoratedBox");
    assert_eq!(thumbs.len(), 1);
    let thumb = thumbs[0];
    assert!(
        laid.size(thumb).height <= height,
        "thumb exceeds {height}px track"
    );
    controller.jump_to(900.0).expect("no build is running");
    laid.tick();
    let bottom = laid.absolute_offset(thumb).dy + laid.size(thumb).height;
    assert!(
        (bottom - height).abs() < 1e-8,
        "thumb must end at track boundary"
    );

    controller.jump_to(0.0).expect("no build is running");
    laid.tick();
    laid.dispatch_pointer_down(97.0, 5.0);
    laid.dispatch_pointer_move(97.0, 25.0);
    laid.dispatch_pointer_move(97.0, 205.0);
    laid.dispatch_pointer_up(97.0, 205.0);
    if height <= 18.0 {
        assert_eq!(controller.pixels(), 0.0, "a full-track thumb has no travel");
    } else {
        assert_eq!(
            controller.pixels(),
            900.0,
            "thumb drag clamps to content end"
        );
    }
    controller.jump_to(0.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        controller.pixels(),
        0.0,
        "next jump still works after dragging"
    );
}

pub(crate) fn dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Scrollbar::new().controller(scroll.clone()).child(
                Scrollable::new()
                    .controller(scroll.clone())
                    .child(SizedBox::new(300.0, 5000.0)),
            ),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    scroll
        .animate_to(
            1000.0,
            Duration::from_millis(300),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    advance_scroll_run(&mut laid);
    assert!(scroll.pixels() > 0.0 && scroll.pixels() < 1000.0);
    assert!(scroll.position().is_scrolling());
    let thumbs = laid.find_all_by_render_type("RenderDecoratedBox");
    assert_eq!(thumbs.len(), 1);
    let top = laid.absolute_offset(thumbs[0]).dy;
    let before = scroll.pixels();
    laid.dispatch_pointer_down(297.0, top + 5.0);
    laid.dispatch_pointer_move(297.0, top + 25.0);
    laid.dispatch_pointer_move(297.0, top + 45.0);
    let dragged = scroll.pixels();
    assert!(
        dragged > before,
        "pointer drag must advance the actual position"
    );
    laid.dispatch_pointer_up(297.0, top + 45.0);
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        scroll.pixels(),
        dragged,
        "old animation must not overwrite the drag"
    );
    for _ in 0..24 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        scroll.pixels(),
        dragged,
        "retired animation must stay stopped"
    );
    assert!(!scroll.position().is_scrolling());
    scroll
        .animate_to(
            4000.0,
            Duration::from_millis(300),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    advance_scroll_run(&mut laid);
    assert!(
        scroll.pixels() > dragged,
        "a later animation still makes progress"
    );
}

// ============================================================================
// Hit-test transform-stack composition through the sliver hit-test walk
// ============================================================================
//
// `PipelineOwner::hit_test_sliver_subtree` computes each child's position and
// recurses into it, but (before the fix this section pins down) never pushed
// the paint offset it consumed onto the `HitTestResult` transform stack --
// unlike the box-side walk, which does for `RenderBox` ancestors
// (`crates/flui-rendering/src/pipeline/owner/accessors.rs`). A `Listener`
// living anywhere below a sliver therefore received the raw, un-localized
// GLOBAL dispatch position instead of one local to its own box -- wrong for
// any sliver whose child paints at a nonzero offset, the common case for a
// scrolled list.
//
// The invariant is that a tap's `localPosition` stays scroll-correct through
// nested slivers. FLUI has no `SliverMainAxisGroup` yet, so these regression
// tests reproduce the class of defect through the sliver widgets FLUI
// does have: `ListView` (the sliver->box leg, under a real scroll offset,
// vertical and horizontal), `SliverPadding` wrapping a `SliverToBoxAdapter`
// (the sliver->sliver leg), and a reversed `AxisDirection` (the sliver->box
// leg's sign, not just its axis).

// ============================================================================
// Scrollable — scroll-activity signal
// ============================================================================

/// The scroll-activity signal across a full gesture: idle before, live
/// from the grab with the user's direction recorded, live through the
/// ballistic run past the release, and idle again — direction reset — once
/// the run settles. The signal a floating header's snap trigger keys on.
pub(crate) fn scroll_activity_tracks_the_whole_gesture_lifecycle() {
    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller)
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    assert!(!position.is_scrolling(), "idle before any gesture");
    assert_eq!(position.user_scroll_direction(), ScrollDirection::Idle);

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: drag on_start
    assert!(
        position.is_scrolling(),
        "the grab begins the scroll activity"
    );
    scoped.dispatch_pointer_move(150.0, 150.0); // drag on_update
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Reverse,
        "an upward finger drag increases the offset — Reverse"
    );
    scoped.dispatch_pointer_up(150.0, 150.0);

    // Whether the release produced a ballistic run (fast release) or not,
    // the signal must return to idle once everything settles; the bounded
    // pump keeps a stuck-true regression a loud failure, not a hang.
    let mut frames = 0;
    while position.is_scrolling() && frames < 2_000 {
        scoped.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert!(
        !position.is_scrolling(),
        "the activity must end after the release settles (still scrolling \
         after {frames} frames)"
    );
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Idle,
        "ending the scroll resets the direction"
    );
}

// ============================================================================
// Scrollable — arbitrated pointer-signal (wheel) routing
// ============================================================================

/// Two vertically nested scrollables under one wheel tick: the outer's
/// 300×300 viewport holds a 300×200 inner scrollable at the top of its
/// content, and the tick lands over the inner.
///
/// Layout used by the three arbitration tests below:
/// outer content = Column[SizedBox(300×200){inner Scrollable}, 300×4800
/// filler], inner content = 300×1000, so the inner can travel 0..=800 and
/// the outer 0..=4700.
fn nested_scrollables(outer: &ScrollController, inner: &ScrollController, vsync: Vsync) -> LaidOut {
    outer
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");
    inner
        .update_dimensions(200.0, 0.0, 800.0)
        .expect("ordered extents");

    let inner_scrollable = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(300.0, 1000.0));
    let outer_scrollable =
        Scrollable::new()
            .controller(outer.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 200.0)
                    .child(inner_scrollable)
                    .into_view()
                    .boxed(),
                SizedBox::new(300.0, 4800.0).into_view().boxed(),
            ]));

    let wrapped = VsyncScope::new(vsync.clone(), outer_scrollable);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync).expect("clock admission");
    scoped
}

/// One wheel tick over nested scrollables moves ONLY the innermost one that
/// can move — the pointer-signal arbitration contract: every scrollable
/// on the hit path registers interest, the first (leaf-most) registrant wins,
/// and the rest never act. Without arbitration
/// the same tick advances BOTH controllers (issue #717's double-scroll).
pub(crate) fn a_wheel_tick_over_nested_scrollables_moves_only_the_inner() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());

    // Over the inner scrollable (its region is 0..200 in root coordinates).
    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_eq!(
        inner.pixels(),
        53.0,
        "the inner scrollable claims the tick and scrolls"
    );
    assert_eq!(
        outer.pixels(),
        0.0,
        "the outer scrollable must NOT also scroll — the inner claimed the tick"
    );
}

fn phased_scroll(
    y: f64,
    dy: f64,
    phase: Option<flui_interaction::events::pointer::ScrollPhase>,
    device: u64,
) -> flui_interaction::PointerEvent {
    use flui_interaction::events::pointer::*;
    let pointer = PointerInfo::new(
        PointerId::try_from(1_u64).expect("pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(device).expect("actual source"));
    let mut event = ScrollEvent::new(
        pointer,
        flui_platform::EventTime::from_nanos(0),
        PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, y))
            .expect("finite focal point"),
        ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("finite scroll"),
    );
    event.phase = phase;
    PointerEvent::Scroll(event)
}

/// The same native gesture cannot jump into an ancestor at the child's edge.
pub(crate) fn nested_scroll_sequence_keeps_its_first_consumptive_target() {
    use flui_interaction::events::pointer::ScrollPhase;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    inner
        .jump_to(inner.max_scroll_extent() - 20.0)
        .expect("no build is running");
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 20.0, Some(ScrollPhase::Began), 1));
    assert_eq!(inner.pixels(), inner.max_scroll_extent());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        outer.pixels(),
        0.0,
        "reaching the edge does not chain the same gesture into the ancestor"
    );
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 0.0, Some(ScrollPhase::Ended), 1));
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Began), 1));
    assert_eq!(
        outer.pixels(),
        30.0,
        "the next gesture selects the first target that can actually move"
    );
    inner.jump_to(0.0).expect("no build is running");
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        inner.pixels(),
        0.0,
        "a newly eligible descendant cannot steal an already claimed stream"
    );
    assert_eq!(outer.pixels(), 60.0);
}

pub(crate) fn scroll_latch_survives_focal_motion_and_releases_on_cancel() {
    use flui_interaction::events::pointer::ScrollPhase;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, Some(ScrollPhase::Began), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        inner.pixels(),
        83.0,
        "the exact first claimant receives later focal motion outside its viewport"
    );
    assert_eq!(outer.pixels(), 0.0);
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 0.0, Some(ScrollPhase::Cancelled), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Began), 1));
    assert_eq!(
        outer.pixels(),
        30.0,
        "cancellation releases the next fresh claim"
    );
}

/// A wheel burst ends after the preset's contactless idle time on the owner
/// clock; a phased source that began and never ended (Wayland) after the
/// longer phased time (ADR-0209 §5).
pub(crate) fn phase_less_scroll_latch_expires_on_owner_clock_inactivity() {
    use flui_interaction::events::pointer::ScrollPhase;
    let settings = flui_interaction::GestureSettings::default();
    let millisecond = Duration::from_millis(1);
    for (phase, idle) in [
        (None, settings.contactless_latch_idle()),
        (Some(ScrollPhase::Changed), settings.phased_latch_idle()),
    ] {
        let outer = ScrollController::new();
        let inner = ScrollController::new();
        let mut scoped = nested_scrollables(&outer, &inner, Vsync::new());
        let first = phase.map(|_| ScrollPhase::Began);
        scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, first, 1));
        scoped.pump_for(idle.saturating_sub(millisecond));
        scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, phase, 1));
        assert_eq!(
            inner.pixels(),
            83.0,
            "{phase:?}: a burst remains latched just before the inactivity boundary"
        );
        assert_eq!(outer.pixels(), 0.0);
        scoped.pump_for(idle);
        scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, phase, 1));
        assert_eq!(
            outer.pixels(),
            30.0,
            "{phase:?}: {idle:?} of owner-clock inactivity releases the burst without sleeping"
        );
    }
}

pub(crate) fn scroll_latches_are_source_local_and_device_removal_releases() {
    use flui_interaction::events::pointer::*;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, Some(ScrollPhase::Began), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Began), 2));
    assert_eq!(
        outer.pixels(),
        30.0,
        "another source owns its independent claim"
    );
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(inner.pixels(), 83.0);
    assert_eq!(outer.pixels(), 30.0);
    scoped.dispatch_pointer_event(&PointerEvent::DeviceRemoved(PointerDeviceChange::new(
        DeviceId::try_from(1_u64).expect("device"),
        PointerKind::Mouse,
        flui_platform::EventTime::from_nanos(0),
    )));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        outer.pixels(),
        60.0,
        "a removed source cannot retain its stale claim"
    );
}

/// Shift turns a plain vertical wheel tick into horizontal scrolling: a
/// horizontal list takes it, a vertical list does not, and a device that
/// already reports horizontal motion is passed through unchanged.
pub(crate) fn shift_wheel_scrolls_the_horizontal_axis() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::events::Modifiers;

    let laid_out = |axis: Axis, scroll: &ScrollController| {
        scroll
            .update_dimensions(300.0, 0.0, 700.0)
            .expect("ordered extents");
        lay_out(
            Scrollable::new()
                .scroll_direction(axis)
                .controller(scroll.clone())
                .child(SizedBox::new(1000.0, 1000.0)),
            tight(300.0, 300.0),
        )
    };

    let horizontal = ScrollController::new();
    let h = laid_out(Axis::Horizontal, &horizontal);
    h.dispatch_scroll(150.0, 150.0, 0.0, 53.0);
    assert_eq!(horizontal.pixels(), 0.0, "a plain wheel tick is vertical");
    h.dispatch_scroll_with_modifiers(150.0, 150.0, 0.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        horizontal.pixels(),
        53.0,
        "shift+wheel scrolls a horizontal list"
    );
    h.dispatch_scroll_with_modifiers(150.0, 150.0, 20.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        horizontal.pixels(),
        73.0,
        "reported horizontal motion is kept"
    );

    let vertical = ScrollController::new();
    let v = laid_out(Axis::Vertical, &vertical);
    v.dispatch_scroll_with_modifiers(150.0, 150.0, 0.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        vertical.pixels(),
        0.0,
        "shift+wheel does not scroll a vertical list"
    );
}

fn assert_bounce(current: f64, proposed: f64, expected: f64) {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    let actual = BouncingScrollPhysics::new()
        .apply_boundary_conditions(&ScrollMetrics::new(current, 0.0, 100.0, 300.0), proposed);
    assert!(
        (actual - expected).abs() < 1e-10,
        "current={current}, proposed={proposed}: expected={expected}, actual={actual}"
    );
}

// Expected values integrate the resistance `0.52 · (1 − overscroll / 300)²`
// over the pull: `0.52·p·s² / (1 + 0.52·p·s / 300)`, `s = 1 − overscroll / 300`.

pub(crate) fn bouncing_lower_edge_preserves_outward_direction() {
    assert_bounce(0.0, -100.0, -44.318_181_818_181_82);
    assert_bounce(-52.0, -62.0, -55.503_365_114_974_76);
}

pub(crate) fn bouncing_upper_edge_preserves_outward_direction() {
    assert_bounce(100.0, 200.0, 144.318_181_818_181_8);
    assert_bounce(152.0, 162.0, 155.503_365_114_974_8);
}

/// The rubber band stiffens as the overscroll grows: a 1200 px pull from
/// the edge of a 600 px viewport moves the content about 306 px out, not
/// 624, no pull however long carries it a viewport's length out, and a pull
/// split over many events lands where one event carrying it all does.
pub(crate) fn bouncing_resistance_stiffens_toward_the_viewport_length() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    let physics = BouncingScrollPhysics::new();
    let pull = |from: f64, by: f64| {
        physics.apply_boundary_conditions(&ScrollMetrics::new(from, 0.0, 1000.0, 600.0), from - by)
    };
    let whole = pull(0.0, 1200.0);
    assert!(
        (whole + 305.882_352_941_176_5).abs() < 1e-9,
        "one 1200 px pull: {whole}"
    );
    let split = (0..12).fold(0.0, |at, _| pull(at, 100.0));
    assert!(
        (split - whole).abs() < 1e-9,
        "twelve 100 px pulls {split} land where one 1200 px pull {whole} does"
    );
    let far = pull(0.0, 1e12);
    assert!(
        far > -600.0 && far < -599.0,
        "an endless pull nears the viewport's length: {far}"
    );
    let past =
        physics.apply_boundary_conditions(&ScrollMetrics::new(1000.0, 0.0, 1000.0, 600.0), 2200.0);
    assert!(
        (past - 1_305.882_352_941_177).abs() < 1e-9,
        "the upper edge resists alike: {past}"
    );
}

pub(crate) fn bouncing_stationary_input_preserves_overscroll() {
    assert_bounce(-52.0, -52.0, -52.0);
    assert_bounce(152.0, 152.0, 152.0);
}

pub(crate) fn bouncing_inward_motion_and_crossing_respect_the_new_edge() {
    assert_bounce(-52.0, -42.0, -42.0);
    assert_bounce(152.0, 142.0, 142.0);
    assert_bounce(-52.0, 120.0, 110.051_546_391_752_6);
    assert_bounce(152.0, -20.0, -10.051_546_391_752_58);
}

fn animated_scroll_content(scroll: &ScrollController, vsync: &Vsync) -> impl flui_view::View {
    VsyncScope::new(
        vsync.clone(),
        Scrollable::new()
            .controller(scroll.clone())
            .child(SizedBox::new(300.0, 5000.0)),
    )
}

fn advance_scroll_run(laid: &mut LaidOut) {
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
    }
}

pub(crate) fn replacing_vsync_releases_old_seats_and_drives_fresh_contacts() {
    use flui_foundation::geometry::EdgeInsets;
    use flui_widgets::{InteractiveViewer, TransformationController};

    let mut failures = Vec::new();
    for (family, fail_stop) in [
        ("scrollable", false),
        ("scrollable", true),
        ("viewer", false),
    ] {
        let scroll = ScrollController::new();
        let transform = TransformationController::new();
        let child = match family {
            "scrollable" => Scrollable::new()
                .controller(scroll.clone())
                .child(SizedBox::new(300.0, 5000.0))
                .boxed(),
            _ => InteractiveViewer::new(crate::common::viewer_alternatives())
                .controller(transform.clone())
                .boundary_margin(EdgeInsets::all(1000.0))
                .scale_enabled(false)
                .child(SizedBox::new(300.0, 300.0))
                .boxed(),
        };
        let pixels = || {
            if family == "viewer" {
                -transform.value().to_col_major_array()[13]
            } else {
                scroll.pixels()
            }
        };
        let first = Vsync::new();
        let second = Vsync::new();
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(first.clone(), child.clone()),
            tight(300.0, 300.0),
            first.clone(),
        );
        let fling = |laid: &LaidOut| {
            laid.dispatch_pointer_down(150.0, 250.0);
            for y in [230.0, 210.0, 190.0, 170.0, 150.0] {
                laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
            }
            laid.dispatch_pointer_up(150.0, 150.0);
        };
        fling(&laid);
        let released = pixels();
        advance_scroll_run(&mut laid);
        assert!(
            pixels() > released,
            "{family}: initial owner drives real inertia"
        );
        let before_same = pixels();
        laid.pump_widget(VsyncScope::new(first, child.clone()));
        laid.pump_for(Duration::from_millis(16));
        if pixels() <= before_same {
            failures.push(format!("{family}: same owner rebuild stopped real inertia"));
        }
        // A fresh, not-yet-ticked release independently exercises clock
        // replacement even if the preceding same-owner control fails.
        fling(&laid);

        let armed = Rc::new(Cell::new(fail_stop));
        let attempted = Rc::new(Cell::new(0_usize));
        let position = scroll.position();
        let (watched, fail, seen) = (position.clone(), armed, attempted.clone());
        let activity_listener = position.add_activity_listener(Rc::new(move || {
            if !watched.is_scrolling() && fail.replace(false) {
                seen.set(seen.get() + 1);
                panic!("clock replacement activity failure");
            }
        }));
        let replacement = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            laid.pump_widget(VsyncScope::new(second.clone(), child.clone()));
            if family == "scrollable" && fail_stop {
                // Clock transfer preserves scrolling. Explicit cancellation
                // still exercises the one-shot activity failure and recovery.
                scroll.jump_to(pixels()).expect("no build is running");
            }
        }));
        if let Err(payload) = replacement {
            assert!(fail_stop, "healthy clock replacement must not unwind");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("clock replacement activity failure"),
                "clock replacement preserves the first callback failure"
            );
        }
        assert_eq!(
            attempted.get(),
            usize::from(fail_stop),
            "{family}: retirement reaches the one-shot activity failure"
        );
        let replaced = pixels();
        // Repeating the accepted new registry cannot hide a seat left on the
        // old clock after cancellation notified a failing activity observer.
        laid.pump_widget(VsyncScope::new(second.clone(), child));
        assert_eq!(
            pixels(),
            replaced,
            "{family}: retry on the same new clock preserves sampled pixels"
        );
        laid.pump_for(Duration::from_millis(16));
        if pixels() != replaced {
            failures.push(format!("{family}: retired Vsync still advances content"));
        }
        fling(&laid);
        let fresh_release = pixels();
        let mut clock = flui_animation::MotionClock::new();
        second.tick_all(&clock.frame(Duration::from_secs(1)));
        second.tick_all(&clock.frame(Duration::from_millis(1032)));
        if pixels() <= fresh_release {
            failures.push(format!(
                "{family}: replacement Vsync cannot drive fresh inertia"
            ));
        }
        position.remove_activity_listener(activity_listener);
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

pub(crate) fn a_repeated_frame_does_not_cancel_viewer_inertia() {
    use flui_foundation::geometry::EdgeInsets;
    use flui_widgets::{InteractiveViewer, TransformationController};

    let controller = TransformationController::new();
    let vsync = Vsync::new();
    let content = VsyncScope::new(
        vsync.clone(),
        InteractiveViewer::new(crate::common::viewer_alternatives())
            .controller(controller.clone())
            .boundary_margin(EdgeInsets::all(1000.0))
            .scale_enabled(false)
            .child(SizedBox::new(300.0, 300.0)),
    );
    let mut laid = crate::common::lay_out_animated(content, tight(300.0, 300.0), vsync);
    let fling = |laid: &LaidOut| {
        laid.dispatch_pointer_down(150.0, 250.0);
        for y in [230.0, 210.0, 190.0, 170.0, 150.0] {
            laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
        }
        laid.dispatch_pointer_up(150.0, 150.0);
    };
    let pixels = || -controller.value().to_col_major_array()[13];
    fling(&laid);
    let released = pixels();
    advance_scroll_run(&mut laid);
    assert!(pixels() > released, "real viewer inertia was admitted");
    let before_repeat = pixels();
    laid.pump_for(Duration::ZERO);
    assert_eq!(
        pixels(),
        before_repeat,
        "an equal-time frame publishes no extra motion"
    );
    laid.pump_for(Duration::from_millis(16));
    let continued = pixels() > before_repeat;
    fling(&laid);
    let fresh_release = pixels();
    advance_scroll_run(&mut laid);
    assert!(
        pixels() > fresh_release,
        "fresh contact recovers real inertia"
    );
    assert!(
        continued,
        "an equal-time frame cancelled the accepted viewer trajectory"
    );
}

pub(crate) fn replacing_a_scroll_position_cancels_its_contact_and_recovers() {
    use std::sync::Mutex;

    let mut failures = Vec::new();
    for (family, cancelled) in [("scrollable", false), ("scrollable", true)] {
        let old = ScrollController::new();
        let new = ScrollController::new();
        let vsync = Vsync::new();
        let incoming = new.position();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let watched = incoming.clone();
        let subscription = incoming.add_activity_listener(std::rc::Rc::new(move || {
            sink.lock()
                .expect("activity observer lock")
                .push((watched.is_scrolling(), watched.user_scroll_direction()));
        }));
        let content = |controller: &ScrollController| {
            VsyncScope::new(
                vsync.clone(),
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(300.0, 5000.0)),
            )
        };
        let mut laid =
            crate::common::lay_out_animated(content(&old), tight(300.0, 300.0), vsync.clone());
        laid.dispatch_pointer_down(150.0, 250.0);
        laid.dispatch_pointer_move_after(150.0, 200.0, Duration::from_millis(10));
        laid.dispatch_pointer_move_after(150.0, 180.0, Duration::from_millis(10));
        assert!(old.pixels() > 0.0);
        assert!(old.position().is_scrolling());
        let same = old.pixels();
        laid.pump_widget(content(&old));
        laid.dispatch_pointer_move_after(150.0, 160.0, Duration::from_millis(10));
        assert!(
            old.pixels() > same,
            "same position preserves the admitted contact"
        );

        laid.pump_widget(content(&new));
        let retired = old.pixels();
        let before = new.pixels();
        laid.dispatch_pointer_move_after(150.0, 140.0, Duration::from_millis(10));
        if new.pixels() != before {
            failures.push(format!(
                "{family}: replacement position consumed the retired contact"
            ));
        }
        assert_eq!(old.pixels(), retired);
        assert!(!old.position().is_scrolling());
        if cancelled {
            laid.dispatch_pointer_cancel();
        } else {
            laid.dispatch_pointer_up(150.0, 140.0);
        }
        advance_scroll_run(&mut laid);
        if new.pixels() != before || incoming.is_scrolling() {
            failures.push(format!(
                "{family}: retired terminal started replacement motion or activity"
            ));
        }
        observed.lock().expect("activity observer lock").clear();
        let fresh = new.pixels();
        laid.dispatch_pointer_down(150.0, 250.0);
        laid.dispatch_pointer_move_after(150.0, 200.0, Duration::from_millis(10));
        laid.dispatch_pointer_move_after(150.0, 180.0, Duration::from_millis(10));
        assert!(
            new.pixels() > fresh,
            "fresh contact drives replacement content"
        );
        assert!(
            observed
                .lock()
                .expect("activity observer lock")
                .contains(&(true, ScrollDirection::Reverse)),
            "fresh movement reaches the real activity consumer"
        );
        laid.dispatch_pointer_cancel();
        assert!(!incoming.is_scrolling());
        incoming.remove_activity_listener(subscription);
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

fn dispatch_typed_wheel(
    laid: &LaidOut,
    precision: flui_platform::pointer::ScrollPrecision,
    dy: f64,
) {
    use flui_foundation::geometry::Point;
    use flui_platform::EventTime;
    use flui_platform::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent, ScrollUnit,
    };
    let event = ScrollEvent::new(
        PointerInfo::new(
            PointerId::try_from(1_u64).expect("nonzero mouse"),
            PointerKind::Mouse,
        ),
        EventTime::from_nanos(0),
        PointerPosition::try_new(Point::new(150.0, 100.0)).expect("finite wheel position"),
        ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("finite wheel distance"),
    )
    .with_precision(precision);
    laid.dispatch_pointer_event(&PointerEvent::Scroll(event));
}

#[derive(Clone, flui_view::prelude::StatelessView)]
struct AuthoredWheelContent {
    axis: flui_foundation::geometry::Axis,
    controller: ScrollController,
    provider: flui_interaction::WheelPreferencesProvider,
    distances: flui_widgets::WheelScrollDistances,
    extent: f64,
}

impl flui_view::StatelessView for AuthoredWheelContent {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        let arena = flui_widgets::GestureArenaScope::of(ctx);
        let scroll = Scrollable::new()
            .scroll_direction(self.axis)
            .controller(self.controller.clone())
            .wheel_distances(self.distances)
            .child(SizedBox::new(self.extent, self.extent));
        let inner = flui_widgets::GestureArenaScope::new(arena.clone(), scroll)
            .settings(flui_interaction::GestureSettings::default());
        flui_widgets::GestureArenaScope::new(arena, inner).wheel_preferences(self.provider.clone())
    }
}

fn dispatch_wheel_distance(
    laid: &LaidOut,
    unit: flui_platform::pointer::ScrollUnit,
    x: f64,
    y: f64,
    modifiers: flui_platform::keyboard::Modifiers,
) {
    use flui_platform::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent,
    };
    laid.dispatch_pointer_event(&PointerEvent::Scroll(
        ScrollEvent::new(
            PointerInfo::new(
                PointerId::try_from(1_u64).expect("pointer"),
                PointerKind::Mouse,
            ),
            flui_platform::EventTime::from_nanos(1),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, 100.0))
                .expect("point"),
            ScrollDelta::try_new(unit, x, y).expect("finite wheel distance"),
        )
        .with_modifiers(modifiers),
    ));
}

pub(crate) fn wheel_policy_resolves_authored_axes_and_provider_replacement() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::WheelPreferencesSource;
    use flui_platform::{WheelPreferences, WheelStep, keyboard::Modifiers, pointer::ScrollUnit};
    use flui_widgets::WheelScrollDistances;

    for axis in [Axis::Vertical, Axis::Horizontal] {
        let source = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(3))
                .with_horizontal_characters(4),
        );
        let scroll = ScrollController::new();
        let content = AuthoredWheelContent {
            axis,
            controller: scroll.clone(),
            provider: source.provider(),
            distances: WheelScrollDistances::try_new(20.0, 7.0).expect("authored distances"),
            extent: 5000.0,
        };
        let mut laid = lay_out(content.clone(), tight(300.0, 300.0));
        for (label, unit, x, y, modifiers, vertical, horizontal) in [
            (
                "raw axis counts",
                ScrollUnit::Detents,
                1.0,
                1.0,
                Modifiers::NONE,
                60.0,
                28.0,
            ),
            (
                "fractional raw axis counts",
                ScrollUnit::Detents,
                0.25,
                0.25,
                Modifiers::NONE,
                15.0,
                7.0,
            ),
            (
                "shift chooses receiving axis",
                ScrollUnit::Detents,
                0.0,
                1.0,
                Modifiers::SHIFT,
                0.0,
                28.0,
            ),
            (
                "existing horizontal axis is not shifted again",
                ScrollUnit::Detents,
                1.0,
                1.0,
                Modifiers::SHIFT,
                60.0,
                28.0,
            ),
            (
                "translated lines bypass counts",
                ScrollUnit::Lines,
                2.0,
                2.0,
                Modifiers::NONE,
                40.0,
                40.0,
            ),
            (
                "translated pages use viewport",
                ScrollUnit::Pages,
                0.5,
                0.5,
                Modifiers::NONE,
                150.0,
                150.0,
            ),
        ] {
            scroll.jump_to(0.0).expect("no build is running");
            dispatch_wheel_distance(&laid, unit, x, y, modifiers);
            assert_eq!(
                scroll.pixels(),
                if axis == Axis::Vertical {
                    vertical
                } else {
                    horizontal
                },
                "{label}: {axis:?}"
            );
        }
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(0))
                .with_horizontal_characters(0),
        );
        scroll.jump_to(0.0).expect("no build is running");
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(scroll.pixels(), 0.0, "zero disables only raw detents");
        dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 19.0, 17.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 17.0 } else { 19.0 },
            "pixel control"
        );

        // Different live sources begin with identical values. A replacement
        // must retain the new authority even when the old writer changes later.
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        scroll.jump_to(0.0).expect("no build is running");
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 40.0 } else { 14.0 }
        );
        let replacement = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        let mut replaced = content;
        replaced.provider = replacement.provider();
        laid.pump_widget(replaced);
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(0))
                .with_horizontal_characters(0),
        );
        replacement.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(5))
                .with_horizontal_characters(5),
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 140.0 } else { 49.0 },
            "replacement authority stays live in a retained scrollable"
        );
    }

    let source = WheelPreferencesSource::new(
        WheelPreferences::default().with_horizontal_characters(u32::MAX),
    );
    let scroll = ScrollController::new();
    let laid = lay_out(
        AuthoredWheelContent {
            axis: Axis::Horizontal,
            controller: scroll.clone(),
            provider: source.provider(),
            distances: WheelScrollDistances::try_new(20.0, 1e-8).expect("positive distances"),
            extent: 5000.0,
        },
        tight(300.0, 300.0),
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 0.0, Modifiers::NONE);
    assert!(
        (scroll.pixels() - 42.949_672_95).abs() < 1e-10,
        "horizontal UINT_MAX remains a character count, not a page sentinel"
    );
}

pub(crate) fn wheel_distances_refuse_overflow_and_the_next_packet_recovers() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::WheelPreferencesSource;
    use flui_platform::{WheelPreferences, WheelStep, keyboard::Modifiers, pointer::ScrollUnit};
    use flui_widgets::{InvalidWheelScrollDistance, WheelScrollDistances};

    for invalid in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            WheelScrollDistances::try_new(invalid, 7.0),
            Err(InvalidWheelScrollDistance::Line)
        );
        assert_eq!(
            WheelScrollDistances::try_new(20.0, invalid),
            Err(InvalidWheelScrollDistance::Character)
        );
    }
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let source = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        let scroll = ScrollController::new();
        let laid = lay_out(
            AuthoredWheelContent {
                axis,
                controller: scroll.clone(),
                provider: source.provider(),
                distances: WheelScrollDistances::try_new(f64::MAX, f64::MAX)
                    .expect("finite authored distances"),
                extent: 5000.0,
            },
            tight(300.0, 300.0),
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            0.0,
            "overflowing native count product is refused before page-like cap"
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Lines, 2.0, 2.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            0.0,
            "overflowing translated line product is refused"
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 9.0, 7.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 7.0 } else { 9.0 },
            "pixel recovery bypasses authored distance multipliers"
        );
    }

    // A real large content extent admits the first finite packet. A second
    // finite packet whose destination overflows must not turn into an end jump.
    let scroll = ScrollController::new();
    let laid = lay_out(
        AuthoredWheelContent {
            axis: Axis::Vertical,
            controller: scroll.clone(),
            provider: WheelPreferences::default().into(),
            distances: WheelScrollDistances::default(),
            extent: f64::MAX,
        },
        tight(300.0, 300.0),
    );
    let distance = f64::MAX * 0.75;
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        distance,
        "finite large content admits meaningful movement"
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        distance,
        "overflowing destination is refused rather than clamped"
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, -distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        0.0,
        "healthy input recovers after destination refusal"
    );
}

pub(crate) fn notched_wheel_accumulates_distance_and_eases_out_in_150ms() {
    use flui_platform::pointer::ScrollPrecision::Notched;
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync,
    );
    dispatch_typed_wheel(&laid, Notched, 50.0);
    dispatch_typed_wheel(&laid, Notched, 50.0);
    assert_eq!(
        scroll.pixels(),
        0.0,
        "notches start a trajectory without jumping"
    );
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(75));
    assert!(
        scroll.pixels() > 50.0 && scroll.pixels() < 100.0,
        "ease-out covers more than half the accepted distance halfway through: {}",
        scroll.pixels()
    );
    dispatch_typed_wheel(&laid, Notched, 50.0);
    dispatch_typed_wheel(&laid, Notched, -25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        125.0,
        "all accepted ticks accumulate, including reversal"
    );
    assert!(
        !scroll.position().is_scrolling(),
        "the settled wheel activity ends"
    );
    dispatch_typed_wheel(&laid, Notched, -25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        100.0,
        "a healthy next wheel trajectory completes"
    );
}

/// A click while a notched wheel's easing runs reaches the content as a tap
/// and the easing goes on to its destination; a press while a released
/// fling coasts catches it instead: the fling stops and the content sees no
/// tap (ADR-0208 rule 4).
pub(crate) fn a_click_during_wheel_easing_taps_and_a_press_on_a_fling_catches_it() {
    use flui_platform::pointer::ScrollPrecision::Notched;
    use flui_widgets::GestureDetector;
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let taps = Rc::new(Cell::new(0_u32));
    let tapped = Rc::clone(&taps);
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new().controller(scroll.clone()).child(
                GestureDetector::new()
                    .behavior(flui_widgets::HitTestBehavior::Opaque)
                    .on_tap(move |_| tapped.set(tapped.get() + 1))
                    .child(SizedBox::new(300.0, 5000.0)),
            ),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(50));
    let easing = scroll.pixels();
    assert!(easing > 0.0 && easing < 100.0, "the easing runs: {easing}");
    laid.dispatch_pointer_down(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(taps.get(), 1, "the click reaches the content");
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        scroll.pixels(),
        100.0,
        "the easing goes on to its destination"
    );

    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    let coasting = scroll.pixels();
    laid.pump_for(Duration::from_millis(16));
    assert!(scroll.pixels() > coasting, "the fling coasts");
    laid.dispatch_pointer_down(150.0, 150.0);
    let caught = scroll.pixels();
    laid.dispatch_pointer_up(150.0, 150.0);
    for _ in 0..10 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(taps.get(), 1, "the press that catches the fling is no tap");
    assert_eq!(scroll.pixels(), caught, "the press stops the fling");
}

/// Fingers that come to rest on a trackpad catch a released fling as a
/// press does, and leave a notched wheel's easing to reach its destination.
pub(crate) fn resting_fingers_catch_a_fling_and_leave_wheel_easing() {
    use flui_foundation::geometry::Offset;
    use flui_platform::pointer::ScrollPrecision::Notched;
    use flui_testing::TrackpadScript;
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync,
    );
    let rest = TrackpadScript::new("fingers rest", Offset::new(150.0, 150.0))
        .rest()
        .build()
        .expect("one discrete event");

    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(50));
    let easing = scroll.pixels();
    assert!(easing > 0.0 && easing < 100.0, "the easing runs: {easing}");
    laid.replay(&rest);
    laid.pump_for(Duration::from_millis(250));
    assert_eq!(
        scroll.pixels(),
        100.0,
        "the easing goes on to its destination"
    );

    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    let coasting = scroll.pixels();
    laid.pump_for(Duration::from_millis(16));
    assert!(scroll.pixels() > coasting, "the fling coasts");
    laid.replay(&rest);
    let caught = scroll.pixels();
    for _ in 0..10 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(scroll.pixels(), caught, "resting fingers stop the fling");
    assert!(
        !scroll.position().is_scrolling(),
        "the caught fling's activity ends"
    );
}

pub(crate) fn a_notched_wheel_run_migrates_with_its_accepted_destination() {
    use flui_platform::pointer::ScrollPrecision::Notched;
    let scroll = ScrollController::new();
    let old = Vsync::new();
    let next = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &old),
        tight(300.0, 300.0),
        old.clone(),
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(30));
    let before = scroll.pixels();
    assert!(before > 0.0 && before < 100.0);
    assert!(old.has_running());
    laid.pump_widget(animated_scroll_content(&scroll, &next));
    assert!(old.is_empty());
    laid.adopt_vsync(next.clone())
        .expect("replacement clock admission");
    laid.pump_for(Duration::ZERO);
    assert!((scroll.pixels() - before).abs() < 1e-8);
    laid.pump_for(Duration::from_millis(16));
    assert!(scroll.pixels() > before);
    // A subsequent notch accumulates against the retained destination, not
    // the intermediate painted position at registry replacement.
    dispatch_typed_wheel(&laid, Notched, 50.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(scroll.pixels(), 150.0);
    assert!(!scroll.position().is_scrolling());
    laid.pump_widget(SizedBox::shrink());
    assert!(next.is_empty());
}

pub(crate) fn precise_and_unknown_wheels_interrupt_synthetic_motion_once() {
    use flui_platform::pointer::ScrollPrecision::{Notched, Precise, Unknown};
    for precision in [Precise, Unknown] {
        let scroll = ScrollController::new();
        let vsync = Vsync::new();
        let mut laid = crate::common::lay_out_animated(
            animated_scroll_content(&scroll, &vsync),
            tight(300.0, 300.0),
            vsync,
        );
        dispatch_typed_wheel(&laid, precision, 20.0);
        assert_eq!(scroll.pixels(), 20.0, "{precision:?}: immediate control");
        laid.pump_for(Duration::from_millis(200));
        assert_eq!(scroll.pixels(), 20.0, "{precision:?}: no duplicated motion");
        dispatch_typed_wheel(&laid, Notched, 100.0);
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(60));
        let before = scroll.pixels();
        assert!(
            before > 20.0 && before < 120.0,
            "notched motion is still live"
        );
        dispatch_typed_wheel(&laid, precision, 10.0);
        assert_eq!(
            scroll.pixels(),
            before + 10.0,
            "immediate input starts at current pixels"
        );
        laid.pump_for(Duration::from_millis(200));
        assert_eq!(
            scroll.pixels(),
            before + 10.0,
            "interrupted target cannot overwrite input"
        );
        dispatch_typed_wheel(&laid, Notched, 30.0);
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(150));
        assert_eq!(
            scroll.pixels(),
            before + 40.0,
            "old accepted target was retired"
        );
    }
}

pub(crate) fn replacing_or_unmounting_a_scrollable_retires_its_notched_motion() {
    use flui_platform::pointer::ScrollPrecision::Notched;
    let old = ScrollController::new();
    let new = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&old, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    let retired = old.pixels();
    assert!(
        retired > 0.0 && retired < 100.0,
        "the old owner's motion is live"
    );
    laid.pump_widget(animated_scroll_content(&new, &vsync));
    dispatch_typed_wheel(&laid, Notched, 50.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        old.pixels(),
        retired,
        "replacement does not keep driving the old owner"
    );
    assert_eq!(
        new.pixels(),
        50.0,
        "replacement starts with its own accepted target"
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    let unmounted = new.pixels();
    laid.pump_widget(SizedBox::new(300.0, 300.0));
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        new.pixels(),
        unmounted,
        "unmount retires motion before future frames"
    );
}

pub(crate) fn dragging_interrupts_notched_motion_and_windows_progress_independently() {
    use flui_platform::pointer::ScrollPrecision::Notched;
    let scroll = ScrollController::new();
    let other = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync,
    );
    let other_vsync = Vsync::new();
    let mut independent = crate::common::lay_out_animated(
        animated_scroll_content(&other, &other_vsync),
        tight(300.0, 300.0),
        other_vsync,
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    dispatch_typed_wheel(&independent, Notched, 200.0);
    laid.pump_for(Duration::ZERO);
    independent.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    assert_eq!(
        other.pixels(),
        0.0,
        "another owner's clock has not advanced"
    );
    let grabbed = scroll.pixels();
    assert!(
        grabbed > 0.0 && grabbed < 100.0,
        "the grabbed motion is live"
    );
    // A wheel's easing is no motion a press catches: the press does not
    // hold it, and the drag that takes the contact past its slop stops the
    // easing where it shows and moves the content from there.
    laid.dispatch_pointer_down(150.0, 200.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    let started = scroll.pixels();
    assert!(
        started >= grabbed,
        "the drag starts from the displayed position: {started} from {grabbed}"
    );
    laid.dispatch_pointer_move(150.0, 170.0);
    let dragged = scroll.pixels();
    assert!(dragged > started, "and moves the content");
    laid.dispatch_pointer_cancel();
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        scroll.pixels(),
        dragged,
        "the cancelled drag leaves no old wheel motion"
    );
    independent.pump_for(Duration::from_millis(150));
    assert_eq!(
        other.pixels(),
        200.0,
        "the independent owner's trajectory survives"
    );
    dispatch_typed_wheel(&laid, Notched, 25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        dragged + 25.0,
        "next wheel uses the new drag position"
    );
}

pub(crate) fn a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook() {
    let old = ScrollController::new();
    let new = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&old, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    old.animate_to(
        1000.0,
        Duration::from_millis(300),
        SharedCurve::new(curves::LINEAR),
    )
    .expect("no build is running");
    advance_scroll_run(&mut laid);
    assert!(old.pixels() > 0.0);
    let retired = old.pixels();
    laid.pump_widget(animated_scroll_content(&new, &vsync));
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        new.pixels(),
        0.0,
        "old trajectory must not drive the new position"
    );
    assert_eq!(old.pixels(), retired);
    new.animate_to(
        900.0,
        Duration::from_millis(300),
        SharedCurve::new(curves::LINEAR),
    )
    .expect("no build is running");
    advance_scroll_run(&mut laid);
    let before = new.pixels();
    assert!(before > 0.0);
    old.jump_to(42.0).expect("no build is running");
    laid.pump_for(Duration::from_millis(16));
    assert!(
        new.pixels() > before,
        "retired controller must not stop the new run"
    );
    assert_eq!(old.pixels(), 42.0);
}

pub(crate) fn a_same_position_scrollable_rebuild_preserves_motion() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    scroll
        .animate_to(
            1000.0,
            Duration::from_millis(300),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    advance_scroll_run(&mut laid);
    let before = scroll.pixels();
    assert!(before > 0.0);
    laid.pump_widget(animated_scroll_content(&scroll, &vsync));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > before,
        "same position keeps its active trajectory"
    );

    // Without a controller of its own the position is the state's: a parent
    // rebuilt mid-drag (a layout builder, a focus change above it) keeps the
    // offset and the drag.
    let top = flui_interaction::focus::FocusNode::with_debug_label("content top");
    let own_position = |vsync: &Vsync| {
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new().child(flui_widgets::Column::new(vec![
                flui_widgets::Focus::new(SizedBox::new(300.0, 10.0))
                    .focus_node(Rc::clone(&top))
                    .boxed(),
                SizedBox::new(300.0, 5000.0).boxed(),
            ])),
        )
    };
    let vsync = Vsync::new();
    let mut laid =
        crate::common::lay_out_animated(own_position(&vsync), tight(300.0, 300.0), vsync.clone());
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 200.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.pump_for(Duration::from_millis(16));
    let dragged = -top.rect().top();
    assert!(dragged > 0.0, "the drag scrolled the content, by {dragged}");
    laid.pump_widget(own_position(&vsync));
    assert_eq!(
        -top.rect().top(),
        dragged,
        "a rebuild above it keeps the scrollable's own position"
    );
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.pump_for(Duration::from_millis(16));
    assert!(
        -top.rect().top() >= dragged + 49.0,
        "and the drag goes on, at {}",
        -top.rect().top()
    );
    laid.dispatch_pointer_up(150.0, 100.0);
}

pub(crate) fn retiring_one_scrollable_preserves_a_later_owners_jump_hook() {
    let scroll = ScrollController::new();
    let first_vsync = Vsync::new();
    let second_vsync = Vsync::new();
    let mut first = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &first_vsync),
        tight(300.0, 300.0),
        first_vsync.clone(),
    );
    let mut second = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &second_vsync),
        tight(300.0, 300.0),
        second_vsync.clone(),
    );
    second.dispatch_pointer_down(150.0, 250.0);
    second.dispatch_pointer_move(150.0, 180.0);
    second.dispatch_pointer_move(150.0, 150.0);
    second.dispatch_pointer_up(150.0, 150.0);
    let released = scroll.pixels();
    advance_scroll_run(&mut second);
    assert!(
        scroll.pixels() > released,
        "later attachment has its own live fling"
    );
    let before = scroll.pixels();
    assert!(before > 0.0);
    first.pump_widget(SizedBox::new(300.0, 300.0));
    // Equal-value jumps emit no position notification. Only the surviving
    // owner's synchronous hook can stop its run before the next tick writes.
    scroll.jump_to(before).expect("no build is running");
    second.pump_for(Duration::from_millis(16));
    assert_eq!(
        scroll.pixels(),
        before,
        "detaching a different owner must preserve cancellation"
    );
    scroll
        .animate_to(
            900.0,
            Duration::from_millis(300),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    advance_scroll_run(&mut second);
    assert!(scroll.pixels() > before, "next command still progresses");
}

pub(crate) fn cancelling_an_in_range_scroll_ends_activity_without_coasting() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    let cancelled_at = scroll.pixels();
    assert!(cancelled_at > 0.0);
    assert!(scroll.position().is_scrolling());
    laid.dispatch_pointer_cancel();
    for _ in 0..8 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        scroll.pixels(),
        cancelled_at,
        "cancel supplies no fling impulse"
    );
    assert!(!scroll.position().is_scrolling());
    assert_eq!(
        scroll.position().user_scroll_direction(),
        ScrollDirection::Idle
    );
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    let released_at = scroll.pixels();
    advance_scroll_run(&mut laid);
    assert!(
        scroll.pixels() > released_at,
        "next completed gesture still flings"
    );
}

pub(crate) fn cancelling_bouncing_overscroll_settles_without_release_velocity() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new()
                .controller(scroll.clone())
                .physics(Rc::new(BouncingScrollPhysics::new()))
                .child(SizedBox::new(300.0, 800.0)),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    let overscroll = scroll.pixels();
    assert!(overscroll < 0.0);
    laid.dispatch_pointer_cancel();
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > overscroll,
        "zero-impulse spring immediately recovers toward edge"
    );
    for _ in 0..160 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(scroll.pixels().abs() < 1.0);
    assert!(!scroll.position().is_scrolling());
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_up(150.0, 180.0);
    assert!(scroll.pixels() > 0.0, "next gesture advances content");
}

// ============================================================================
// Ballistic rest — device pixel ratio, extents, overscroll
// ============================================================================

/// Clamping physics that records the metrics each release hands it.
#[derive(Debug, Default)]
struct RecordingPhysics {
    ratios: std::sync::Mutex<Vec<f64>>,
}

impl flui_widgets::ScrollPhysics for RecordingPhysics {
    fn apply_boundary_conditions(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        proposed_pixels: f64,
    ) -> f64 {
        ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed_pixels)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn flui_animation::Simulation>> {
        self.ratios
            .lock()
            .expect("recording lock")
            .push(metrics.device_pixel_ratio);
        ClampingScrollPhysics::new().create_ballistic_simulation(metrics, velocity_px_per_sec)
    }
}

/// A release hands the physics the presentation's device pixel ratio, and a
/// fling rests once half a device pixel of glide remains: 8000 px/s at drag
/// 0.135 rests at 4.4874 s at a ratio of 1 and at 4.8336 s at a ratio of 2
/// (`8000·0.135ᵗ / |ln 0.135| = 0.5 / ratio`).
pub(crate) fn scroll_fling_rest_scales_with_device_pixel_ratio() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 4700.0)
        .expect("ordered extents");
    let recording = Rc::new(RecordingPhysics::default());
    let physics: SharedScrollPhysics = recording.clone();
    let widget = Scrollable::new()
        .controller(controller)
        .physics(physics)
        .child(SizedBox::new(300.0, 5000.0));
    let scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    scoped
        .pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);
    assert_eq!(
        *recording.ratios.lock().expect("recording lock"),
        [2.0],
        "the release reads the presentation's device pixel ratio"
    );

    let fling = |ratio: f64| {
        ClampingScrollPhysics::new()
            .create_ballistic_simulation(
                &ScrollMetrics::new(0.0, 0.0, 1e9, 300.0).with_device_pixel_ratio(ratio),
                8000.0,
            )
            .expect("a fling")
    };
    let (single, double) = (fling(1.0), fling(2.0));
    assert!(!single.is_done(4.4874) && single.is_done(4.4875));
    assert!(!double.is_done(4.8335) && double.is_done(4.8336));
    let bouncing = BouncingScrollPhysics::new().create_ballistic_simulation(
        &ScrollMetrics::new(0.0, 0.0, 1e9, 300.0).with_device_pixel_ratio(2.0),
        8000.0,
    );
    let bouncing = bouncing.expect("a fling");
    assert!(!bouncing.is_done(4.8335) && bouncing.is_done(4.8336));
}

/// Unordered or non-finite extents start no ballistic run, under every
/// physics, instead of panicking.
pub(crate) fn inverted_extents_do_not_fling() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    let physics: [&dyn ScrollPhysics; 2] =
        [&ClampingScrollPhysics::new(), &BouncingScrollPhysics::new()];
    for metrics in [
        ScrollMetrics::new(50.0, 100.0, 0.0, 300.0),
        ScrollMetrics::new(f64::NAN, 0.0, 100.0, 300.0),
        ScrollMetrics::new(50.0, 0.0, 100.0, 300.0).with_device_pixel_ratio(f64::NAN),
    ] {
        for physics in physics {
            assert!(
                physics
                    .create_ballistic_simulation(&metrics, 4000.0)
                    .is_none(),
                "{physics:?} under {metrics:?}"
            );
        }
    }
}

/// Extents that are unordered, NaN, infinite or over a negative viewport are
/// refused at the controller and leave the position as it was; physics handed
/// such metrics bound nothing instead of panicking.
pub(crate) fn unordered_extents_are_refused_and_bound_nothing() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, 500.0)
        .expect("ordered extents");
    for (viewport, min, max) in [
        (300.0, 300.0, 100.0),
        (300.0, f64::NAN, 100.0),
        (300.0, 0.0, f64::INFINITY),
        (f64::NAN, 0.0, 100.0),
        (-1.0, 0.0, 100.0),
    ] {
        let refused = controller.update_dimensions(viewport, min, max);
        assert!(refused.is_err(), "({viewport}, {min}, {max}) is refused");
        assert_eq!(
            (
                controller.viewport_dimension_pixels(),
                controller.min_scroll_extent(),
                controller.max_scroll_extent()
            ),
            (300.0, 0.0, 500.0),
            "a refusal keeps the extents"
        );
    }
    controller.jump_to(700.0).expect("no build is running");
    assert_eq!(
        controller.pixels(),
        500.0,
        "a jump still clamps to the kept range"
    );

    let physics: [&dyn ScrollPhysics; 2] =
        [&ClampingScrollPhysics::new(), &BouncingScrollPhysics::new()];
    for metrics in [
        ScrollMetrics::new(50.0, 100.0, 0.0, 300.0),
        ScrollMetrics::new(50.0, f64::NAN, 100.0, 300.0),
    ] {
        for physics in physics {
            assert_eq!(
                physics.apply_boundary_conditions(&metrics, 70.0),
                70.0,
                "{physics:?} under {metrics:?} bounds nothing"
            );
        }
        assert_eq!(
            metrics.page(1.0),
            50.0 / 300.0,
            "{metrics:?}: the page of the pixels"
        );
    }
}

/// A clamping release from past an edge, where content that shrank under
/// the offset leaves it, returns to the edge at any velocity, below the
/// fling threshold and moving outward alike, without passing it.
pub(crate) fn clamping_release_past_an_edge_returns_to_it() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    for velocity in [0.0, 30.0, 3000.0] {
        let simulation = ClampingScrollPhysics::new()
            .create_ballistic_simulation(&ScrollMetrics::new(600.0, 0.0, 500.0, 300.0), velocity)
            .expect("a return to the edge");
        let mut time = 0.0;
        while !simulation.is_done(time) {
            assert!(
                simulation.x(time) >= 500.0 - 0.5,
                "at {velocity} px/s the return stays out of the range: {} at {time}",
                simulation.x(time)
            );
            time += 1.0 / 60.0;
            assert!(time < 10.0, "at {velocity} px/s the return comes to rest");
        }
        assert!(
            (simulation.x(time) - 500.0).abs() <= 0.5,
            "rests on the edge"
        );
    }
}

/// A fling inside the range that friction carries past the edge overscrolls,
/// then springs back and rests exactly on the edge.
pub(crate) fn bouncing_fling_into_the_edge_overscrolls_and_returns() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f64;
    controller
        .update_dimensions(300.0, 0.0, max_extent)
        .expect("ordered extents");
    let physics: SharedScrollPhysics = Rc::new(BouncingScrollPhysics::new());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));
    let mut scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    controller.set_pixels(300.0);

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);
    assert!(
        controller.pixels() < max_extent,
        "released inside the range"
    );

    let mut furthest = controller.pixels();
    for _ in 0..240 {
        scoped.pump_for(Duration::from_millis(16));
        furthest = furthest.max(controller.pixels());
    }
    assert!(
        furthest > max_extent + 1.0,
        "the fling overscrolls past the edge; furthest {furthest:.3}"
    );
    assert_eq!(controller.pixels(), max_extent, "rests exactly on the edge");
}

/// A clamping fling that friction would carry past the edge stops on it.
pub(crate) fn clamping_fling_into_the_edge_rests_on_it() {
    let max_extent = 500.0_f64;
    let controller = ScrollController::new();
    controller
        .update_dimensions(300.0, 0.0, max_extent)
        .expect("ordered extents");
    let mut laid = fling_scoped(
        Scrollable::new()
            .controller(controller.clone())
            .child(SizedBox::new(300.0, 800.0)),
        Vsync::new(),
        tight(300.0, 300.0),
    );
    controller.set_pixels(300.0);
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    assert!(controller.pixels() < max_extent, "released inside");
    let mut furthest = controller.pixels();
    for _ in 0..240 {
        laid.pump_for(Duration::from_millis(16));
        furthest = furthest.max(controller.pixels());
    }
    assert_eq!(furthest, max_extent, "never passes the edge");
    assert_eq!(controller.pixels(), max_extent, "rests on the edge");
}

/// A fling down a lazy list whose first rows are taller than the rest: the
/// list estimates its end from the tall rows, the end moves in as the short
/// ones are measured while the list coasts, and the fling, under either
/// physics, rests on the end the measured rows give, never in the blank
/// space past it. Clamping never passes the end; bouncing may overscroll it
/// and comes back.
pub(crate) fn a_fling_rests_on_the_end_the_measured_rows_give() {
    let physics: [(SharedScrollPhysics, bool); 2] = [
        (Rc::new(ClampingScrollPhysics::new()), false),
        (Rc::new(BouncingScrollPhysics::new()), true),
    ];
    for (physics, bounces) in physics {
        let controller = ScrollController::new();
        let list = Scrollable::new()
            .controller(controller.clone())
            .physics(physics.clone())
            .viewport_builder(Rc::new(|position| {
                // 20 rows of 120 and 40 of 40: 4000 px over a 300 px viewport.
                flui_widgets::ListView::builder(60, 50.0, |index| {
                    (index < 60).then(|| {
                        SizedBox::new(300.0, if index < 20 { 120.0 } else { 40.0 }).boxed()
                    })
                })
                .position(position)
                .boxed()
            }));
        let mut laid = fling_scoped(list, Vsync::new(), tight(300.0, 300.0));
        laid.pump_for(Duration::from_millis(16));
        let estimated = controller.max_scroll_extent();
        assert!(
            estimated > 3700.0,
            "{physics:?}: the tall rows overestimate the end: {estimated}"
        );
        laid.dispatch_pointer_down(150.0, 290.0);
        laid.dispatch_pointer_move(150.0, 200.0);
        laid.dispatch_pointer_move(150.0, 100.0);
        laid.dispatch_pointer_move(150.0, 10.0);
        laid.dispatch_pointer_up(150.0, 10.0);
        for frame in 0..300 {
            laid.pump_for(Duration::from_millis(16));
            if !bounces {
                assert!(
                    controller.pixels() <= controller.max_scroll_extent(),
                    "{physics:?}: frame {frame} at {} is past the end {}",
                    controller.pixels(),
                    controller.max_scroll_extent()
                );
            }
        }
        assert_eq!(
            controller.max_scroll_extent(),
            3700.0,
            "{physics:?}: the measured end"
        );
        assert_eq!(
            controller.pixels(),
            3700.0,
            "{physics:?}: the fling rests on it"
        );
        assert!(
            !controller.position().is_scrolling(),
            "{physics:?}: and ends"
        );
    }
}

/// A list of `height` (a viewport of 300) under `snap`, its height read
/// again whenever `resized` notifies.
fn resizable_snapping_list(
    controller: &ScrollController,
    snap: Option<flui_widgets::SharedSnapPolicy>,
    height: &Rc<Cell<f64>>,
    resized: &Rc<flui_foundation::ChangeNotifier>,
    vsync: &Vsync,
) -> impl flui_view::View {
    resizable_list(controller, height, resized, vsync, |list| match snap {
        Some(snap) => list.snap_policy(snap),
        None => list,
    })
}

/// A list of `height` (a viewport of 300) configured by `configure`, its
/// height read again whenever `resized` notifies.
fn resizable_list(
    controller: &ScrollController,
    height: &Rc<Cell<f64>>,
    resized: &Rc<flui_foundation::ChangeNotifier>,
    vsync: &Vsync,
    configure: impl FnOnce(Scrollable) -> Scrollable,
) -> impl flui_view::View {
    let height = Rc::clone(height);
    let listenable = Rc::clone(resized) as Rc<dyn flui_foundation::Listenable>;
    let content =
        flui_widgets::AnimatedBuilder::new(listenable, move || SizedBox::new(300.0, height.get()));
    let list = Scrollable::new()
        .controller(controller.clone())
        .child(content);
    VsyncScope::new(vsync.clone(), configure(list))
}

/// A fling under `physics` down a 5000 px range that a rebuild switches to
/// `rebuilt` physics three frames in, as the end moves in to 1000: each
/// frame's offset and range.
fn fling_rebuilt_as_the_end_moves_in(
    physics: SharedScrollPhysics,
    rebuilt: SharedScrollPhysics,
) -> Vec<(f64, f64)> {
    use crate::nested_scroll::{Device, at, fling};
    let (controller, vsync) = (ScrollController::new(), Vsync::new());
    let height = Rc::new(Cell::new(5300.0));
    let resized = Rc::new(flui_foundation::ChangeNotifier::new());
    let first = Rc::clone(&physics);
    let mut laid = crate::common::lay_out_animated(
        resizable_list(&controller, &height, &resized, &vsync, |list| {
            list.physics(first)
        }),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    laid.replay(&fling(Device::Touch, at(150.0, 250.0), at(150.0, 100.0)));
    let mut frames = Vec::new();
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
        frames.push((controller.pixels(), controller.max_scroll_extent()));
    }
    laid.pump_widget(resizable_list(
        &controller,
        &height,
        &resized,
        &vsync,
        |list| list.physics(rebuilt),
    ));
    height.set(1300.0);
    resized.notify_listeners();
    for _ in 0..240 {
        laid.pump_for(Duration::from_millis(16));
        frames.push((controller.pixels(), controller.max_scroll_extent()));
    }
    frames
}

/// A fling a rebuild gave other physics keeps its own when the end moves in
/// under it: a bouncing fling rebuilt with clamping physics still
/// overscrolls the new end and comes back, as a set physics leaves a run in
/// progress its simulation.
pub(crate) fn a_rebuilt_run_keeps_the_physics_its_release_started_with() {
    let frames = fling_rebuilt_as_the_end_moves_in(
        Rc::new(BouncingScrollPhysics::new()),
        Rc::new(ClampingScrollPhysics::new()),
    );
    let past = frames
        .iter()
        .map(|(pixels, max)| pixels - max)
        .fold(f64::MIN, f64::max);
    assert!(
        past > 1.0,
        "the bouncing fling overscrolls the new end by {past}"
    );
    assert_eq!(frames.last(), Some(&(1000.0, 1000.0)), "and rests on it");
}

/// Clamping physics that writes the offset to `JUMP_TO` once, from its
/// `boundary_velocity`, after it is armed: user code taking over the start
/// of a run.
#[derive(Debug, Default)]
struct JumpingPhysics;

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static JUMP: std::cell::RefCell<Option<(ScrollController, f64)>> =
        const { std::cell::RefCell::new(None) };
}

impl flui_widgets::ScrollPhysics for JumpingPhysics {
    fn apply_boundary_conditions(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        proposed: f64,
    ) -> f64 {
        ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity: f64,
    ) -> Option<Box<dyn flui_animation::Simulation>> {
        ClampingScrollPhysics::new().create_ballistic_simulation(metrics, velocity)
    }

    fn boundary_velocity(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity: f64,
    ) -> Option<f64> {
        if let Some((controller, to)) = JUMP.with_borrow_mut(Option::take) {
            controller.set_pixels(to);
        }
        ClampingScrollPhysics::new().boundary_velocity(metrics, velocity)
    }
}

/// A physics that moves the offset while a run is rebuilt for new extents
/// takes the run over: the obsolete run stops where the physics put the
/// offset instead of applying its stale motion from there.
pub(crate) fn a_rebuild_taken_over_by_its_physics_stops_the_obsolete_run() {
    use crate::nested_scroll::{Device, at, fling};
    let (controller, vsync) = (ScrollController::new(), Vsync::new());
    let height = Rc::new(Cell::new(5300.0));
    let resized = Rc::new(flui_foundation::ChangeNotifier::new());
    let mut laid = crate::common::lay_out_animated(
        resizable_list(&controller, &height, &resized, &vsync, |list| {
            list.physics(Rc::new(JumpingPhysics))
        }),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    laid.replay(&fling(Device::Touch, at(150.0, 250.0), at(150.0, 100.0)));
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
    }
    let coasting = controller.pixels();
    laid.pump_for(Duration::from_millis(16));
    assert!(controller.pixels() > coasting, "the fling coasts");
    JUMP.with_borrow_mut(|jump| *jump = Some((controller.clone(), 300.0)));
    height.set(4300.0);
    resized.notify_listeners();
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        JUMP.with_borrow(Option::is_none),
        "the rebuild asked the physics"
    );
    assert_eq!(
        controller.pixels(),
        300.0,
        "the run stopped where the physics put it"
    );
    assert!(
        !controller.position().is_scrolling(),
        "and its activity ended"
    );
}

/// Anchors at 0, 500 and 1000 px.
fn three_anchors() -> flui_widgets::SharedSnapPolicy {
    Rc::new(flui_widgets::AnchorSnap::try_new([0.0, 500.0, 1000.0]).expect("three anchors"))
}

/// A drag from 800 that comes to rest at 740 and lifts: the snap settles
/// toward the anchor at 500. Returns the laid-out list one frame into it.
fn settling_toward_500(
    controller: &ScrollController,
    height: &Rc<Cell<f64>>,
    resized: &Rc<flui_foundation::ChangeNotifier>,
    vsync: &Vsync,
) -> LaidOut {
    use crate::nested_scroll::{Device, at, slow};
    let mut laid = crate::common::lay_out_animated(
        resizable_snapping_list(controller, Some(three_anchors()), height, resized, vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    controller.jump_to(800.0).expect("no build is running");
    laid.pump_for(Duration::from_millis(16));
    laid.replay(&slow(Device::Touch, at(150.0, 100.0), at(150.0, 160.0)));
    for _ in 0..4 {
        laid.pump_for(Duration::from_millis(16));
    }
    let settling = controller.pixels();
    assert!(
        settling > 600.0 && settling < 740.0,
        "the snap settles toward 500, at {settling}"
    );
    laid
}

/// A snap whose target the new extents still hold, while layout clamps the
/// offset under it: the end moves in from 1000 to 600 while the settle is
/// above 600, and the settle still rests on its anchor at 500.
pub(crate) fn a_snap_rests_on_its_target_when_the_end_clamps_the_offset() {
    let (controller, vsync) = (ScrollController::new(), Vsync::new());
    let height = Rc::new(Cell::new(1300.0));
    let resized = Rc::new(flui_foundation::ChangeNotifier::new());
    let mut laid = settling_toward_500(&controller, &height, &resized, &vsync);
    height.set(900.0);
    resized.notify_listeners();
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(controller.max_scroll_extent(), 600.0, "the end moved in");
    assert!(
        (controller.pixels() - 500.0).abs() <= 0.5,
        "the settle rests on its anchor, at {}",
        controller.pixels()
    );
}

/// A run rebuilt for new extents keeps the mode its release started in: a
/// snap whose policy a rebuild removed still settles on its anchor, and a
/// fling under a policy a rebuild installed still coasts where the physics
/// takes it.
pub(crate) fn a_rebuilt_run_keeps_the_mode_its_release_started_in() {
    use crate::nested_scroll::{Device, at, fling};
    let resized = Rc::new(flui_foundation::ChangeNotifier::new());

    let (controller, vsync) = (ScrollController::new(), Vsync::new());
    let height = Rc::new(Cell::new(1300.0));
    let mut laid = settling_toward_500(&controller, &height, &resized, &vsync);
    laid.pump_widget(resizable_snapping_list(
        &controller,
        None,
        &height,
        &resized,
        &vsync,
    ));
    height.set(1200.0);
    resized.notify_listeners();
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        (controller.pixels() - 500.0).abs() <= 0.5,
        "a snap whose policy was removed still rests on its anchor, at {}",
        controller.pixels()
    );

    let coast = |install: bool| {
        let (controller, vsync) = (ScrollController::new(), Vsync::new());
        let height = Rc::new(Cell::new(5300.0));
        let mut laid = crate::common::lay_out_animated(
            resizable_snapping_list(&controller, None, &height, &resized, &vsync),
            tight(300.0, 300.0),
            vsync.clone(),
        );
        laid.replay(&fling(Device::Touch, at(150.0, 250.0), at(150.0, 100.0)));
        for _ in 0..3 {
            laid.pump_for(Duration::from_millis(16));
        }
        if install {
            let snap = Some(three_anchors());
            laid.pump_widget(resizable_snapping_list(
                &controller,
                snap,
                &height,
                &resized,
                &vsync,
            ));
        }
        height.set(4300.0);
        resized.notify_listeners();
        for _ in 0..240 {
            laid.pump_for(Duration::from_millis(16));
        }
        controller.pixels()
    };
    let coasted = coast(false);
    assert!(
        coasted > 500.0,
        "the fling coasts past the anchors' reach: {coasted}"
    );
    assert_eq!(
        coast(true),
        coasted,
        "a policy installed while the fling coasts leaves it to the physics"
    );
}

/// A scrollable rebuilt with a fresh physics value every frame, as a build
/// that writes `.physics(Rc::new(..))` does, keeps its fling: the run goes
/// on, bounces past the edge and comes back to rest exactly on it, and a
/// press after it rests reaches the content as a tap.
pub(crate) fn a_fresh_physics_each_build_keeps_the_fling_and_its_bounce() {
    use flui_widgets::GestureDetector;
    let controller = ScrollController::new();
    let max_extent = 500.0_f64;
    let vsync = Vsync::new();
    let taps = Rc::new(Cell::new(0_u32));
    let tree = || {
        let taps = Rc::clone(&taps);
        let physics: SharedScrollPhysics = Rc::new(BouncingScrollPhysics::new());
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new()
                .controller(controller.clone())
                .physics(physics)
                .child(
                    GestureDetector::new()
                        .behavior(flui_widgets::HitTestBehavior::Opaque)
                        .on_tap(move |_| taps.set(taps.get() + 1))
                        .child(SizedBox::new(300.0, 800.0)),
                ),
        )
    };
    let mut laid = crate::common::lay_out_animated(tree(), tight(300.0, 300.0), vsync.clone());
    controller.set_pixels(300.0);
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    assert!(
        controller.pixels() < max_extent,
        "released inside the range"
    );

    let mut furthest = controller.pixels();
    for frame in 0..240 {
        let before = controller.pixels();
        laid.pump_widget(tree());
        laid.pump_for(Duration::from_millis(16));
        if frame < 3 {
            assert!(
                controller.pixels() > before,
                "frame {frame}: the rebuilt scrollable keeps flinging, at {}",
                controller.pixels()
            );
        }
        furthest = furthest.max(controller.pixels());
    }
    assert!(
        furthest > max_extent + 1.0,
        "the fling bounces past the edge; furthest {furthest:.3}"
    );
    assert_eq!(controller.pixels(), max_extent, "rests exactly on the edge");

    laid.dispatch_pointer_down(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(taps.get(), 1, "a press on the resting list is a tap");
}

fn nested_fling_content(
    outer: &ScrollController,
    inner: &ScrollController,
    vsync: &Vsync,
    outer_axis: flui_foundation::geometry::Axis,
    inner_axis: flui_foundation::geometry::Axis,
    inner_reversed: bool,
    bouncing: bool,
) -> impl flui_view::View {
    use flui_foundation::geometry::Axis;
    let mut child = Scrollable::new()
        .controller(inner.clone())
        .scroll_direction(inner_axis)
        .child(match inner_axis {
            Axis::Vertical => SizedBox::new(300.0, 1000.0),
            Axis::Horizontal => SizedBox::new(1000.0, 300.0),
        });
    if inner_reversed {
        child = child
            .axis_direction(match inner_axis {
                Axis::Vertical => flui_rendering::constraints::AxisDirection::BottomToTop,
                Axis::Horizontal => flui_rendering::constraints::AxisDirection::RightToLeft,
            })
            .viewport_builder(Rc::new(move |position| {
                flui_widgets::SingleChildScrollView::new()
                    .scroll_direction(inner_axis)
                    .reverse(true)
                    .position(position)
                    .child(match inner_axis {
                        Axis::Vertical => SizedBox::new(300.0, 1000.0),
                        Axis::Horizontal => SizedBox::new(1000.0, 300.0),
                    })
                    .boxed()
            }));
    }
    if bouncing {
        child = child.physics(Rc::new(BouncingScrollPhysics::new()));
    }
    let child = flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT)
        .child(SizedBox::new(200.0, 200.0).child(child));
    let content: flui_view::BoxedView = match outer_axis {
        Axis::Vertical => flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 600.0).boxed(),
            SizedBox::new(300.0, 200.0).child(child).boxed(),
            SizedBox::new(300.0, 4800.0).boxed(),
        ])
        .boxed(),
        Axis::Horizontal => flui_widgets::Row::new(vec![
            SizedBox::new(600.0, 300.0).boxed(),
            SizedBox::new(200.0, 300.0).child(child).boxed(),
            SizedBox::new(4800.0, 300.0).boxed(),
        ])
        .boxed(),
    };
    VsyncScope::new(
        vsync.clone(),
        Scrollable::new()
            .controller(outer.clone())
            .scroll_direction(outer_axis)
            .child(content),
    )
}

fn release_inner_fling(laid: &LaidOut, axis: flui_foundation::geometry::Axis, reversed: bool) {
    use flui_foundation::geometry::Axis;
    let samples = if reversed {
        [50.0, 80.0, 100.0]
    } else {
        [150.0, 120.0, 100.0]
    };
    let at = |main| match axis {
        Axis::Vertical => (100.0, main),
        Axis::Horizontal => (main, 100.0),
    };
    let (x, y) = at(samples[0]);
    laid.dispatch_pointer_down(x, y);
    for main in samples.into_iter().skip(1) {
        let (x, y) = at(main);
        laid.dispatch_pointer_move(x, y);
    }
    let (x, y) = at(samples[2]);
    laid.dispatch_pointer_up(x, y);
}

pub(crate) fn nested_fling_hands_remaining_velocity_to_matching_parent_axes() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, axis, axis, false, false),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        release_inner_fling(&laid, axis, false);
        assert_eq!(
            inner.pixels(),
            670.0,
            "{axis:?}: actual inner drag receives the post-threshold move"
        );
        assert_eq!(
            outer.pixels(),
            600.0,
            "parent does not also consume the drag"
        );
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(inner.pixels(), 800.0, "inner hits its real hard extent");
        assert!(
            outer.pixels() > 600.0,
            "{axis:?}: residual fling reaches parent, got {}",
            outer.pixels()
        );
        for _ in 0..500 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            !outer.position().is_scrolling(),
            "parent eventually settles"
        );
        assert!(
            !inner.position().is_scrolling(),
            "inner retires its transferred motion"
        );
    }
}

pub(crate) fn nested_fling_parent_boundary_policy_receives_presentation_pixel_ratio() {
    use flui_animation::Simulation;
    use flui_platform::{
        EventTime, PlatformInput,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
            PointerMove, PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };
    use flui_testing::{HeadlessHost, HeadlessWindow};
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    #[derive(Debug)]
    struct HighDensityPhysics;
    impl ScrollPhysics for HighDensityPhysics {
        fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed: f64) -> f64 {
            if metrics.device_pixel_ratio != 2.0 {
                return metrics.pixels;
            }
            ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed)
        }
        fn create_ballistic_simulation(
            &self,
            metrics: &ScrollMetrics,
            velocity: f64,
        ) -> Option<Box<dyn Simulation>> {
            ClampingScrollPhysics::new().create_ballistic_simulation(metrics, velocity)
        }
        fn boundary_velocity(&self, metrics: &ScrollMetrics, velocity: f64) -> Option<f64> {
            ClampingScrollPhysics::new().boundary_velocity(metrics, velocity)
        }
    }

    for density_sensitive in [false, true] {
        let (outer, inner) = (ScrollController::new(), ScrollController::new());
        let physics: SharedScrollPhysics = if density_sensitive {
            Rc::new(HighDensityPhysics)
        } else {
            Rc::new(ClampingScrollPhysics::new())
        };
        let parent = Scrollable::new()
            .controller(outer.clone())
            .physics(physics)
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 600.0).boxed(),
                SizedBox::new(300.0, 200.0)
                    .child(
                        flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT).child(
                            SizedBox::new(200.0, 200.0).child(
                                Scrollable::new()
                                    .controller(inner.clone())
                                    .child(SizedBox::new(300.0, 1000.0)),
                            ),
                        ),
                    )
                    .boxed(),
                SizedBox::new(300.0, 4800.0).boxed(),
            ]));
        let mut host = HeadlessHost::new(HeadlessWindow::new(300, 300));
        host.set_scale_factor(host.primary_window(), 2.0);
        host.attach(&parent)
            .expect("mount dense nested scroll owner");
        let _ = host.pump(Duration::ZERO);
        assert_eq!(inner.position().max_scroll_extent(), 800.0);
        for id in 1_u64..=2 {
            outer.jump_to(600.0).expect("no build is running");
            inner.jump_to(650.0).expect("no build is running");
            let _ = host.pump(Duration::ZERO);
            let pointer = PointerInfo::new(
                PointerId::try_from(id).expect("nonzero contact"),
                PointerKind::Mouse,
            );
            let sample = |y: f64| {
                PointerSample::new(
                    EventTime::from_nanos(
                        u64::try_from(host.clock().elapsed().as_nanos())
                            .expect("fixture clock fits event time"),
                    ),
                    PointerPosition::try_new(flui_foundation::geometry::Point::new(100.0, y))
                        .expect("finite authored mouse position"),
                )
            };
            host.dispatch(PlatformInput::Pointer(PointerEvent::Down(
                PointerPress::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::only(PointerButton::PRIMARY),
                    sample(150.0),
                ),
            )));
            for y in [120.0, 100.0] {
                host.clock().advance(Duration::from_millis(8));
                host.dispatch(PlatformInput::Pointer(PointerEvent::Move(
                    PointerMove::new(
                        pointer,
                        PointerButtons::only(PointerButton::PRIMARY),
                        sample(y),
                    ),
                )));
            }
            host.dispatch(PlatformInput::Pointer(PointerEvent::Up(
                PointerRelease::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample(100.0),
                ),
            )));
            assert_eq!(inner.pixels(), 670.0, "actual child release premise");
            assert_eq!(outer.pixels(), 600.0, "parent has not consumed the drag");
            for _ in 0..15 {
                let _ = host.pump(Duration::from_millis(16));
            }
            assert_eq!(inner.pixels(), 800.0, "child reaches its hard edge");
            assert!(
                outer.pixels() > 600.0,
                "density_sensitive={density_sensitive}: the real DPR2 parent's policy accepts the impulse, got {}",
                outer.pixels()
            );
        }
    }
}

pub(crate) fn nested_fling_same_controller_rebuild_preserves_accepted_handoff() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, axis, axis, false, false),
            tight(300.0, 300.0),
            vsync.clone(),
        );
        for rebuild in [false, true, false] {
            outer.jump_to(600.0).expect("no build is running");
            inner.jump_to(650.0).expect("no build is running");
            laid.tick();
            release_inner_fling(&laid, axis, false);
            assert_eq!(inner.pixels(), 670.0, "actual post-threshold drag");
            assert_eq!(outer.pixels(), 600.0, "parent has not consumed the drag");
            if rebuild {
                laid.pump_widget(nested_fling_content(
                    &outer, &inner, &vsync, axis, axis, false, false,
                ));
            }
            for _ in 0..15 {
                laid.pump_for(Duration::from_millis(16));
            }
            assert_eq!(inner.pixels(), 800.0, "child reaches its actual extent");
            assert!(
                outer.pixels() > 600.0,
                "{axis:?}, rebuild={rebuild}: same controllers retain accepted handoff, got {}",
                outer.pixels()
            );
        }
    }
}

pub(crate) fn nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for (outer_axis, reversed, bouncing, transfers) in [
        (Vertical, true, false, true),
        (Horizontal, false, false, false),
        (Vertical, false, true, false),
    ] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(
                &outer, &inner, &vsync, outer_axis, Vertical, reversed, bouncing,
            ),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        release_inner_fling(&laid, Vertical, reversed);
        assert_eq!(
            inner.pixels(),
            670.0,
            "premise: real child drag moves toward its end after recognition"
        );
        for _ in 0..500 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(inner.pixels(), 800.0, "child settles at its actual extent");
        if transfers {
            assert!(
                outer.pixels() < 600.0,
                "reversed child's physical impulse decreases normal parent offset"
            );
        } else {
            assert_eq!(
                outer.pixels(),
                600.0,
                "orthogonal parent or child bounce never receives a fling"
            );
        }
        assert!(!outer.position().is_scrolling());
        assert!(!inner.position().is_scrolling());
    }
}

pub(crate) fn replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers() {
    use flui_foundation::geometry::Axis::Vertical;
    let (old, new, inner, vsync) = (
        ScrollController::new(),
        ScrollController::with_initial_scroll_offset(600.0),
        ScrollController::new(),
        Vsync::new(),
    );
    let mut laid = crate::common::lay_out_animated(
        nested_fling_content(&old, &inner, &vsync, Vertical, Vertical, false, false),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    old.jump_to(600.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(inner.pixels(), 670.0);
    laid.pump_widget(nested_fling_content(
        &new, &inner, &vsync, Vertical, Vertical, false, false,
    ));
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        old.pixels(),
        600.0,
        "retired parent cannot receive a later edge impulse"
    );
    assert_eq!(
        new.pixels(),
        600.0,
        "new owner cannot inherit an old accepted handoff"
    );
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        new.pixels() > 600.0,
        "new gesture resolves the current parent owner"
    );
    let retired = new.pixels();
    laid.pump_widget(SizedBox::new(300.0, 300.0));
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        new.pixels(),
        retired,
        "unmount retires the transferred trajectory"
    );
}

pub(crate) fn nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress() {
    use flui_foundation::geometry::Axis::Vertical;
    use std::sync::atomic::{AtomicBool, Ordering};
    for competing in [false, true] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, Vertical, Vertical, false, false),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        let listenable = outer.listenable();
        let first = Arc::new(AtomicBool::new(true));
        let fail = Arc::clone(&first);
        let first_id = listenable.add_listener(std::rc::Rc::new(move || {
            assert!(
                !fail.swap(false, Ordering::SeqCst),
                "first parent handoff notification"
            );
        }));
        let second = Arc::new(AtomicBool::new(competing));
        let fail = Arc::clone(&second);
        let second_id = listenable.add_listener(std::rc::Rc::new(move || {
            assert!(
                !fail.swap(false, Ordering::SeqCst),
                "second parent handoff notification"
            );
        }));
        release_inner_fling(&laid, Vertical, false);
        let ((), log) = flui_testing::log_capture::capture(|| {
            for _ in 0..15 {
                laid.pump_for(Duration::from_millis(16));
            }
        });
        assert_contained_sampling_failure(&log, "first parent handoff notification");
        let expected = if competing {
            vec![
                "first parent handoff notification",
                "second parent handoff notification",
            ]
        } else {
            vec!["first parent handoff notification"]
        };
        assert_notification_failures(&log, &expected);
        assert!(!first.load(Ordering::SeqCst));
        assert!(
            !second.load(Ordering::SeqCst),
            "competing notification cleanup completed"
        );
        assert!(
            outer.pixels() > 600.0,
            "contained notification failures do not stall accepted motion"
        );
        listenable.remove_listener(first_id);
        listenable.remove_listener(second_id);
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            outer.pixels() > 600.0,
            "after containment a fresh gesture still hands off"
        );
    }
}

fn assert_contained_sampling_failure(log: &flui_testing::log_capture::CapturedLog, expected: &str) {
    let reports: Vec<_> = log
        .records()
        .iter()
        .filter(|record| record.message == "presentation sampling failure contained")
        .collect();
    assert_eq!(
        reports.len(),
        1,
        "one accepted operation reports once: {log}"
    );
    let report = reports[0];
    assert_eq!(
        report.field("first"),
        Some("SamplingFailure { stage: Animation, origin: Delivery }"),
        "the committed animation's delivery preserves provenance: {log}"
    );
    assert_eq!(report.field("blocking"), Some("None"), "{log}");
    assert_eq!(report.field("disposition"), Some("Contained"), "{log}");
    assert_eq!(report.field("admitted_attempt"), Some("true"), "{log}");
    assert!(
        report
            .field("panic_message")
            .is_some_and(|text| text.contains(expected)),
        "the first actual callback failure remains authoritative: {log}"
    );
}

fn assert_notification_failures(log: &flui_testing::log_capture::CapturedLog, expected: &[&str]) {
    let reported: Vec<_> = log
        .records()
        .iter()
        .filter(|record| record.target == "flui_foundation::notifier_generic")
        .filter_map(|record| record.field("panic_payload"))
        .collect();
    assert!(
        reported.starts_with(expected)
            && reported[expected.len()..]
                .iter()
                .all(|message| *message == expected[0]),
        "the actual notifier reports the first failure before competing failures: {log}"
    );
}

fn assert_scroll_failure(payload: &Box<dyn std::any::Any + Send>, expected: &str) {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .expect("the scroll boundary preserves the first failure text");
    assert!(text.contains(expected), "first failure: {text}");
}

pub(crate) fn nested_fling_equal_edge_jump_cancels_old_handoff_and_next_gesture_recovers() {
    use flui_foundation::geometry::Axis::Vertical;
    use std::sync::atomic::{AtomicBool, Ordering};
    let (outer, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let mut laid = crate::common::lay_out_animated(
        nested_fling_content(&outer, &inner, &vsync, Vertical, Vertical, false, false),
        tight(300.0, 300.0),
        vsync,
    );
    outer.jump_to(600.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    let pending_jump = Arc::new(AtomicBool::new(true));
    let jump = Arc::clone(&pending_jump);
    let controller = inner.clone();
    let listenable = inner.listenable();
    let listener = listenable.add_listener(std::rc::Rc::new(move || {
        if controller.pixels() == 800.0 && jump.swap(false, Ordering::SeqCst) {
            // Explicit programmatic cancellation must win even when it leaves
            // the already-committed edge pixels unchanged.
            controller.jump_to(800.0).expect("no build is running");
        }
    }));
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "actual release retains its accepted velocity"
    );
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        !pending_jump.load(Ordering::SeqCst),
        "edge notification actually reentered jump_to"
    );
    assert_eq!(inner.pixels(), 800.0);
    assert_eq!(
        outer.pixels(),
        600.0,
        "the explicit equal-edge jump cancels the older residual impulse"
    );
    listenable.remove_listener(listener);
    inner.jump_to(650.0).expect("no build is running");
    outer.jump_to(600.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        outer.pixels() > 600.0,
        "a fresh gesture can still hand off after cancellation"
    );
}

pub(crate) fn nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer() {
    let (outer, middle, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let leaf = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(200.0, 1000.0));
    let middle_view =
        Scrollable::new()
            .controller(middle.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, 800.0).boxed(),
                SizedBox::new(200.0, 200.0).child(leaf).boxed(),
            ]));
    let outer_view = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0).child(middle_view).boxed(),
            SizedBox::new(200.0, 4800.0).boxed(),
        ]));
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), outer_view),
        tight(200.0, 200.0),
        vsync,
    );
    outer.jump_to(600.0).expect("no build is running");
    middle.jump_to(800.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "leaf claimed the post-threshold move"
    );
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        middle.pixels(),
        800.0,
        "saturated intermediate ancestor stays clamped"
    );
    assert!(
        outer.pixels() > 600.0,
        "remaining impulse reaches the nearest willing ancestor"
    );

    outer.jump_to(600.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    let once = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let entered = Arc::clone(&once);
    let reentrant = outer.clone();
    let listenable = outer.listenable();
    let listener = listenable.add_listener(std::rc::Rc::new(move || {
        if entered.swap(false, std::sync::atomic::Ordering::SeqCst) {
            reentrant.jump_to(123.0).expect("no build is running");
        }
    }));
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        !once.load(std::sync::atomic::Ordering::SeqCst),
        "real transferred motion invoked the reentrant listener"
    );
    assert_eq!(
        outer.pixels(),
        123.0,
        "reentrant jump cancels the accepted trajectory"
    );
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        outer.pixels(),
        123.0,
        "no older handoff overwrites the new position"
    );
    listenable.remove_listener(listener);
    outer.jump_to(600.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        outer.pixels() > 600.0,
        "fresh handoff recovers after reentry"
    );
}

fn reveal_target_content(axis: flui_foundation::geometry::Axis) -> flui_view::BoxedView {
    reveal_target_content_at(axis, 600.0)
}

pub(crate) fn nested_fling_bouncing_parent_at_extent_absorbs_before_grandparent() {
    use flui_foundation::geometry::Axis::Vertical;
    let (outer, middle, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let leaf = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(200.0, 1000.0));
    let middle_view = Scrollable::new()
        .controller(middle.clone())
        .physics(Rc::new(BouncingScrollPhysics::new()))
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 800.0).boxed(),
            SizedBox::new(200.0, 200.0).child(leaf).boxed(),
        ]));
    let outer_view = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0).child(middle_view).boxed(),
            SizedBox::new(200.0, 4800.0).boxed(),
        ]));
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), outer_view),
        tight(200.0, 200.0),
        vsync,
    );
    outer.jump_to(600.0).expect("no build is running");
    middle.jump_to(800.0).expect("no build is running");
    inner.jump_to(650.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        middle.position().max_scroll_extent(),
        800.0,
        "parent starts at its outward edge"
    );
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "actual leaf claimed the post-threshold move"
    );
    let mut parent_overscrolled = false;
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
        parent_overscrolled |= middle.pixels() > 800.0;
        assert_eq!(
            outer.pixels(),
            600.0,
            "a willing bouncing parent absorbs before grandparent"
        );
    }
    assert!(
        parent_overscrolled,
        "actual parent ballistic physics received the boundary impulse"
    );
    assert_eq!(inner.pixels(), 800.0);
    for _ in 0..500 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        middle.pixels(),
        800.0,
        "absorbed impulse springs back locally"
    );
    assert_eq!(outer.pixels(), 600.0);
    inner.jump_to(650.0).expect("no build is running");
    middle.jump_to(800.0).expect("no build is running");
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    let mut recovered = false;
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
        recovered |= middle.pixels() > 800.0;
    }
    assert!(
        recovered,
        "next real gesture still reaches the parent physics"
    );
    assert_eq!(outer.pixels(), 600.0);
}

fn reveal_target_content_at(
    axis: flui_foundation::geometry::Axis,
    before: f64,
) -> flui_view::BoxedView {
    use flui_foundation::geometry::Axis;
    let target = flui_widgets::Semantics::new()
        .container(true)
        .role(SemanticsRole::Button)
        .label("reveal target")
        .child(SizedBox::new(40.0, 40.0))
        .boxed();
    match axis {
        Axis::Vertical => flui_widgets::Column::new(vec![
            SizedBox::new(200.0, before).boxed(),
            target,
            SizedBox::new(200.0, 600.0).boxed(),
        ])
        .boxed(),
        Axis::Horizontal => flui_widgets::Row::new(vec![
            SizedBox::new(before, 200.0).boxed(),
            target,
            SizedBox::new(600.0, 200.0).boxed(),
        ])
        .boxed(),
    }
}

fn request_reveal_target(laid: &LaidOut) {
    use flui_testing::a11y::{Action, ActionRequest, TreeId};
    let tree = laid.a11y_tree().expect("actual published semantics");
    let target = tree
        .find_by_label("reveal target")
        .expect("offscreen target remains addressable");
    let advertised = target.supports_action(Action::ScrollIntoView);
    assert!(
        !target.raw().is_hidden(),
        "an offered offscreen target must survive native hidden-subtree filtering"
    );
    laid.invoke_semantics_action(ActionRequest {
        action: Action::ScrollIntoView,
        target_tree: TreeId::ROOT,
        target_node: target.id(),
        data: None,
    })
    .expect("actual ScrollIntoView request resolves through ancestor scrollables");
    assert!(
        advertised,
        "automatic reveal must also be offered to the native adapter"
    );
}

fn assert_reveal_label_fixture_is_visible(axis: flui_foundation::geometry::Axis) {
    let mut visible = lay_out(
        Scrollable::new()
            .scroll_direction(axis)
            .child(reveal_target_content_at(axis, 0.0)),
        tight(200.0, 200.0),
    );
    visible.enable_semantics();
    visible.tick();
    assert_reveal_target_visible(&visible);
}

fn assert_reveal_target_visible(laid: &LaidOut) {
    let tree = laid.a11y_tree().expect("republished semantics");
    let rect = tree
        .find_by_label("reveal target")
        .expect("same target")
        .bounds()
        .expect("actual target geometry");
    assert!(
        rect.x0 >= -1e-9 && rect.y0 >= -1e-9 && rect.x1 <= 200.0 + 1e-9 && rect.y1 <= 200.0 + 1e-9,
        "requested node is visibly inside the receiving viewport: {rect:?}"
    );
}

pub(crate) fn show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        assert_reveal_label_fixture_is_visible(axis);
        for reversed in [false, true] {
            let scroll = ScrollController::new();
            let mut view = Scrollable::new()
                .controller(scroll.clone())
                .scroll_direction(axis)
                .child(reveal_target_content(axis));
            if reversed {
                view = view
                    .axis_direction(match axis {
                        Vertical => flui_rendering::constraints::AxisDirection::BottomToTop,
                        Horizontal => flui_rendering::constraints::AxisDirection::RightToLeft,
                    })
                    .viewport_builder(Rc::new(move |position| {
                        flui_widgets::SingleChildScrollView::new()
                            .scroll_direction(axis)
                            .reverse(true)
                            .position(position)
                            .child(reveal_target_content(axis))
                            .boxed()
                    }));
            }
            let mut laid = lay_out(view, tight(200.0, 200.0));
            laid.enable_semantics();
            laid.tick();
            let before = laid.a11y_tree().expect("tree");
            let rect = before
                .find_by_label("reveal target")
                .expect("offscreen node")
                .bounds()
                .expect("node bounds");
            assert!(
                rect.x0 < 0.0 || rect.y0 < 0.0 || rect.x1 > 200.0 || rect.y1 > 200.0,
                "premise: target is offscreen {axis:?}/{reversed}: {rect:?}"
            );
            request_reveal_target(&laid);
            laid.tick();
            assert!(
                scroll.pixels() > 0.0,
                "real viewport moved toward its offscreen target"
            );
            assert_reveal_target_visible(&laid);
            let revealed = scroll.pixels();
            request_reveal_target(&laid);
            laid.tick();
            assert_eq!(
                scroll.pixels(),
                revealed,
                "already-visible reveal is idempotent"
            );
            scroll.jump_to(0.0).expect("no build is running");
            laid.tick();
            request_reveal_target(&laid);
            laid.tick();
            assert_reveal_target_visible(&laid);
        }
    }
}

pub(crate) fn show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for inner_axis in [Vertical, Horizontal] {
        assert_reveal_label_fixture_is_visible(inner_axis);
        let (outer, inner, replacement) = (
            ScrollController::new(),
            ScrollController::new(),
            ScrollController::new(),
        );
        let content = |controller: &ScrollController| {
            Scrollable::new()
                .controller(outer.clone())
                .child(flui_widgets::Column::new(vec![
                    SizedBox::new(200.0, 600.0).boxed(),
                    SizedBox::new(200.0, 200.0)
                        .child(
                            Scrollable::new()
                                .controller(controller.clone())
                                .scroll_direction(inner_axis)
                                .child(reveal_target_content(inner_axis)),
                        )
                        .boxed(),
                    SizedBox::new(200.0, 800.0).boxed(),
                ]))
        };
        let mut laid = lay_out(content(&inner), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(
            inner.pixels(),
            440.0,
            "inner reveals its nearest target edge"
        );
        assert_eq!(
            outer.pixels(),
            600.0,
            "outer reveals the resulting inner viewport"
        );
        assert_reveal_target_visible(&laid);
        outer.jump_to(0.0).expect("no build is running");
        laid.pump_widget(content(&replacement));
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(
            inner.pixels(),
            440.0,
            "retired controller is not driven by a new request"
        );
        assert_eq!(
            replacement.pixels(),
            440.0,
            "replacement gets its real target geometry"
        );
        assert_eq!(outer.pixels(), 600.0);
        assert_reveal_target_visible(&laid);
    }
}

fn nested_reveal_content(outer: &ScrollController, inner: &ScrollController) -> Scrollable {
    Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0)
                .child(
                    Scrollable::new()
                        .controller(inner.clone())
                        .child(reveal_target_content(
                            flui_foundation::geometry::Axis::Vertical,
                        )),
                )
                .boxed(),
            SizedBox::new(200.0, 800.0).boxed(),
        ]))
}

pub(crate) fn show_on_screen_failure_continues_live_ancestors_and_fresh_requests_recover() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for competing in [false, true] {
        let (outer, inner) = (ScrollController::new(), ScrollController::new());
        let mut laid = lay_out(nested_reveal_content(&outer, &inner), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        let layout_notifications = Rc::new(Cell::new(0));
        let observed = layout_notifications.clone();
        flui_rendering::view::ViewportOffset::add_listener(
            &inner.position(),
            Rc::new(move || {
                observed.set(observed.get() + 1);
            }),
        );
        let first = Arc::new(AtomicBool::new(true));
        let failed = Arc::clone(&first);
        let inner_listenable = inner.listenable();
        let first_id = inner_listenable.add_listener(std::rc::Rc::new(move || {
            assert!(
                !failed.swap(false, Ordering::SeqCst),
                "first inner reveal notification"
            );
        }));
        let second = Arc::new(AtomicBool::new(competing));
        let failed = Arc::clone(&second);
        let outer_listenable = outer.listenable();
        let second_id = outer_listenable.add_listener(std::rc::Rc::new(move || {
            assert!(
                !failed.swap(false, Ordering::SeqCst),
                "second outer reveal notification"
            );
        }));
        let (failure, log) = flui_testing::log_capture::capture(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                request_reveal_target(&laid);
            }))
            .expect_err("reveal reports its first failure after delivering live ancestors")
        });
        assert_scroll_failure(&failure, "first inner reveal notification");
        assert_eq!(
            layout_notifications.get(),
            1,
            "an accepted offset still reaches the render listener after a value listener failure"
        );
        let expected = if competing {
            vec![
                "first inner reveal notification",
                "second outer reveal notification",
            ]
        } else {
            vec!["first inner reveal notification"]
        };
        assert_notification_failures(&log, &expected);
        assert!(!first.load(Ordering::SeqCst));
        assert!(
            !second.load(Ordering::SeqCst),
            "live ancestor delivery completed"
        );
        assert_eq!(inner.pixels(), 440.0);
        assert_eq!(
            outer.pixels(),
            600.0,
            "inner failure cannot starve the enclosing viewport"
        );
        inner_listenable.remove_listener(first_id);
        outer_listenable.remove_listener(second_id);
        inner.jump_to(0.0).expect("no build is running");
        outer.jump_to(0.0).expect("no build is running");
        laid.tick();
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(inner.pixels(), 440.0);
        assert_eq!(outer.pixels(), 600.0);
        assert_reveal_target_visible(&laid);
    }
}

pub(crate) fn show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers() {
    use flui_rendering::semantics::{
        AccessibilityNodeId, ActionCall, SemanticsActionInvocation, SemanticsActionRequest,
    };
    use std::cell::RefCell;
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static PENDING_REVEAL: RefCell<Option<SemanticsActionInvocation>> = const { RefCell::new(None) };
    }
    struct RestoreScope(Option<SemanticsActionInvocation>);
    impl Drop for RestoreScope {
        fn drop(&mut self) {
            let outgoing = PENDING_REVEAL.with(|slot| slot.replace(self.0.take()));
            drop(outgoing);
        }
    }
    let (outer, inner) = (ScrollController::new(), ScrollController::new());
    let mut laid = lay_out(nested_reveal_content(&outer, &inner), tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("actual tree");
    let target = tree
        .find_by_label("reveal target")
        .expect("addressable target")
        .id();
    let invocation = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(target.0)
                    .expect("published nonzero identity"),
                call: ActionCall::ScrollIntoView(None),
            })
        })
        .expect("actual automatic ancestor reveal");
    let _scope = RestoreScope(PENDING_REVEAL.with(|slot| slot.replace(Some(invocation))));
    let listenable = inner.listenable();
    let listener = listenable.add_listener(std::rc::Rc::new(|| {
        // The notification runs on the same owner thread. Resolve its local
        // pending request without making that request cross-thread ownership.
        let invocation = PENDING_REVEAL.with(|slot| slot.borrow_mut().take());
        if let Some(invocation) = invocation {
            invocation.invoke();
        }
    }));
    request_reveal_target(&laid);
    laid.tick();
    assert!(
        PENDING_REVEAL.with(|slot| slot.borrow().is_none()),
        "same-pipeline request actually reentered"
    );
    assert_eq!(
        inner.pixels(),
        440.0,
        "reentry cannot apply old reveal geometry twice"
    );
    assert_eq!(outer.pixels(), 600.0);
    assert_reveal_target_visible(&laid);
    listenable.remove_listener(listener);
    inner.jump_to(0.0).expect("no build is running");
    outer.jump_to(0.0).expect("no build is running");
    laid.tick();
    request_reveal_target(&laid);
    laid.tick();
    assert_eq!(inner.pixels(), 440.0);
    assert_eq!(outer.pixels(), 600.0);
    assert_reveal_target_visible(&laid);
}

pub(crate) fn show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion() {
    use flui_rendering::semantics::{
        AccessibilityNodeId, ActionCall, SemanticsActionInvocation, SemanticsActionRequest,
    };
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static PENDING_SIBLING: RefCell<Option<SemanticsActionInvocation>> = const { RefCell::new(None) };
    }
    struct RestoreScope(Option<SemanticsActionInvocation>);
    impl Drop for RestoreScope {
        fn drop(&mut self) {
            let outgoing = PENDING_SIBLING.with(|slot| slot.replace(self.0.take()));
            drop(outgoing);
        }
    }
    let content = |controller: &ScrollController, before, gap| {
        let target = |label| {
            flui_widgets::Semantics::new()
                .container(true)
                .role(SemanticsRole::Button)
                .label(label)
                .child(SizedBox::new(40.0, 40.0))
                .boxed()
        };
        Scrollable::new()
            .controller(controller.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, before).boxed(),
                target("first sibling"),
                SizedBox::new(200.0, gap).boxed(),
                target("second sibling"),
                SizedBox::new(200.0, 800.0).boxed(),
            ]))
    };
    // Both labelled nodes from the same constructor are visible and native
    // addressable when their actual layout positions fit the viewport.
    let mut visible = lay_out(
        content(&ScrollController::new(), 0.0, 40.0),
        tight(200.0, 200.0),
    );
    visible.enable_semantics();
    visible.tick();
    for label in ["first sibling", "second sibling"] {
        let tree = visible.a11y_tree().expect("visible control tree");
        let target = tree.find_by_label(label).expect("visible labelled sibling");
        assert!(!target.raw().is_hidden());
        assert!(target.supports_action(flui_testing::a11y::Action::ScrollIntoView));
        let bounds = target.bounds().expect("actual visible geometry");
        assert!(bounds.y0 >= 0.0 && bounds.y1 <= 200.0);
    }
    for first_is_nearer in [true, false] {
        let controller = ScrollController::new();
        let mut laid = lay_out(content(&controller, 600.0, 360.0), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        let (first_label, last_label, expected) = if first_is_nearer {
            ("first sibling", "second sibling", 840.0)
        } else {
            ("second sibling", "first sibling", 600.0)
        };
        let tree = laid.a11y_tree().expect("actual offscreen tree");
        let first = tree
            .find_by_label(first_label)
            .expect("first addressable sibling")
            .id();
        let last = tree
            .find_by_label(last_label)
            .expect("last addressable sibling")
            .id();
        let invocation = laid
            .pipeline_owner()
            .with(|owner| {
                owner.resolve_semantics_action(SemanticsActionRequest::new(
                    AccessibilityNodeId::from_u64(last.0).expect("published identity"),
                    ActionCall::ScrollIntoView(None),
                ))
            })
            .expect("actual pending sibling reveal");
        let _scope = RestoreScope(PENDING_SIBLING.with(|slot| slot.replace(Some(invocation))));
        let notifications = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&notifications);
        let listenable = controller.listenable();
        let listener = listenable.add_listener(std::rc::Rc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            let invocation = PENDING_SIBLING.with(|slot| slot.borrow_mut().take());
            if let Some(invocation) = invocation {
                invocation.invoke();
            }
        }));
        laid.invoke_semantics_action(flui_testing::a11y::ActionRequest {
            action: flui_testing::a11y::Action::ScrollIntoView,
            target_tree: flui_testing::a11y::TreeId::ROOT,
            target_node: first,
            data: None,
        })
        .expect("first accepted native reveal");
        laid.tick();
        assert!(
            PENDING_SIBLING.with(|slot| slot.borrow().is_none()),
            "sibling request actually reentered"
        );
        assert!(
            notifications.load(Ordering::SeqCst) >= 2,
            "both accepted targets caused actual movement"
        );
        assert_eq!(
            controller.pixels(),
            expected,
            "last sibling wins without reapplying cached geometry: {first_label} then {last_label}"
        );
        let current = laid.a11y_tree().expect("republished geometry");
        let bounds = current
            .find_by_label(last_label)
            .expect("last target remains present")
            .bounds()
            .expect("actual last target geometry");
        assert!(
            bounds.y0 >= -1e-9 && bounds.y1 <= 200.0 + 1e-9,
            "last accepted target is visible: {bounds:?}"
        );
        listenable.remove_listener(listener);
        controller.jump_to(0.0).expect("no build is running");
        laid.tick();
        let current = laid.a11y_tree().expect("fresh geometry");
        let target = current
            .find_by_label(last_label)
            .expect("fresh target")
            .id();
        laid.invoke_semantics_action(flui_testing::a11y::ActionRequest {
            action: flui_testing::a11y::Action::ScrollIntoView,
            target_tree: flui_testing::a11y::TreeId::ROOT,
            target_node: target,
            data: None,
        })
        .expect("fresh request remains deliverable");
        laid.tick();
        assert_eq!(
            controller.pixels(),
            if first_is_nearer { 840.0 } else { 440.0 }
        );
    }
}

pub(crate) fn nested_fling_custom_physics_failure_and_retirement_preserve_first_and_recover() {
    use flui_animation::Simulation;
    use flui_foundation::geometry::Axis::Vertical;
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct RetiringSimulation {
        motion: Box<dyn Simulation>,
        fail_drop: Arc<AtomicBool>,
    }
    impl Simulation for RetiringSimulation {
        fn x(&self, time: f64) -> f64 {
            self.motion.x(time)
        }
        fn dx(&self, time: f64) -> f64 {
            self.motion.dx(time)
        }
        fn is_done(&self, time: f64) -> bool {
            self.motion.is_done(time)
        }
    }
    impl Drop for RetiringSimulation {
        fn drop(&mut self) {
            assert!(
                !self.fail_drop.swap(false, Ordering::SeqCst),
                "custom parent simulation retirement failure"
            );
        }
    }
    #[derive(Debug)]
    struct FaultPhysics {
        fail_boundary: Arc<AtomicBool>,
        fail_drop: Arc<AtomicBool>,
    }
    impl ScrollPhysics for FaultPhysics {
        fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed: f64) -> f64 {
            ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed)
        }
        fn create_ballistic_simulation(
            &self,
            metrics: &ScrollMetrics,
            velocity: f64,
        ) -> Option<Box<dyn Simulation>> {
            Some(Box::new(RetiringSimulation {
                motion: ClampingScrollPhysics::new()
                    .create_ballistic_simulation(metrics, velocity)?,
                fail_drop: Arc::clone(&self.fail_drop),
            }))
        }
        fn boundary_velocity(&self, metrics: &ScrollMetrics, velocity: f64) -> Option<f64> {
            assert!(
                !self.fail_boundary.swap(false, Ordering::SeqCst),
                "custom parent boundary failure"
            );
            ClampingScrollPhysics::new().boundary_velocity(metrics, velocity)
        }
    }

    for (boundary_fault, retirement_fault) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let fail_boundary = Arc::new(AtomicBool::new(boundary_fault));
        let fail_drop = Arc::new(AtomicBool::new(retirement_fault));
        let parent = Scrollable::new()
            .controller(outer.clone())
            .physics(Rc::new(FaultPhysics {
                fail_boundary: Arc::clone(&fail_boundary),
                fail_drop: Arc::clone(&fail_drop),
            }))
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, 600.0).boxed(),
                SizedBox::new(200.0, 200.0)
                    .child(
                        Scrollable::new()
                            .controller(inner.clone())
                            .child(SizedBox::new(200.0, 1000.0)),
                    )
                    .boxed(),
                SizedBox::new(200.0, 4800.0).boxed(),
            ]));
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), parent),
            tight(200.0, 200.0),
            vsync,
        );
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        assert_eq!(inner.pixels(), 670.0);
        let ((), log) = flui_testing::log_capture::capture(|| {
            for _ in 0..500 {
                laid.pump_for(Duration::from_millis(16));
                let fault_pending = if boundary_fault {
                    fail_boundary.load(Ordering::SeqCst)
                } else {
                    fail_drop.load(Ordering::SeqCst)
                };
                if (boundary_fault || retirement_fault) && !fault_pending {
                    break;
                }
            }
        });
        if boundary_fault || retirement_fault {
            let expected = if boundary_fault {
                "custom parent boundary failure"
            } else {
                "custom parent simulation retirement failure"
            };
            assert_contained_sampling_failure(&log, expected);
            assert_eq!(
                fail_drop.load(Ordering::SeqCst),
                boundary_fault && retirement_fault,
                "a failed boundary prevents creation of the parent simulation"
            );
        } else {
            assert!(
                log.records()
                    .iter()
                    .all(|record| record.message != "presentation sampling failure contained"),
                "healthy custom physics completes without a failure report: {log}"
            );
            assert!(
                outer.pixels() > 600.0,
                "actual custom parent receives the impulse"
            );
        }
        assert!(!fail_boundary.load(Ordering::SeqCst));
        // A failed boundary can leave the uncreated simulation's fault armed.
        // Disable it before checking the next gesture.
        fail_drop.store(false, Ordering::SeqCst);
        inner.jump_to(650.0).expect("no build is running");
        outer.jump_to(600.0).expect("no build is running");
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            outer.pixels() > 600.0,
            "fresh transfer makes progress after containment"
        );
    }
}

/// A command queued before the scrollable mounted starts its run when the
/// scrollable applies its view, and the activity listeners that run notifies
/// may write signals: that is outside `build`, where the runtime refuses a
/// write.
pub(crate) fn a_command_queued_before_mount_is_serviced_outside_build() {
    let mut laid = lay_out(SizedBox::square(1.0), tight(300.0, 300.0));
    let vsync = Vsync::new();
    laid.adopt_vsync(vsync.clone()).expect("clock admission");
    let reactive = laid.with_build_owner_mut(|owner| owner.signal_graph().clone());
    let signal = reactive.signal(0u32);
    let controller = ScrollController::new();
    // Room to scroll, so the command moves something.
    controller
        .update_dimensions(300.0, 0.0, 500.0)
        .expect("ordered extents");
    let outcome = Rc::new(Cell::new(None));
    let seen = Rc::clone(&outcome);
    let writer = laid.with_build_owner_mut(|owner| owner.writer_source());
    let _listener = controller
        .position()
        .add_activity_listener(Rc::new(move || {
            seen.set(Some(writer.write(|cx| signal.set(cx, 1))));
        }));
    controller
        .animate_to(
            100.0,
            Duration::from_secs(1),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    assert_eq!(outcome.get(), None, "nothing serves the command yet");

    laid.pump_widget(VsyncScope::new(
        vsync,
        Scrollable::new()
            .controller(controller)
            .child(SizedBox::new(300.0, 800.0)),
    ));

    assert_eq!(
        outcome.get(),
        Some(Ok(())),
        "the run started and its listener wrote a signal"
    );
}

/// A widget whose `build` drives a scroll controller, recording what the
/// controller answered.
#[derive(Clone, flui_view::prelude::StatefulView)]
struct ScrollInBuild {
    scroll: ScrollController,
    drive: bool,
    outcome: Rc<std::cell::RefCell<Vec<Result<(), flui_foundation::MutatedDuringBuild>>>>,
}

struct ScrollInBuildState;

impl flui_view::StatefulView for ScrollInBuild {
    type State = ScrollInBuildState;
    fn create_state(&self) -> Self::State {
        ScrollInBuildState
    }
}

impl flui_view::ViewState<ScrollInBuild> for ScrollInBuildState {
    fn build(&self, view: &ScrollInBuild, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        if view.drive {
            let mut outcome = view.outcome.borrow_mut();
            outcome.push(view.scroll.jump_to(200.0));
            outcome.push(view.scroll.animate_to(
                300.0,
                Duration::from_millis(100),
                SharedCurve::new(curves::LINEAR),
            ));
        }
        SizedBox::square(1.0)
    }
}

/// `jump_to` and `animate_to` called from `build` are refused with a typed
/// error and move nothing; the same calls from outside a build land
/// (ADR-0086 §7).
pub(crate) fn controller_mutation_during_build_is_refused() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let outcome = Rc::new(std::cell::RefCell::new(Vec::new()));
    let tree = |drive: bool| {
        flui_widgets::Column::new((
            SizedBox::new(300.0, 300.0).child(animated_scroll_content(&scroll, &vsync)),
            ScrollInBuild {
                scroll: scroll.clone(),
                drive,
                outcome: Rc::clone(&outcome),
            },
        ))
    };
    let mut laid = lay_out(tree(false), tight(300.0, 600.0));
    scroll
        .jump_to(10.0)
        .expect("a jump outside build is accepted");
    assert_eq!(scroll.pixels(), 10.0);

    laid.pump_widget(tree(true));
    advance_scroll_run(&mut laid);

    let refused = flui_foundation::MutatedDuringBuild {
        controller: "ScrollController",
    };
    assert_eq!(*outcome.borrow(), vec![Err(refused), Err(refused)]);
    assert_eq!(scroll.pixels(), 10.0, "a refused mutation moves nothing");
}
