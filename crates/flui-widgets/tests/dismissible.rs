//! Dismissible release: cancellation restores a dragged card, a fling keeps
//! the finger's speed.
use crate::common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, DismissDirection, Dismissible, GestureDetector, VsyncScope};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

pub(crate) fn dismissal_release_uses_its_captured_fling_profile() {
    dismissal_release_uses_profile(DismissDirection::Horizontal);
}

pub(crate) fn vertical_dismissal_release_uses_its_captured_fling_profile() {
    dismissal_release_uses_profile(DismissDirection::Vertical);
}

fn dismissal_release_uses_profile(direction: DismissDirection) {
    for (min, max) in [(50.0, 100.0), (5000.0, 5000.0)] {
        let profile = |min, max| {
            flui_interaction::GestureSettings::default()
                .try_with_fling_velocity(min, max)
                .expect("valid fling range")
        };
        let source = flui_interaction::settings::GestureSettingsSource::new(profile(min, max));
        let dismissed = Rc::new(Cell::new(0));
        let recorder = Rc::clone(&dismissed);
        let card = Dismissible::new(ColoredBox::new(Color::rgb(10, 20, 30)))
            .direction(direction)
            .resize_duration(None)
            .on_dismissed(move |_, _| recorder.set(recorder.get() + 1));
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                crate::scroll::FlingProfile {
                    provider: source.provider(),
                    child: flui_view::ViewExt::boxed(card),
                },
            ),
            tight(300.0, 300.0),
            vsync,
        );
        let horizontal = direction == DismissDirection::Horizontal;
        for attempt in 0..2 {
            laid.dispatch_pointer_down(20.0, 20.0);
            for value in [35.0, 50.0, 65.0, 80.0, 95.0] {
                let (x, y) = if horizontal {
                    (value, 20.0)
                } else {
                    (20.0, value)
                };
                laid.dispatch_pointer_move_after(x, y, Duration::from_millis(10));
            }
            if attempt == 0 {
                source.replace(profile(50.0, 2000.0));
            }
            let (x, y) = if horizontal {
                (95.0, 20.0)
            } else {
                (20.0, 95.0)
            };
            laid.dispatch_pointer_up(x, y);
            for _ in 0..60 {
                laid.pump_for(Duration::from_millis(16));
            }
            assert_eq!(
                dismissed.get(),
                attempt,
                "{direction:?}: admitted range {min}..{max} suppresses the old release; a fresh profile recovers"
            );
        }
    }
}

fn cancelled_card(direction: DismissDirection, end: f64) {
    let dismissed = Rc::new(Cell::new(0));
    let taps = Rc::new(Cell::new(0));
    let on_dismiss = Rc::clone(&dismissed);
    let on_tap = Rc::clone(&taps);
    let vsync = Vsync::new();
    let child = GestureDetector::new()
        .on_tap(move |_| on_tap.set(on_tap.get() + 1))
        .child(ColoredBox::new(Color::rgb(10, 20, 30)));
    let card = Dismissible::new(child)
        .direction(direction)
        .resize_duration(None)
        .on_dismissed(move |_, _| on_dismiss.set(on_dismiss.get() + 1));
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), card),
        tight(200.0, 200.0),
        vsync,
    );
    let horizontal = direction == DismissDirection::Horizontal;
    let (x, y) = if horizontal { (end, 20.0) } else { (20.0, end) };
    let card_target = laid.find_by_render_type("RenderDecoratedBox");
    assert!(
        laid.hit_test_pointer(Offset::new(20.0, 20.0))
            .path()
            .iter()
            .any(|entry| entry.target == card_target)
    );
    let (slop_x, slop_y) = if horizontal {
        (40.0, 20.0)
    } else {
        (20.0, 40.0)
    };
    laid.dispatch_pointer_down(20.0, 20.0);
    // The child's tap competes with the drag. The first move wins the arena;
    // Start behavior reanchors there, so a second move must update the card.
    laid.dispatch_pointer_move(slop_x, slop_y);
    laid.dispatch_pointer_move(x, y);
    laid.pump();
    assert!(
        !laid
            .hit_test_pointer(Offset::new(20.0, 20.0))
            .path()
            .iter()
            .any(|entry| entry.target == card_target),
        "the admitted drag visibly moved the card before cancellation"
    );
    if end > 200.0 {
        assert!(
            !laid
                .hit_test_pointer(Offset::new(199.0, 20.0))
                .path()
                .iter()
                .any(|entry| entry.target == card_target),
            "the fully slid horizontal card is outside the viewport"
        );
    }
    laid.dispatch_pointer_cancel();
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        dismissed.get(),
        0,
        "cancel beyond threshold never dismisses"
    );
    // The original hit location becomes reachable again only after the slide
    // returns to zero; suppressing callbacks without restoring motion fails.
    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);
    assert_eq!(
        taps.get(),
        1,
        "cancelled card returned to its original hit location"
    );
    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_move(slop_x, slop_y);
    laid.dispatch_pointer_move(x, y);
    laid.dispatch_pointer_up(x, y);
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(dismissed.get(), 1, "next completed gesture still dismisses");
}

pub(crate) fn a_cancelled_horizontal_dismiss_restores_the_card() {
    cancelled_card(DismissDirection::Horizontal, 150.0);
}

pub(crate) fn a_cancelled_vertical_dismiss_restores_the_card() {
    cancelled_card(DismissDirection::Vertical, 150.0);
}

pub(crate) fn cancelling_a_fully_slid_card_restores_it_without_dismissal() {
    cancelled_card(DismissDirection::Horizontal, 250.0);
}

/// The card's speed, in px/s, just after a release at 1500 px/s on a card
/// `width` px wide.
fn release_speed(width: f64, reverse: bool, maximum: Option<f64>, vertical: bool) -> f64 {
    let painted_x = |laid: &crate::common::LaidOut| {
        laid.draw_ops()
            .into_iter()
            .find_map(|command| {
                if let flui_painting::display_list::DrawOp::Rect { rect, .. } = command.op {
                    let (x, y) = command.transform.transform_point(rect.left(), rect.top());
                    Some(if vertical { y } else { x })
                } else {
                    None
                }
            })
            .expect("the card paints a rectangle")
    };
    let vsync = Vsync::new();
    let size = if vertical {
        flui_foundation::geometry::Size::new(100.0, width)
    } else {
        flui_foundation::geometry::Size::new(width, 100.0)
    };
    let card = Dismissible::new(
        flui_widgets::SizedBox::new(size.width, size.height)
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
    )
    .direction(if vertical {
        DismissDirection::Vertical
    } else {
        DismissDirection::Horizontal
    })
    .resize_duration(None);
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), card),
        if let Some(maximum) = maximum {
            flui_rendering::constraints::BoxConstraints::loose(
                flui_foundation::geometry::Size::new(
                    if vertical { 100.0 } else { maximum },
                    if vertical { maximum } else { 100.0 },
                ),
            )
        } else {
            tight(size.width, size.height)
        },
        vsync,
    );
    let point = |primary| {
        if vertical {
            (50.0, primary)
        } else {
            (primary, 50.0)
        }
    };
    let (start_x, start_y) = point(10.0);
    laid.dispatch_pointer_down(start_x, start_y);
    // 15 px every 10 ms: 1500 px/s to the right.
    let mut x = 10.0;
    for _ in 0..if reverse { 12 } else { 5 } {
        x += if reverse { 20.0 } else { 15.0 };
        let (px, py) = point(x);
        laid.dispatch_pointer_move_after(px, py, Duration::from_millis(10));
    }
    if reverse {
        for _ in 0..12 {
            x -= 15.0;
            let (px, py) = point(x);
            laid.dispatch_pointer_move_after(px, py, Duration::from_millis(10));
        }
    }
    laid.pump();
    let before_release = painted_x(&laid);
    assert!(before_release > 0.0);
    let (px, py) = point(x);
    laid.dispatch_pointer_up(px, py);
    laid.pump();
    let after_release = painted_x(&laid);
    assert!(
        (after_release - before_release).abs() < 1e-9,
        "release on width {width}, reverse {reverse} moved the painted card from {before_release} to {after_release}"
    );
    // A frame anchors the fling, then a 0.1 ms step samples its start: the
    // settle spring's acceleration changes the speed by under 5 % that soon.
    let step = Duration::from_micros(100);
    let mut samples = Vec::new();
    for _ in 0..3 {
        laid.pump_for(step);
        samples.push(painted_x(&laid));
    }
    let moving: Vec<_> = samples
        .windows(2)
        .filter(|pair| pair[1] != pair[0])
        .collect();
    let pair = moving.first().expect("the released card moves");
    (pair[1] - pair[0]) / step.as_secs_f64()
}

/// A fling hands the finger's speed to the settle animation whatever the
/// card's width: the gesture's px/s become controller units per second by
/// dividing by the width, not by a fixed scale.
pub(crate) fn a_dismissible_release_keeps_finger_speed_on_any_width() {
    for vertical in [false, true] {
        for (width, reverse, maximum) in [
            (150.0, false, None),
            (1200.0, false, None),
            (150.0, true, None),
            (1200.0, true, None),
            (150.0, false, Some(2400.0)),
            (1200.0, false, Some(2400.0)),
            (150.0, true, Some(2400.0)),
            (150.0, false, Some(f64::INFINITY)),
        ] {
            let speed = release_speed(width, reverse, maximum, vertical);
            let expected = if reverse { -1500.0 } else { 1500.0 };
            assert!(
                (speed - expected).abs() <= 0.1 * 1500.0,
                "a {width} px card (maximum {maximum:?}, vertical {vertical}) left at {speed} px/s after a {expected} px/s release"
            );
        }
    }
}

pub(crate) fn a_dismissible_collapses_its_laid_out_size() {
    let color = Color::rgb(131, 43, 71);
    let dismissed = Rc::new(Cell::new(0));
    let output = dismissed.clone();
    let vsync = Vsync::new();
    let card = Dismissible::new(
        flui_widgets::SizedBox::new(150.0, 100.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
    )
    .background(ColoredBox::new(color))
    .on_dismissed(move |_, _| output.set(output.get() + 1));
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), card),
        flui_rendering::constraints::BoxConstraints::loose(flui_foundation::geometry::Size::new(
            2400.0, 100.0,
        )),
        vsync,
    );
    laid.dispatch_pointer_down(10.0, 50.0);
    for step in 1..=12 {
        laid.dispatch_pointer_move_after(
            10.0 + f64::from(step) * 20.0,
            50.0,
            Duration::from_millis(10),
        );
    }
    laid.dispatch_pointer_up(250.0, 50.0);
    laid.pump();
    let rect = laid
        .draw_ops()
        .into_iter()
        .find_map(|command| {
            if let flui_painting::display_list::DrawOp::Rect { rect, paint } = command.op
                && paint.color == color
            {
                Some(rect)
            } else {
                None
            }
        })
        .expect("the collapsing background paints");
    assert_eq!((rect.width(), rect.height()), (150.0, 100.0));
    assert_eq!(dismissed.get(), 0);
    laid.pump_for(Duration::from_secs(1));
    laid.pump_for(Duration::from_secs(1));
    assert_eq!(dismissed.get(), 1);
}
