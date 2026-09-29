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
//! **Not covered here** (see `checkbox.rs`'s own unit tests instead, since
//! neither needs a render tree): the M3 default token-table branch
//! order/combined-state pins (exhaustively unit-tested against
//! `ColorScheme` directly), and the widget -> theme -> default tier
//! precedence + `active_color`'s `!Disabled && Selected` gate — both
//! exercised directly against `resolve_checkbox_fill_color` (extracted out
//! of `build` specifically so this cascade is unit-testable without
//! mounting a widget tree; see `theme_tier_beats_the_m3_default_when_no_widget_override_is_set`/
//! `widget_override_is_ignored_when_disabled_even_if_selected`), plus `CheckboxPainter`'s
//! own paint-invocation proof (`draws_the_correct_mark_per_tristate_value`,
//! a real `Canvas`/`DisplayList` recording). The illegal
//! `(None, tristate: false)` pair is unrepresentable at the type level
//! (`CheckboxMode`); the a11y cases below prove indeterminate still exports
//! `Toggled::Mixed` while binary never does.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{Checkbox, Theme, ThemeData};
use flui_testing::a11y::Toggled;

/// The checkbox's full tap target — Flutter parity: `kMinInteractiveDimension`
/// (`constants.dart`, `48.0`, oracle tag `3.44.0`), the branch
/// `Checkbox.build` always takes in this V1 (no `materialTapTargetSize`
/// override yet).
const TAP_TARGET: f64 = 48.0;

fn constraints() -> flui_sdk::rendering::BoxConstraints {
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
fn tristate_cycle_survives_a_rebuild_between_each_tap() {
    // Flutter parity: `_handleTap`'s tristate cycle (`checkbox.dart`
    // `:241-248`) — false -> true -> null -> false. Each tap here rebuilds
    // the tree with the previously-observed value (mirroring how a real
    // caller's `setState` re-renders `Checkbox` with the new `value` from
    // `onChanged`), proving the cycle end to end through real dispatch, not
    // just `Checkbox::next_value`'s unit-level pure function.
    let observed: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));

    let build = |value: Option<bool>, sink: Rc<RefCell<Option<bool>>>| {
        themed(Checkbox::tristate(value).on_changed(move |_cx, next| {
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

/// Mounts `checkbox` with a unique label, enables semantics, and returns the
/// AccessKit toggle state announced for that label.
fn announced_toggled(checkbox: Checkbox, label: &str) -> Option<Toggled> {
    let mut laid = lay_out(
        themed(checkbox.semantic_label(label).on_changed(|_cx, _| {})),
        constraints(),
    );
    laid.enable_semantics();
    laid.pump();
    laid.a11y_tree()
        .expect("semantics enabled before the frame")
        .find_by_label(label)
        .unwrap_or_else(|error| panic!("expected one node labeled {label:?}: {error}"))
        .toggled()
}

#[test]
fn indeterminate_tristate_exports_mixed_semantics() {
    // Issue #1102 AC: valid tristate `None` paints the dash (unit-covered)
    // AND exports mixed — never the old release hole of dash + unchecked.
    assert_eq!(
        announced_toggled(Checkbox::tristate(None), "indeterminate"),
        Some(Toggled::Mixed),
    );
}
