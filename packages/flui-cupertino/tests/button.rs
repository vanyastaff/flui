//! Integration tests for [`CupertinoButton`]: pointer tap delivery, focus and
//! keyboard activation, accessibility availability, the press fade and its
//! ownership across clock replacement and unmount, including the compositing
//! alpha, and the fill's resolved alpha.

use crate::common;

use common::{lay_out, loose, tight};
use flui_cupertino::CupertinoButton;
use flui_sdk::view::SignalWriteExt;
use flui_sdk::widgets::SizedBox;
use flui_sdk::widgets::Text;
use flui_testing::a11y::{Action, Role};

pub fn tap_callback_writes_a_signal_and_rebuilds_its_reader() {
    let probe = common::SignalProbe::new(|signals| {
        CupertinoButton::new(SizedBox::shrink())
            .on_pressed(move |ctx| signals.count.update(ctx, |count| *count += 1))
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 44.0));
    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
    assert_eq!(probe.value(), Ok(1));
    laid.pump();
    assert_eq!(probe.reads(), [0, 1]);
}

pub fn an_active_button_press_fade_migrates_and_retires() {
    exercise_button_fade_migration(false);
}

pub fn an_active_button_release_fade_migrates_and_retires() {
    exercise_button_fade_migration(true);
}

fn exercise_button_fade_migration(releasing: bool) {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use flui_sdk::animation::Vsync;
    use flui_sdk::painting::DrawOp;
    use flui_sdk::painting::styling::Color;
    use flui_sdk::view::environment::VsyncScope;
    use flui_sdk::widgets::ColoredBox;

    let phase = if releasing { "release" } else { "press" };
    let old = Vsync::new();
    let next = Vsync::new();
    let taps = Rc::new(Cell::new(0));
    let tree = |registry| {
        let taps = Rc::clone(&taps);
        VsyncScope::new(
            registry,
            CupertinoButton::new(ColoredBox::new(Color::RED).child(SizedBox::new(40.0, 20.0)))
                .on_pressed(move |_| {
                    taps.set(taps.get() + 1);
                }),
        )
    };
    let mut laid = common::lay_out_animated(tree(old.clone()), tight(100.0, 44.0), old.clone());
    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
    laid.pump_for(Duration::ZERO);
    if releasing {
        laid.pump_for(Duration::from_millis(120));
        laid.pump_for(Duration::ZERO);
    }
    laid.pump_for(Duration::from_millis(40));
    let fade = laid.find_by_render_type("RenderAnimatedOpacity");
    let before = laid.opacity(fade);
    assert!(
        before > 0.4 && before < 1.0,
        "active {phase} fade: {before}"
    );
    let painted_before = press_fade_alpha(&laid).expect("the active fade composites opacity");
    assert_eq!(painted_before, before, "the layer retains accepted opacity");
    assert!(old.has_running());
    assert_eq!(taps.get(), 1);

    laid.pump_widget(tree(next.clone()));
    assert!(old.is_empty(), "the retained button withdraws its old seat");
    assert_eq!(next.len(), 1);
    laid.adopt_vsync(next.clone())
        .expect("replacement clock admission");
    laid.pump_for(Duration::ZERO);
    let migrated = laid.opacity(fade);
    assert!((migrated - before).abs() < 1e-9, "{before} -> {migrated}");
    assert_eq!(press_fade_alpha(&laid), Some(painted_before));
    laid.pump_for(Duration::from_millis(20));
    let advanced = laid.opacity(fade);
    let painted_advanced = press_fade_alpha(&laid).expect("the advancing fade composites opacity");
    assert!(
        if releasing {
            advanced > migrated && painted_advanced > painted_before
        } else {
            advanced < migrated && painted_advanced < painted_before
        },
        "the new clock advances the selected fade phase: {migrated} -> {advanced}"
    );
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(180));
    }
    assert!((laid.opacity(fade) - 1.0).abs() < 1e-9);
    assert_eq!(press_fade_alpha(&laid), None);
    assert!(
        laid.draw_ops().iter().any(|command| {
            matches!(&command.op, DrawOp::Rect { paint, .. } if paint.color == Color::RED)
        }),
        "the child remains painted after the release"
    );
    assert!(!next.has_running(), "the release does not retrigger itself");
    assert_eq!(taps.get(), 1);

    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
    laid.pump_for(Duration::from_millis(20));
    assert!(next.has_running(), "a second tap starts another fade");
    assert_eq!(taps.get(), 2);
    laid.pump_widget(SizedBox::shrink());
    assert!(next.is_empty(), "unmount retires the active fade owner");
    laid.pump_for(Duration::from_secs(1));
    assert!(!next.has_running());
    assert_eq!(taps.get(), 2);
}

/// The alpha the button's fade composites, or `None` while it paints at full
/// opacity and composites nothing.
fn press_fade_alpha(laid: &common::LaidOut) -> Option<f64> {
    let fade = laid.find_by_render_type("RenderAnimatedOpacity");
    laid.pipeline_owner().with(|owner| {
        Some(
            owner
                .render_tree()
                .get(fade)
                .expect("the fade remains mounted")
                .paint_effects()
                .opacity?
                .alpha(),
        )
    })
}

/// The child label merges into one button node, with the same enabled state
/// that governs pointer interaction and the click a screen reader activates.
pub fn cupertino_button_with_text_child_announces_one_labelled_button_node() {
    assert_button_announcement(
        CupertinoButton::new(Text::new("Tap")).on_pressed(|_ctx| {}),
        Announced {
            disabled: false,
            click: true,
            context_menu: false,
        },
    );
}

/// A button that only long presses is enabled and offers its long press as
/// the context menu, as a detector with `on_long_press` does.
pub fn long_press_only_button_announces_enabled() {
    assert_button_announcement(
        CupertinoButton::new(Text::new("Tap")).on_long_press(|_ctx| {}),
        Announced {
            disabled: false,
            click: false,
            context_menu: true,
        },
    );
}

pub fn disabled_button_announces_disabled() {
    assert_button_announcement(
        CupertinoButton::new(Text::new("Tap")),
        Announced {
            disabled: true,
            click: false,
            context_menu: false,
        },
    );
}

/// What a button's node says and offers.
struct Announced {
    disabled: bool,
    click: bool,
    context_menu: bool,
}

fn assert_button_announcement(button: CupertinoButton, expected: Announced) {
    let mut laid = lay_out(button, loose(200.0));
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node = tree
        .find_by_label("Tap")
        .unwrap_or_else(|error| panic!("expected one node labelled \"Tap\": {error}"));

    assert_eq!(node.role(), Role::Button);
    assert_eq!(
        node.is_disabled(),
        expected.disabled,
        "the accessibility node must report interaction availability. Tree was:\n{}",
        tree.describe()
    );
    assert_eq!(
        (
            node.supports_action(Action::Click),
            node.supports_action(Action::ShowContextMenu),
        ),
        (expected.click, expected.context_menu),
        "the node offers the click and the context menu its callbacks give. Tree was:\n{}",
        tree.describe()
    );
    assert!(
        node.child_ids().is_empty(),
        "the child paragraph's label must merge into the button's own node, not form a \
         separate child node. Tree was:\n{}",
        tree.describe()
    );
}

/// The identifier a `CupertinoButton`'s author gives it (ADR-0240) is the
/// `author_id` of the button's node, and is never announced.
pub fn button_publishes_its_identifier() {
    let mut laid = lay_out(
        CupertinoButton::new(Text::new("Tap"))
            .on_pressed(|_ctx| {})
            .identifier("tap-button"),
        loose(200.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node = tree
        .find_by_label("Tap")
        .unwrap_or_else(|error| panic!("expected one node labelled \"Tap\": {error}"));
    assert_eq!(node.role(), Role::Button);
    assert_eq!(
        node.raw().author_id(),
        Some("tap-button"),
        "Tree was:\n{}",
        tree.describe()
    );
    assert!(node.raw().description().is_none() && node.value().is_none());
}

/// A button with both callbacks answers a held contact with its long press
/// alone, and a quick one with its tap alone: the two compete in one press
/// group, as Flutter's `GestureDetector` with both slots does.
pub fn a_held_press_long_presses_and_a_quick_one_taps() {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    let (taps, long_presses) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (tapped, held) = (Rc::clone(&taps), Rc::clone(&long_presses));
    let mut laid = lay_out(
        CupertinoButton::new(SizedBox::shrink())
            .on_pressed(move |_ctx| tapped.set(tapped.get() + 1))
            .on_long_press(move |_ctx| held.set(held.get() + 1)),
        tight(100.0, 44.0),
    );

    laid.dispatch_pointer_down(50.0, 22.0);
    laid.pump_for(Duration::from_millis(700));
    laid.dispatch_pointer_up(50.0, 22.0);
    assert_eq!((taps.get(), long_presses.get()), (0, 1), "a held press");

    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
    assert_eq!((taps.get(), long_presses.get()), (1, 1), "a quick press");
}

/// A rebuild that adds or removes the long press reshapes the button's press
/// group: once the rebuilt button has a long press, a held press calls it;
/// once it has none again, a held press taps, as a tap with no long press
/// beside it does in Flutter, and so does a quick one.
pub fn a_rebuild_that_adds_or_removes_the_long_press_reshapes_the_press() {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use flui_sdk::view::SignalWriteExt;

    let (taps, long_presses) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let with_long_press = Rc::new(Cell::new(false));
    let signals = Rc::new(Cell::new(None));
    let probe = {
        let (taps, long_presses) = (Rc::clone(&taps), Rc::clone(&long_presses));
        let (with_long_press, signals) = (Rc::clone(&with_long_press), Rc::clone(&signals));
        common::SignalProbe::new(move |probe_signals| {
            signals.set(Some(probe_signals));
            let tapped = Rc::clone(&taps);
            let button = CupertinoButton::new(SizedBox::shrink())
                .on_pressed(move |_ctx| tapped.set(tapped.get() + 1));
            if with_long_press.get() {
                let held = Rc::clone(&long_presses);
                button.on_long_press(move |_ctx| held.set(held.get() + 1))
            } else {
                button
            }
        })
    };
    let mut laid = lay_out(probe.view(), tight(100.0, 44.0));
    let rebuild_with = |laid: &mut common::LaidOut, long_press: bool| {
        with_long_press.set(long_press);
        let count = signals.get().expect("the probe built").count;
        probe
            .write(|ctx| count.update(ctx, |n| *n += 1))
            .expect("the probe's signal takes a write");
        laid.pump();
    };
    let held = |laid: &mut common::LaidOut| {
        laid.dispatch_pointer_down(50.0, 22.0);
        laid.pump_for(Duration::from_millis(700));
        laid.dispatch_pointer_up(50.0, 22.0);
    };

    rebuild_with(&mut laid, true);
    held(&mut laid);
    assert_eq!((taps.get(), long_presses.get()), (0, 1), "long press added");

    rebuild_with(&mut laid, false);
    held(&mut laid);
    assert_eq!(
        (taps.get(), long_presses.get()),
        (1, 1),
        "long press removed"
    );
    laid.dispatch_pointer_down(50.0, 22.0);
    laid.dispatch_pointer_up(50.0, 22.0);
    assert_eq!((taps.get(), long_presses.get()), (2, 1), "a quick press");
}

/// Tab focuses an enabled button and Enter presses it, as the activation
/// intent of Flutter's `FocusableActionDetector` does; a disabled button
/// takes no focus, so Tab leaves it unfocused.
pub fn the_activation_key_presses_a_focused_button_and_a_disabled_one_takes_no_focus() {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_testing::find;
    use flui_testing::tester::Tester;

    let presses = Rc::new(Cell::new(0));
    let counted = Rc::clone(&presses);
    let mut tester = Tester::mount(CupertinoButton::new(Text::new("Tap")).on_pressed(
        move |_ctx| {
            counted.set(counted.get() + 1);
        },
    ));
    tester.press_keys(&["Tab"]);
    tester.expect(find::button("Tap")).to_be_focused();
    assert_eq!(presses.get(), 0, "focusing presses nothing");
    tester.press_keys(&["Enter"]);
    assert_eq!(presses.get(), 1, "Enter presses the focused button");

    let mut tester = Tester::mount(CupertinoButton::new(Text::new("Tap")));
    tester.press_keys(&["Tab"]);
    let disabled = tester.read(&find::button("Tap"));
    assert!(
        !disabled.focusable && !disabled.focused,
        "a disabled button takes no focus: {disabled:?}"
    );
}

/// The fade follows the contact: it holds the pressed opacity while the
/// pointer is down, past the fade-out time, and returns to full opacity
/// after the release, which calls the handler once. A press that moves past
/// the slop, or that the platform cancels, fades back without calling it.
pub fn the_press_fade_follows_the_held_contact_and_a_cancelled_press_restores_it() {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use flui_sdk::animation::Vsync;
    use flui_sdk::painting::styling::Color;
    use flui_sdk::view::environment::VsyncScope;
    use flui_sdk::widgets::ColoredBox;

    let vsync = Vsync::new();
    let taps = Rc::new(Cell::new(0));
    let counted = Rc::clone(&taps);
    let mut laid = common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            CupertinoButton::new(ColoredBox::new(Color::RED).child(SizedBox::new(40.0, 20.0)))
                .on_pressed(move |_| counted.set(counted.get() + 1)),
        ),
        tight(100.0, 44.0),
        vsync,
    );
    let settle = |laid: &mut common::LaidOut| {
        laid.pump_for(Duration::ZERO);
        for _ in 0..3 {
            laid.pump_for(Duration::from_millis(200));
        }
    };
    let assert_held = |laid: &common::LaidOut, case: &str| {
        let alpha = press_fade_alpha(laid)
            .unwrap_or_else(|| panic!("{case}: a held press composites its fade"));
        assert!(
            (alpha - 0.4).abs() < 1e-6,
            "{case}: a held press rests at the pressed opacity, painted {alpha}"
        );
    };
    let assert_released = |laid: &common::LaidOut, case: &str| {
        let fade = laid.find_by_render_type("RenderAnimatedOpacity");
        assert!(
            (laid.opacity(fade) - 1.0).abs() < 1e-9,
            "{case}: the button returns to full opacity, at {}",
            laid.opacity(fade)
        );
        assert_eq!(press_fade_alpha(laid), None, "{case}");
    };

    laid.dispatch_pointer_down(50.0, 22.0);
    settle(&mut laid);
    assert_held(&laid, "held");
    assert_eq!(taps.get(), 0, "a held press has not tapped yet");
    laid.dispatch_pointer_up(50.0, 22.0);
    settle(&mut laid);
    assert_released(&laid, "released");
    assert_eq!(taps.get(), 1, "the release taps once");

    laid.dispatch_pointer_down(50.0, 22.0);
    settle(&mut laid);
    assert_held(&laid, "before moving out");
    laid.dispatch_pointer_move(50.0, 300.0);
    settle(&mut laid);
    assert_released(&laid, "moved out");
    laid.dispatch_pointer_up(50.0, 300.0);
    settle(&mut laid);
    assert_eq!(taps.get(), 1, "a press moved past the slop does not tap");

    laid.dispatch_pointer_down(50.0, 22.0);
    settle(&mut laid);
    assert_held(&laid, "before the cancel");
    laid.dispatch_pointer_cancel();
    settle(&mut laid);
    assert_released(&laid, "cancelled");
    assert_eq!(taps.get(), 1, "a cancelled press does not tap");
}

/// An activation with no contact behind it, here a screen reader's click,
/// calls the handler once and changes no fade, as Flutter's `_handleTap`
/// does: the path the activation key takes too.
pub fn a_contactless_activation_runs_the_handler_without_a_fade() {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use flui_sdk::animation::Vsync;
    use flui_sdk::view::environment::VsyncScope;
    use flui_testing::a11y::{ActionRequest, TreeId};

    let vsync = Vsync::new();
    let taps = Rc::new(Cell::new(0));
    let counted = Rc::clone(&taps);
    let mut laid = common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            CupertinoButton::new(Text::new("Tap"))
                .on_pressed(move |_| counted.set(counted.get() + 1)),
        ),
        tight(100.0, 44.0),
        vsync.clone(),
    );
    laid.enable_semantics();
    laid.pump();
    let button = laid
        .a11y_tree()
        .expect("semantics enabled before the frame")
        .find_by_label("Tap")
        .unwrap_or_else(|error| panic!("expected one node labelled \"Tap\": {error}"))
        .id();

    laid.invoke_semantics_action(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: button,
        data: None,
    })
    .unwrap_or_else(|error| panic!("the button takes a click: {error}"));
    assert_eq!(taps.get(), 1, "the click calls the handler once");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    let fade = laid.find_by_render_type("RenderAnimatedOpacity");
    assert!(
        (laid.opacity(fade) - 1.0).abs() < 1e-9,
        "a contactless activation leaves full opacity, at {}",
        laid.opacity(fade)
    );
    assert_eq!(press_fade_alpha(&laid), None, "and composites no fade");
    assert!(
        vsync.is_empty() && !vsync.has_running(),
        "no controller is made, so no frames are requested"
    );
    assert_eq!(taps.get(), 1);
}

/// A plain button paints its dynamic colour with the alpha of the variant
/// the theme's brightness resolves: `SEPARATOR` is alpha 73 light and 153
/// dark.
pub fn a_plain_button_paints_its_resolved_colour_with_its_own_alpha() {
    use flui_cupertino::{CupertinoColors, CupertinoTheme, CupertinoThemeData};
    use flui_sdk::painting::DrawOp;
    use flui_sdk::platform::Brightness;

    for (brightness, alpha) in [(Brightness::Light, 73), (Brightness::Dark, 153)] {
        let mut theme = CupertinoThemeData::default();
        theme.brightness = Some(brightness);
        let laid = lay_out(
            CupertinoTheme::new(
                theme,
                CupertinoButton::new(SizedBox::new(40.0, 20.0))
                    .color(CupertinoColors::SEPARATOR)
                    .on_pressed(|_ctx| {}),
            ),
            tight(100.0, 44.0),
        );
        let fills: Vec<u8> = laid
            .draw_ops()
            .iter()
            .filter_map(|command| match &command.op {
                DrawOp::RRect { paint, .. } => Some(paint.color.to_rgba8()[3]),
                _ => None,
            })
            .collect::<Vec<u8>>();
        assert_eq!(fills, [alpha], "{brightness:?}: the fill's alpha");
    }
}
