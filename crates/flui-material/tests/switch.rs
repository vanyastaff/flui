//! `Switch` widget-level mount/interaction coverage.
//!
//! Complements `switch.rs`'s own unit tests (M3 default token-table probes,
//! thumb-radius selection, painter `should_repaint`) with end-to-end
//! mount/dispatch proof: a real down+up through the render tree reaches
//! [`Switch::on_changed`] with `!value`, and a handler-removal/-addition
//! across the lifecycle resyncs interactivity — the same classes
//! `tests/checkbox.rs` proves for `Checkbox`, since `Switch` shares the same
//! `InkWell`-composition shape.
//!
//! **Not covered here** (see `switch.rs`'s own unit tests instead): the M3
//! default token-table branch order/combined-state pins, and per-field
//! theme-tier-beats-default resolution — both pure-function unit tests, no
//! render tree needed.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size, tight};
use flui_material::{Switch, Theme, ThemeData};

/// The switch's full tap target (track + M3 default horizontal padding).
const TAP_TARGET_WIDTH: f32 = 60.0;
const TAP_TARGET_HEIGHT: f32 = 48.0;

fn constraints() -> flui_rendering::constraints::BoxConstraints {
    tight(TAP_TARGET_WIDTH, TAP_TARGET_HEIGHT)
}

/// Every `Switch` needs a [`Theme`] ancestor (`Theme::of` panics without
/// one) — mirrors `tests/checkbox.rs`'s own `themed` helper.
fn themed(switch: Switch) -> Theme {
    Theme::new(ThemeData::light(), switch)
}

#[test]
fn mounting_a_switch_creates_a_semantics_annotated_tap_target() {
    let laid = lay_out(themed(Switch::new(false).on_changed(|_| {})), constraints());

    let semantics = laid
        .find_by_render_type("RenderSemanticsAnnotations")
        .expect("Switch must mount a Semantics wrapper");
    assert_eq!(
        laid.size(semantics),
        size(TAP_TARGET_WIDTH, TAP_TARGET_HEIGHT)
    );
}

#[test]
fn tap_fires_on_changed_with_the_flipped_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(Switch::new(false).on_changed(move |next| {
            *recorder.borrow_mut() = Some(next);
        })),
        constraints(),
    );

    laid.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);

    assert_eq!(
        *observed.borrow(),
        Some(true),
        "a tap on an off, enabled switch must fire on_changed(true)",
    );
}

#[test]
fn a_second_tap_after_rebuild_flips_back() {
    let observed: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let build = |value: bool, sink: Rc<RefCell<bool>>| {
        themed(Switch::new(value).on_changed(move |next| {
            *sink.borrow_mut() = next;
        }))
    };

    let mut laid = lay_out(build(false, Rc::clone(&observed)), constraints());

    laid.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    let after_first_tap = *observed.borrow();
    assert!(after_first_tap, "false -> true");

    laid.pump_widget(build(after_first_tap, Rc::clone(&observed)));
    laid.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);

    assert!(!*observed.borrow(), "true -> false");
}

#[test]
fn disabled_switch_swallows_a_tap_then_resyncs_once_a_handler_is_added() {
    // Same "handler-removal resync" class `tests/checkbox.rs` proves for
    // `Checkbox`: `Switch` shares its `WidgetStatesController` with the
    // `InkWell` it builds, so adding `on_changed` across a rebuild must
    // flip `Disabled` and make the NEXT tap interactive.
    let taps = Rc::new(RefCell::new(0_u32));

    let laid_disabled = lay_out(themed(Switch::new(false)), constraints());
    laid_disabled.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid_disabled.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    // No on_changed at all: nothing to observe going wrong beyond "does not
    // panic" — the InkWell-level swallow behavior itself is already proven
    // by `tests/ink_well.rs`'s disabled-state coverage.

    let mut laid_enabled = laid_disabled;
    let counter = Rc::clone(&taps);
    laid_enabled.pump_widget(themed(Switch::new(false).on_changed(move |_| {
        *counter.borrow_mut() += 1;
    })));
    laid_enabled.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid_enabled.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);

    assert_eq!(
        *taps.borrow(),
        1,
        "adding on_changed on rebuild must make the very next tap interactive",
    );
}
