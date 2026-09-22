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
//! `widget_override_wins_over_theme_and_default_when_selected_and_enabled`/
//! `widget_override_is_ignored_when_disabled_even_if_selected`/
//! `widget_override_is_ignored_when_unselected`), plus `CheckboxPainter`'s
//! own paint-invocation proof (`draws_the_correct_mark_per_tristate_value`,
//! a real `Canvas`/`DisplayList` recording). The illegal
//! `(None, tristate: false)` pair is unrepresentable at the type level
//! (`CheckboxMode`); the a11y cases below prove indeterminate still exports
//! `Toggled::Mixed` while binary never does.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size, tight};
use flui_material::{Checkbox, Theme, ThemeData};
use flui_testing::a11y::{Role, Toggled};

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
        themed(Checkbox::new(false).on_changed(|_| {})),
        constraints(),
    );

    // Two annotation nodes are expected: the checkbox's own `Semantics`
    // wrapper (the outer one, sized to the tap target) and the one its
    // `GestureDetector` mounts to advertise the tap action to assistive
    // technology. The outer one is the checkbox's.
    let semantics = laid.find_all_by_render_type("RenderSemanticsAnnotations");
    assert_eq!(
        semantics.len(),
        2,
        "the Checkbox Semantics wrapper plus the GestureDetector's action node"
    );
    let outer = semantics
        .iter()
        .copied()
        .find(|id| laid.size(*id) == size(TAP_TARGET, TAP_TARGET))
        .expect("Checkbox must mount a Semantics wrapper sized to its tap target");
    assert_eq!(laid.size(outer), size(TAP_TARGET, TAP_TARGET));
}

#[test]
fn tap_fires_on_changed_with_the_next_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(Checkbox::new(false).on_changed(move |next| {
            // PORT-CHECK-OK-LOCK: plain data: bool, no Drop
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
        themed(Checkbox::tristate(value).on_changed(move |next| {
            // PORT-CHECK-OK-LOCK: plain data: bool, no Drop
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

    let laid_disabled = lay_out(themed(Checkbox::new(false)), constraints());
    laid_disabled.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid_disabled.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    // No on_changed at all: nothing to observe going wrong beyond "does not
    // panic" — the InkWell-level swallow behavior itself is already proven
    // by `tests/ink_well.rs`'s disabled-state coverage.

    let mut laid_enabled = laid_disabled;
    let counter = Rc::clone(&taps);
    laid_enabled.pump_widget(themed(Checkbox::new(false).on_changed(move |_| {
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

/// Mounts `checkbox` with a unique label, enables semantics, and returns the
/// AccessKit toggle state announced for that label.
fn announced_toggled(checkbox: Checkbox, label: &str) -> Option<Toggled> {
    let mut laid = lay_out(
        themed(checkbox.semantic_label(label).on_changed(|_| {})),
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

/// Every AccessKit role the tree exports for a mounted `Checkbox`.
fn announced_roles(checkbox: Checkbox, label: &str) -> Vec<Role> {
    let mut laid = lay_out(
        themed(checkbox.semantic_label(label).on_changed(|_| {})),
        constraints(),
    );
    laid.enable_semantics();
    laid.pump();
    laid.a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect()
}

#[test]
fn a_checkbox_still_announces_as_a_checkbox() {
    // Guards the checkable roles against the group flag leaking: publishing
    // `in_mutually_exclusive_group` for radios must not reclassify the other
    // checkables. A checkbox carries neither `IsButton` nor the group flag, so
    // it resolves to `CheckBox` under either arm order of the role cascade —
    // this passes before and after that reorder and is therefore a leak guard,
    // not evidence for the reorder itself (see `crates/flui-material/ARCHITECTURE.md`).
    let roles = announced_roles(Checkbox::new(false), "unchecked");
    assert!(
        roles.contains(&Role::CheckBox),
        "a Checkbox must still announce as a checkbox, got {roles:?}",
    );
}

#[test]
fn a_checkbox_never_announces_as_a_radio_button() {
    // The discriminating half of the leak guard above: a resolution that
    // answered `RadioButton` for every checkable would satisfy that test and
    // fail this one. Same caveat applies — a checkbox carries neither of the
    // flags the cascade reorder turned on, so this bounds the group flag's
    // reach rather than proving the reorder.
    let roles = announced_roles(Checkbox::new(false), "unchecked");
    assert!(
        !roles.contains(&Role::RadioButton),
        "a Checkbox must never announce as a radio button, got {roles:?}",
    );
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

#[test]
fn binary_checkbox_never_exports_mixed_semantics() {
    assert_eq!(
        announced_toggled(Checkbox::new(false), "binary-off"),
        Some(Toggled::False),
    );
    assert_eq!(
        announced_toggled(Checkbox::new(true), "binary-on"),
        Some(Toggled::True),
    );
}

#[test]
fn tristate_some_values_export_checked_not_mixed() {
    assert_eq!(
        announced_toggled(Checkbox::tristate(Some(false)), "tri-off"),
        Some(Toggled::False),
    );
    assert_eq!(
        announced_toggled(Checkbox::tristate(Some(true)), "tri-on"),
        Some(Toggled::True),
    );
}
