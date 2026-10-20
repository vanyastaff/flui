//! Integration tests for [`CupertinoButton`]: pointer tap delivery,
//! accessibility availability, and active fade ownership across clock
//! replacement and unmount, including the compositing alpha.

use crate::common;

use common::{lay_out, loose, tight};
use flui_cupertino::CupertinoButton;
use flui_sdk::view::SignalWriteExt;
use flui_sdk::widgets::SizedBox;
use flui_sdk::widgets::Text;
use flui_testing::a11y::Role;

pub fn tap_callback_writes_a_signal_and_rebuilds_its_reader() {
    let probe = common::SignalProbe::new(|signals| {
        CupertinoButton::new(SizedBox::shrink())
            .on_pressed(move |cx| signals.count.update(cx, |count| *count += 1))
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
    use flui_sdk::painting::{Color, DrawOp};
    use flui_sdk::widgets::{ColoredBox, VsyncScope};

    let paint_alpha = |laid: &common::LaidOut, render| {
        laid.pipeline_owner().with(|owner| {
            Some(
                owner
                    .render_tree()
                    .get(render)
                    .expect("the fade remains mounted")
                    .paint_effects()
                    .opacity?
                    .alpha(),
            )
        })
    };

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
    let painted_before = paint_alpha(&laid, fade).expect("the active fade composites opacity");
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
    assert_eq!(paint_alpha(&laid, fade), Some(painted_before));
    laid.pump_for(Duration::from_millis(20));
    let advanced = laid.opacity(fade);
    let painted_advanced = paint_alpha(&laid, fade).expect("the advancing fade composites opacity");
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
    assert_eq!(paint_alpha(&laid, fade), None);
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

/// The child label merges into one button node, with the same enabled state
/// that governs pointer interaction.
pub fn cupertino_button_with_text_child_announces_one_labelled_button_node() {
    assert_button_announcement(
        CupertinoButton::new(Text::new("Tap")).on_pressed(|_cx| {}),
        false,
    );
}

pub fn long_press_only_button_announces_enabled() {
    assert_button_announcement(
        CupertinoButton::new(Text::new("Tap")).on_long_press(|_cx| {}),
        false,
    );
}

pub fn disabled_button_announces_disabled() {
    assert_button_announcement(CupertinoButton::new(Text::new("Tap")), true);
}

fn assert_button_announcement(button: CupertinoButton, disabled: bool) {
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
        disabled,
        "the accessibility node must report interaction availability. Tree was:\n{}",
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
            .on_pressed(|_cx| {})
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
