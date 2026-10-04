//! Integration tests for [`CupertinoButton`] — tap firing, the disabled
//! swallow, the press-opacity timeline under a real vsync, and per-size
//! geometry reaching the mounted render tree.

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
