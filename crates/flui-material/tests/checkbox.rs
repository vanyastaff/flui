//! `Checkbox` widget-level mount/interaction coverage.
//!
//! Complements `checkbox.rs`'s own unit tests (M3 default token-table
//! probes, tristate `next_value` cycle, painter `should_repaint`) with
//! end-to-end mount/dispatch proof: a real down+up through the render tree
//! reaches [`Checkbox::on_changed`], the tristate cycle survives a real
//! `pump_widget` rebuild between taps, and a handler-removal/-addition
//! across the lifecycle resyncs interactivity (the same "disabled through
//! the lifecycle" class `tests/ink_well.rs` proves for `InkWell` itself,
//! since `Checkbox` shares its `WidgetStatesController` with the `InkWell`
//! it builds).
//!
//! **Not covered here** (see `checkbox.rs`'s own unit tests instead): the
//! M3 default token-table branch order/combined-state pins (exhaustively
//! unit-tested against `ColorScheme` directly — no render-tree needed to
//! prove pure functions), and per-field theme-tier-beats-default resolution
//! (same reason: `resolve_state_color`'s fallthrough contract is unit
//! tested, and the harness has no generic accessor for a `CustomPainter`'s
//! resolved fields to assert against post-mount).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size, tight};
use flui_material::{Checkbox, Theme, ThemeData};

/// The checkbox's full tap target — Flutter parity: `kMinInteractiveDimension`
/// (`constants.dart`, `48.0`, oracle tag `3.44.0`), the branch
/// `Checkbox.build` always takes in this V1 (no `materialTapTargetSize`
/// override yet).
const TAP_TARGET: f32 = 48.0;

fn constraints() -> flui_rendering::constraints::BoxConstraints {
    tight(TAP_TARGET, TAP_TARGET)
}

/// Every `Checkbox` needs a [`Theme`] ancestor (`Theme::of` panics without
/// one) — this wraps the M3 light baseline around `checkbox`, matching how
/// every other themed-widget integration test in this crate mounts its
/// subject (see e.g. `tests/card.rs`).
fn themed(checkbox: Checkbox) -> Theme {
    Theme::new(ThemeData::light(), checkbox)
}

#[test]
fn mounting_a_checkbox_creates_a_semantics_annotated_tap_target() {
    let laid = lay_out(
        themed(Checkbox::new(Some(false)).on_changed(|_| {})),
        constraints(),
    );

    let semantics = laid
        .find_by_render_type("RenderSemanticsAnnotations")
        .expect("Checkbox must mount a Semantics wrapper");
    assert_eq!(laid.size(semantics), size(TAP_TARGET, TAP_TARGET));
}

#[test]
fn tap_fires_on_changed_with_the_next_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(Checkbox::new(Some(false)).on_changed(move |next| {
            *recorder.borrow_mut() = Some(next);
        })),
        constraints(),
    );

    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);

    assert_eq!(
        *observed.borrow(),
        Some(Some(true)),
        "a tap on an unchecked, enabled checkbox must fire on_changed(Some(true))",
    );
}

#[test]
fn tristate_cycle_survives_a_rebuild_between_each_tap() {
    // Flutter parity: `_handleTap`'s tristate cycle (`checkbox.dart`
    // `:241-248`) — false -> true -> null -> false. Each tap here rebuilds
    // the tree with the previously-observed value (mirroring how a real
    // caller's `setState` re-renders `Checkbox` with the new `value` from
    // `onChanged`), proving the cycle end to end through real dispatch, not
    // just `Checkbox::next_value`'s unit-level pure function.
    let observed: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));

    let build = |value: Option<bool>, sink: Rc<RefCell<Option<bool>>>| {
        themed(Checkbox::new(value).tristate(true).on_changed(move |next| {
            *sink.borrow_mut() = next;
        }))
    };

    let mut laid = lay_out(build(Some(false), Rc::clone(&observed)), constraints());

    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    let after_first_tap = *observed.borrow();
    assert_eq!(after_first_tap, Some(true), "false -> true");

    laid.pump_widget(build(after_first_tap, Rc::clone(&observed)));
    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    let after_second_tap = *observed.borrow();
    assert_eq!(after_second_tap, None, "true -> null (indeterminate)");

    laid.pump_widget(build(after_second_tap, Rc::clone(&observed)));
    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    assert_eq!(*observed.borrow(), Some(false), "null -> false");
}

#[test]
fn disabled_checkbox_swallows_a_tap_then_resyncs_once_a_handler_is_added() {
    // The "handler-removal resync" class: `Checkbox` shares its
    // `WidgetStatesController` with the `InkWell` it builds, so adding
    // `on_changed` across a rebuild must flip that controller's `Disabled`
    // bit and make the NEXT tap interactive — proving the shared-controller
    // wiring survives `did_update_view`, not just the initial mount.
    let taps = Rc::new(RefCell::new(0_u32));

    let laid_disabled = lay_out(themed(Checkbox::new(Some(false))), constraints());
    laid_disabled.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid_disabled.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    // No on_changed at all: nothing to observe going wrong beyond "does not
    // panic" — the InkWell-level swallow behavior itself is already proven
    // by `tests/ink_well.rs`'s disabled-state coverage.

    let mut laid_enabled = laid_disabled;
    let counter = Rc::clone(&taps);
    laid_enabled.pump_widget(themed(Checkbox::new(Some(false)).on_changed(move |_| {
        *counter.borrow_mut() += 1;
    })));
    laid_enabled.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid_enabled.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);

    assert_eq!(
        *taps.borrow(),
        1,
        "adding on_changed on rebuild must make the very next tap interactive",
    );
}
