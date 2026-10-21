//! `InkWell`'s state layer and ripple, read from the recorded display list on
//! virtual time: the M3 state-layer alpha, the wave's radius and fade timing,
//! clipping to the well, reduced motion and a disabled configuration.

use std::time::Duration;

use flui_material::{InkWell, RippleConfiguration, RippleScope, StateLayerAlpha, Theme, ThemeData};
use flui_sdk::animation::MotionOverride;
use flui_sdk::geometry::{Point, Rect};
use flui_sdk::painting::{Alignment, Color, DrawOp};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    GestureDetector, OverflowBox, Row, SizedBox, WidgetState, WidgetStatesController,
};

use crate::common::{LaidOut, lay_out, tight};

const WIDTH: f64 = 100.0;
const HEIGHT: f64 = 40.0;
/// The tint every row configures; its alpha is 1.
const TINT: Color = Color::RED;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn well() -> InkWell {
    InkWell::new(SizedBox::new(WIDTH, HEIGHT)).on_tap(|_| {})
}

fn tinted(child: impl IntoView) -> RippleScope {
    RippleScope::new(
        Some(RippleConfiguration {
            color: Some(TINT),
            alpha: StateLayerAlpha::M3,
        }),
        child,
    )
}

fn mount(view: impl View) -> LaidOut {
    let mut laid = lay_out(view, tight(WIDTH, HEIGHT));
    laid.pump_for(ms(16));
    laid
}

/// One painted piece of ink, in root space, with the clip it was drawn under.
#[derive(Debug)]
enum Ink {
    Layer {
        alpha: f64,
        visible: Option<Rect>,
    },
    Wave {
        center: Point,
        radius: f64,
        alpha: f64,
        visible: Option<Rect>,
    },
}

/// Every rect and circle painted in the tint, with the part its clip lets
/// through: the clip stack follows `Save`/`Restore` and each clip's bounds.
fn ink(laid: &LaidOut) -> Vec<Ink> {
    let mut clips: Vec<Option<Rect>> = vec![None];
    let mut painted = Vec::new();
    let clip_to = |current: Option<Rect>, bounds: Rect| match current {
        Some(clip) => Some(
            clip.intersect(&bounds)
                .unwrap_or(Rect::from_ltwh(0.0, 0.0, 0.0, 0.0)),
        ),
        None => Some(bounds),
    };
    for command in laid.draw_ops() {
        let current = *clips
            .last()
            .expect("BUG: the root clip level is never popped");
        let to_root = |rect: Rect| command.transform.transform_rect(&rect);
        let tinted = |color: Color| color.with_opacity(1.0) == TINT;
        match &command.op {
            DrawOp::Save | DrawOp::SaveLayer { .. } => clips.push(current),
            DrawOp::Restore | DrawOp::RestoreLayer => {
                if clips.len() > 1 {
                    clips.pop();
                }
            }
            DrawOp::ClipRect { rect, .. } => {
                *clips.last_mut().expect("BUG: a clip level exists") =
                    clip_to(current, to_root(*rect));
            }
            DrawOp::ClipRRect { rrect, .. } => {
                let bounds = Rect::from_ltwh(
                    rrect.left(),
                    rrect.top(),
                    rrect.right() - rrect.left(),
                    rrect.bottom() - rrect.top(),
                );
                *clips.last_mut().expect("BUG: a clip level exists") =
                    clip_to(current, to_root(bounds));
            }
            DrawOp::Rect { rect, paint } if tinted(paint.color) => painted.push(Ink::Layer {
                alpha: f64::from(paint.color.opacity()),
                visible: clip_to(current, to_root(*rect)),
            }),
            DrawOp::Circle {
                center,
                radius,
                paint,
            } if tinted(paint.color) => {
                let (x, y) = command.transform.transform_point(center.x, center.y);
                let bounds = Rect::from_ltwh(x - radius, y - radius, radius * 2.0, radius * 2.0);
                painted.push(Ink::Wave {
                    center: Point::new(x, y),
                    radius: *radius,
                    alpha: f64::from(paint.color.opacity()),
                    visible: clip_to(current, bounds),
                });
            }
            _ => {}
        }
    }
    painted
}

fn waves(laid: &LaidOut) -> Vec<(Point, f64, f64)> {
    ink(laid)
        .into_iter()
        .filter_map(|ink| match ink {
            Ink::Wave {
                center,
                radius,
                alpha,
                ..
            } => Some((center, radius, alpha)),
            Ink::Layer { .. } => None,
        })
        .collect()
}

fn layers(laid: &LaidOut) -> Vec<f64> {
    ink(laid)
        .into_iter()
        .filter_map(|ink| match ink {
            Ink::Layer { alpha, .. } => Some(alpha),
            Ink::Wave { .. } => None,
        })
        .collect()
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 2e-3,
        "{what}: expected {expected}, painted {actual}"
    );
}

/// A settled hover paints the shape at the hover token times the tint's
/// alpha, and no wave.
pub fn a_settled_hover_paints_the_hover_state_layer() {
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_hover(50.0, 20.0);
    laid.pump_for(ms(16));
    laid.pump_for(ms(16));
    let layers = layers(&laid);
    assert_eq!(layers.len(), 1, "one state layer: {:?}", ink(&laid));
    assert_close(layers[0], 0.08, "hover state-layer alpha");
    assert!(waves(&laid).is_empty(), "hover grows no wave");
}

/// 100 ms into a press, the wave is centred on the contact with a radius
/// strictly between `0.3 × max(w, h)` and the farthest corner plus 10.
pub fn a_held_press_grows_a_wave_between_its_start_and_end_radius() {
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.tick();
    laid.pump_for(ms(50));
    laid.pump_for(ms(50));
    let waves = waves(&laid);
    assert_eq!(waves.len(), 1, "one wave: {:?}", ink(&laid));
    let (center, radius, _) = waves[0];
    assert_close(center.x, 10.0, "wave origin x");
    assert_close(center.y, 10.0, "wave origin y");
    let start = 0.3 * WIDTH;
    let end = (WIDTH - 10.0).hypot(HEIGHT - 10.0) + 10.0;
    assert!(
        start + 1.0 < radius && radius < end - 1.0,
        "radius {radius} must lie strictly between {start} and {end}"
    );
    laid.dispatch_pointer_up(10.0, 10.0);
}

/// A release 30 ms into the press waits for the 75 ms fade-in: at 75 ms the
/// wave is at the full pressed alpha, then it fades out over 150 ms.
pub fn a_quick_release_finishes_the_fade_in_before_fading_out() {
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(30));
    laid.dispatch_pointer_up(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(45));
    let at_fade_in = waves(&laid);
    assert_eq!(at_fade_in.len(), 1, "the wave is visible at 75 ms");
    assert_close(
        at_fade_in[0].2,
        0.10,
        "wave alpha when its fade-in completes",
    );
    assert!(
        layers(&laid).is_empty(),
        "while the wave runs it carries the press, not the state layer: {:?}",
        ink(&laid)
    );
    laid.pump_for(ms(75));
    let halfway = waves(&laid);
    assert_eq!(halfway.len(), 1, "the wave is fading at 150 ms");
    assert_close(
        halfway[0].2,
        0.05,
        "wave alpha halfway through its fade-out",
    );
    laid.pump_for(ms(80));
    assert!(
        waves(&laid).is_empty(),
        "the wave is gone after its fade-out"
    );
}

/// A cancelled contact fades out at once, from where its fade-in had got to.
pub fn a_cancelled_press_fades_out_at_once() {
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(30));
    let at_cancel = waves(&laid);
    assert_eq!(at_cancel.len(), 1, "the wave is fading in at 30 ms");
    let reached = at_cancel[0].2;
    laid.dispatch_pointer_cancel();
    laid.tick();
    laid.pump_for(ms(75));
    let fading = waves(&laid);
    assert_eq!(
        fading.len(),
        1,
        "the wave is fading out 75 ms after the cancel"
    );
    assert_close(
        fading[0].2,
        reached / 2.0,
        "wave alpha halfway through its fade-out",
    );
}

/// Reduced motion paints the pressed state layer and no expanding circle,
/// both while the contact is held and after the tap.
pub fn reduced_motion_paints_the_state_layer_without_a_wave() {
    let mut laid = mount(tinted(well()));
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(ms(16));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(16));
    assert!(waves(&laid).is_empty(), "no wave under a held press");
    let held = layers(&laid);
    assert_eq!(
        held.len(),
        1,
        "a held press shows the state layer: {:?}",
        ink(&laid)
    );
    assert_close(held[0], 0.10, "held pressed state-layer alpha");
    laid.dispatch_pointer_up(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(16));
    assert!(waves(&laid).is_empty(), "no wave after the tap");
    let layers = layers(&laid);
    assert_eq!(layers.len(), 1, "one state layer: {:?}", ink(&laid));
    assert_close(layers[0], 0.10, "pressed state-layer alpha");
}

/// A press near the shared edge of two adjacent wells paints nothing inside
/// the other well: the wave is clipped to its own well.
pub fn a_wave_never_paints_inside_the_adjacent_well() {
    let row = Row::new(vec![well().into_view().boxed(), well().into_view().boxed()]);
    let mut laid = lay_out(tinted(row), tight(WIDTH * 2.0, HEIGHT));
    laid.pump_for(ms(16));
    laid.dispatch_pointer_down(WIDTH - 5.0, HEIGHT / 2.0);
    laid.tick();
    laid.pump_for(ms(100));
    let painted = ink(&laid);
    let mut wave_seen = false;
    for ink in &painted {
        let visible = match ink {
            Ink::Wave { visible, .. } => {
                wave_seen = true;
                visible
            }
            Ink::Layer { visible, .. } => visible,
        };
        let visible = visible.expect("ink is drawn under its well's clip");
        assert!(
            visible.right() <= WIDTH + 1e-6,
            "ink reaches {} inside the adjacent well: {painted:?}",
            visible.right()
        );
    }
    assert!(wave_seen, "the pressed well paints its wave: {painted:?}");
    laid.dispatch_pointer_up(WIDTH - 5.0, HEIGHT / 2.0);
}

/// `Pressed` set without a contact (a shared states controller, an
/// assistive activation) has no wave to carry it, so the state layer shows
/// it.
pub fn a_press_without_a_wave_shows_the_pressed_state_layer() {
    let states = WidgetStatesController::default();
    let mut laid = mount(tinted(well().states_controller(states.clone())));
    states.update(WidgetState::Pressed, true);
    laid.tick();
    laid.pump_for(ms(16));
    laid.pump_for(ms(16));
    assert!(waves(&laid).is_empty(), "no contact, no wave");
    let layers = layers(&laid);
    assert_eq!(layers.len(), 1, "one state layer: {:?}", ink(&laid));
    assert_close(layers[0], 0.10, "pressed state-layer alpha");
}

/// A contact that moves past the touch slop stops being a tap: its wave
/// fades out at once from where its fade-in had got to, with no Up.
pub fn a_contact_dragged_past_the_slop_cancels_its_wave() {
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_down(20.0, 20.0);
    laid.tick();
    laid.pump_for(ms(30));
    let at_drag = waves(&laid);
    assert_eq!(at_drag.len(), 1, "the wave is fading in at 30 ms");
    let reached = at_drag[0].2;
    laid.dispatch_pointer_move(90.0, 20.0);
    laid.tick();
    laid.pump_for(ms(75));
    let fading = waves(&laid);
    assert_eq!(
        fading.len(),
        1,
        "the wave is fading out 75 ms after the drag"
    );
    assert_close(
        fading[0].2,
        reached / 2.0,
        "wave alpha halfway through its fade-out",
    );
    laid.dispatch_pointer_up(90.0, 20.0);
}

/// A well too large for its wave's extent to be finite publishes no
/// non-finite geometry.
pub fn a_huge_well_publishes_only_finite_ink() {
    let huge = 1.5e308;
    let mut laid = mount(tinted(
        OverflowBox::new()
            .with_alignment(Alignment::TOP_LEFT)
            .with_max_width(huge)
            .with_max_height(huge)
            .child(InkWell::new(SizedBox::new(huge, huge)).on_tap(|_| {})),
    ));
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.tick();
    laid.pump_for(ms(100));
    for command in laid.draw_ops() {
        match command.op {
            DrawOp::Circle { center, radius, .. } => assert!(
                radius.is_finite()
                    && [
                        center.x - radius,
                        center.x + radius,
                        center.y - radius,
                        center.y + radius
                    ]
                    .iter()
                    .all(|edge| edge.is_finite()),
                "a wave publishes non-finite geometry: centre {center:?}, radius {radius}"
            ),
            DrawOp::Rect { rect, .. } => assert!(
                [rect.left(), rect.top(), rect.right(), rect.bottom()]
                    .iter()
                    .all(|edge| edge.is_finite()),
                "a state layer publishes non-finite geometry: {rect:?}"
            ),
            _ => {}
        }
    }
    laid.dispatch_pointer_up(10.0, 10.0);
}

/// A scope with no configuration turns the ripple and the state layer off,
/// even where the theme supplies a feedback colour.
pub fn a_disabled_ripple_configuration_paints_no_feedback() {
    let mut laid = mount(Theme::new(
        ThemeData::light(),
        RippleScope::new(None, well()),
    ));
    laid.dispatch_pointer_hover(50.0, 20.0);
    laid.pump_for(ms(16));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(100));
    let feedback: Vec<_> = laid
        .draw_ops()
        .into_iter()
        .filter(|command| matches!(command.op, DrawOp::Rect { .. } | DrawOp::Circle { .. }))
        .collect();
    assert!(feedback.is_empty(), "no feedback is painted: {feedback:?}");
    laid.dispatch_pointer_up(50.0, 20.0);
}

/// A contact that loses the arena without moving (a long press around the
/// well wins it) stops being a tap: its wave fades out at once.
pub fn a_long_press_winning_the_arena_cancels_the_wave() {
    let mut laid = mount(tinted(
        GestureDetector::new().on_long_press(|_| {}).child(well()),
    ));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    // The long press holds the contact, so the well shows it as pressed at
    // the press timeout, and the wave paints a frame later.
    laid.pump_for(ms(300));
    laid.pump_for(ms(16));
    laid.pump_for(ms(16));
    assert_eq!(waves(&laid).len(), 1, "the press grows a wave");
    laid.pump_for(ms(300));
    laid.tick();
    laid.pump_for(ms(75));
    let fading = waves(&laid);
    assert_eq!(fading.len(), 1, "the wave fades after the long press wins");
    assert_close(fading[0].2, 0.05, "wave alpha halfway through its fade-out");
    laid.dispatch_pointer_up(50.0, 20.0);
}

/// The well shows the one contact its tap recognizer tracks: a second
/// contact that lands while it is pending grows no wave, and the tracked one
/// fades once it moves past the slop.
pub fn a_second_contact_grows_no_wave_beside_a_pending_tap() {
    use flui_interaction::events::{
        PointerId, PointerKind, make_down_event_for_id, make_move_event_for_id,
    };
    let contact = |id: u64| PointerId::new(std::num::NonZeroU64::new(id).expect("non-zero"));
    let at = |x: f64| flui_sdk::geometry::Offset::new(x, 20.0);
    let (first, second) = (contact(901), contact(902));
    let mut laid = mount(tinted(well()));
    laid.dispatch_pointer_event(
        &make_down_event_for_id(first, at(20.0), PointerKind::Touch).expect("finite fixture"),
    );
    laid.tick();
    laid.pump_for(ms(100));
    laid.dispatch_pointer_event(
        &make_down_event_for_id(second, at(80.0), PointerKind::Touch).expect("finite fixture"),
    );
    laid.tick();
    // The press shows at the win, and its wave paints two frames on.
    laid.pump_for(ms(100));
    laid.pump_for(ms(16));
    laid.pump_for(ms(16));
    let pressing = waves(&laid);
    assert_eq!(pressing.len(), 1, "one wave: {pressing:?}");
    assert!((pressing[0].0.x - 20.0).abs() < 1e-6, "it is the first's");
    laid.dispatch_pointer_event(
        &make_move_event_for_id(first, at(70.0), PointerKind::Touch).expect("finite fixture"),
    );
    laid.tick();
    laid.pump_for(ms(75));
    let fading = waves(&laid);
    assert_eq!(fading.len(), 1, "the dragged contact's wave is fading");
    assert_close(fading[0].2, 0.05, "the dragged contact's wave fades");
}

/// Pressing and releasing another button while the primary is held (a
/// `ButtonChange` each way) keeps the press.
pub fn releasing_another_button_leaves_the_primary_pressing() {
    use flui_interaction::events::{
        ButtonChange, PointerButton, PointerButtons, PointerEvent, PointerId, PointerKind,
        PointerPress, PointerRelease, make_down_event_for_id_with_button,
    };
    let mut laid = mount(tinted(well()));
    let pointer = PointerId::new(std::num::NonZeroU64::new(903).expect("non-zero"));
    let down = make_down_event_for_id_with_button(
        pointer,
        flui_sdk::geometry::Offset::new(50.0, 20.0),
        PointerKind::Mouse,
        PointerButton::PRIMARY,
    )
    .expect("finite fixture position");
    let PointerEvent::Down(press) = &down else {
        unreachable!("the fixture builds a Down");
    };
    let both = PointerButtons::only(PointerButton::PRIMARY).with(PointerButton::SECONDARY);
    let secondary_down = PointerEvent::ButtonChange(ButtonChange::Pressed(PointerPress::new(
        press.pointer,
        PointerButton::SECONDARY,
        both,
        press.sample,
    )));
    let secondary_up = PointerEvent::ButtonChange(ButtonChange::Released(PointerRelease::new(
        press.pointer,
        PointerButton::SECONDARY,
        PointerButtons::only(PointerButton::PRIMARY),
        press.sample,
    )));
    laid.dispatch_pointer_event(&down);
    laid.tick();
    laid.pump_for(ms(30));
    laid.dispatch_pointer_event(&secondary_down);
    laid.dispatch_pointer_event(&secondary_up);
    laid.tick();
    laid.pump_for(ms(100));
    let held = waves(&laid);
    assert_eq!(held.len(), 1, "the press keeps its wave");
    assert_close(held[0].2, 0.10, "the wave stays at the full pressed alpha");
}

/// The primary button coming up while another is still held ends the press,
/// though the contact goes on until that other button is released.
pub fn a_primary_release_under_another_button_ends_the_press() {
    use flui_interaction::events::{
        ButtonChange, PointerButton, PointerButtons, PointerEvent, PointerId, PointerKind,
        PointerRelease, make_down_event_for_id_with_button,
    };
    let mut laid = mount(tinted(well()));
    let pointer = PointerId::new(std::num::NonZeroU64::new(900).expect("non-zero"));
    let down = make_down_event_for_id_with_button(
        pointer,
        flui_sdk::geometry::Offset::new(50.0, 20.0),
        PointerKind::Mouse,
        PointerButton::PRIMARY,
    )
    .expect("finite fixture position");
    let PointerEvent::Down(press) = &down else {
        unreachable!("the fixture builds a Down");
    };
    let primary_up = PointerEvent::ButtonChange(ButtonChange::Released(PointerRelease::new(
        press.pointer,
        PointerButton::PRIMARY,
        PointerButtons::only(PointerButton::SECONDARY),
        press.sample,
    )));
    laid.dispatch_pointer_event(&down);
    laid.tick();
    laid.pump_for(ms(100));
    laid.dispatch_pointer_event(&primary_up);
    laid.tick();
    laid.pump_for(ms(75));
    let fading = waves(&laid);
    assert_eq!(fading.len(), 1, "the wave is fading");
    assert_close(fading[0].2, 0.05, "wave alpha halfway through its fade-out");
}

/// Without a frame clock a wave cannot grow, so a press shows on the state
/// layer, as under reduced motion.
pub fn a_well_without_a_frame_clock_shows_the_pressed_layer() {
    let mut laid = mount(VsyncScope::detached(tinted(well())));
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(16));
    assert!(waves(&laid).is_empty(), "no wave without a clock");
    let layers = layers(&laid);
    assert_eq!(layers.len(), 1, "one state layer: {:?}", ink(&laid));
    assert_close(layers[0], 0.10, "pressed state-layer alpha");
    laid.dispatch_pointer_up(50.0, 20.0);
}

/// Once a hover and a press have settled, the well records exactly what it
/// recorded idle: the painter is detached again.
pub fn a_settled_well_detaches_its_painter() {
    let mut laid = mount(tinted(well()));
    let idle = laid.draw_ops().len();
    laid.dispatch_pointer_hover(50.0, 20.0);
    laid.pump_for(ms(16));
    laid.pump_for(ms(16));
    assert!(
        laid.draw_ops().len() > idle,
        "a hover paints its state layer"
    );
    laid.dispatch_window_hover_left();
    laid.pump_for(ms(16));
    laid.pump_for(ms(200));
    laid.pump_for(ms(16));
    assert_eq!(
        laid.draw_ops().len(),
        idle,
        "a settled hover leaves nothing behind"
    );
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(30));
    laid.dispatch_pointer_up(50.0, 20.0);
    laid.tick();
    laid.pump_for(ms(300));
    laid.pump_for(ms(16));
    assert_eq!(
        laid.draw_ops().len(),
        idle,
        "a settled wave leaves nothing behind"
    );
}
