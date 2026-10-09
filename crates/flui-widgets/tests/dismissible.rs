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

pub(crate) fn dismissible_slides_without_rebuilding_per_frame() {
    use flui_testing::{HeadlessBinding, MountOptions, MountOwners, PointerScript};
    use flui_widgets::{FocusRoot, GestureArenaScope, SizedBox};
    for (vertical, sign) in [(false, 1.0), (false, -1.0), (true, 1.0), (true, -1.0)] {
        let mut binding = HeadlessBinding::new();
        let progress = Rc::new(std::cell::RefCell::new(Vec::new()));
        let updates = Rc::clone(&progress);
        let mut card = Dismissible::new(
            SizedBox::new(200.0, 200.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
        )
        .direction(if vertical {
            DismissDirection::Vertical
        } else {
            DismissDirection::Horizontal
        })
        .background(ColoredBox::new(Color::rgb(80, 90, 100)))
        .secondary_background(ColoredBox::new(Color::rgb(110, 120, 130)))
        .movement_duration(Duration::from_secs(1))
        .on_update(move |_, details| updates.borrow_mut().push(details.progress));
        for direction in [
            DismissDirection::StartToEnd,
            DismissDirection::EndToStart,
            DismissDirection::Up,
            DismissDirection::Down,
        ] {
            card = card.dismiss_threshold(direction, 1.0);
        }
        let root = GestureArenaScope::new(
            binding.arena().clone(),
            FocusRoot::new(VsyncScope::new(binding.vsync().clone(), card)),
        );
        let _ = binding.mount_root(
            &root,
            MountOwners::fresh(),
            MountOptions::tight(200.0, 200.0),
        );
        binding.pump_frame(Duration::ZERO);
        let script = PointerScript::drag(
            Offset::new(100.0, 100.0),
            if vertical {
                Offset::new(100.0, 100.0 + sign * 80.0)
            } else {
                Offset::new(100.0 + sign * 80.0, 100.0)
            },
            5,
            Duration::from_millis(10),
        );
        let mut release_position = script
            .events()
            .last()
            .expect("drag ends with release")
            .position;
        for (index, mut event) in script.events().iter().copied().enumerate() {
            if event.phase == flui_testing::PointerPhase::Up {
                event.position = release_position;
            }
            event.at = if index == 0 {
                Duration::ZERO
            } else {
                Duration::from_millis(10)
            };
            binding.replay(&PointerScript::new("one drag event").with(event));
            binding.pump_frame(Duration::ZERO);
            if (3..=5).contains(&index) {
                assert_eq!(
                    binding.last_frame_report().build.elements_built,
                    0,
                    "drag motion does not rebuild after its background mounts"
                );
            }
            if index == 5 {
                if vertical {
                    event.position.dy -= sign * 160.0;
                } else {
                    event.position.dx -= sign * 160.0;
                }
                binding.replay(&PointerScript::new("cross the origin").with(event));
                binding.pump_frame(Duration::ZERO);
                let expected = if sign > 0.0 {
                    Color::rgb(110, 120, 130)
                } else {
                    Color::rgb(80, 90, 100)
                };
                let scene = binding.layer_tree().expect("dragged card scene");
                assert!(scene.iter().any(|(_, node)| {
                    if let flui_rendering::layer::Layer::Picture(picture) = node.layer() {
                        picture.picture().iter().any(|command| matches!(&command.op,
                            flui_painting::display_list::DrawOp::Rect { paint, .. } if paint.color == expected))
                    } else { false }
                }), "crossing the origin selects the other background");
                if vertical {
                    event.position.dy -= sign * 16.0;
                } else {
                    event.position.dx -= sign * 16.0;
                }
                release_position = event.position;
                binding.replay(&PointerScript::new("continue on the other side").with(event));
                binding.pump_frame(Duration::ZERO);
                assert_eq!(
                    binding.last_frame_report().build.elements_built,
                    0,
                    "motion after the sign change does not rebuild"
                );
            }
        }
        binding.pump_frame(Duration::from_millis(16));
        assert!(
            binding.vsync().has_running(),
            "release must start a return animation"
        );
        let mut previous = *progress.borrow().last().expect("drag update delivered");
        for _ in 0..4 {
            let delivered = progress.borrow().len();
            binding.pump_frame(Duration::from_millis(16));
            assert_eq!(
                binding.last_frame_report().build.elements_built,
                0,
                "vertical {vertical}, sign {sign}: movement rebuilt {:?}",
                binding.last_frame_report().build
            );
            let values = progress.borrow();
            assert_eq!(
                values.len(),
                delivered + 1,
                "on_update still follows each moving frame"
            );
            let current = *values.last().expect("moving frame delivered");
            assert!(
                current < previous && current > 0.0,
                "card returns toward its origin"
            );
            previous = current;
        }
    }
}

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
        use flui_rendering::layer::Layer;
        let tree = laid.layer_tree().expect("committed card scene");
        for (_, node) in tree.iter() {
            let Layer::Picture(picture) = node.layer() else {
                continue;
            };
            for command in picture.picture() {
                if let flui_painting::display_list::DrawOp::Rect { rect, .. } = &command.op {
                    let (x, y) = command.transform.transform_point(rect.left(), rect.top());
                    let mut point = flui_foundation::geometry::Point::new(x, y);
                    let mut parent = node.parent();
                    while let Some(id) = parent {
                        let ancestor = tree.get(id).expect("scene parent exists");
                        match ancestor.layer() {
                            Layer::Transform(layer) => point = layer.transform_point(point),
                            Layer::Offset(layer) => point += layer.offset(),
                            Layer::Opacity(layer) => point += layer.offset(),
                            _ => {}
                        }
                        parent = ancestor.parent();
                    }
                    return if vertical { point.y } else { point.x };
                }
            }
        }
        panic!("the card paints a rectangle");
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
