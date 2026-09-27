//! `Radio` widget-level mount/interaction coverage.
//!
//! Complements `radio.rs`'s own unit tests (M3 default token-table probes,
//! `is_selected` equality, painter `should_repaint`) with end-to-end
//! mount/dispatch proof: a real down+up through the render tree reaches
//! [`Radio::on_changed`] with the tapped radio's own value, a tap on an
//! already-selected radio is a no-op, and a handler-removal/-addition
//! across the lifecycle resyncs interactivity — the same classes
//! `tests/checkbox.rs`/`tests/switch.rs` prove for their own controls,
//! since `Radio<T>` shares the same `InkWell`-composition shape.
//!
//! Exercised with a concrete `T = &'static str` group of two radios,
//! proving the generic `Radio<T>` mounts and dispatches through the render
//! tree for a non-trivial `T` (not just the primitive `u32` the unit tests
//! use) — see `radio.rs`'s module docs for why no monomorphic fallback was
//! needed.
//!
//! **Not covered here** (see `radio.rs`'s own unit tests instead, since
//! neither needs a render tree): the M3 default token-table branch
//! order/combined-state pins, and the widget -> theme -> default tier
//! precedence + `active_color`'s `!Disabled && Selected` gate — both
//! exercised directly against `resolve_radio_ring_color` (extracted out of
//! `build` specifically so this cascade is unit-testable without mounting
//! a widget tree; see `theme_tier_beats_the_m3_default_when_no_widget_override_is_set`/
//! `widget_override_wins_over_theme_and_default_when_selected_and_enabled`/
//! `widget_override_is_ignored_when_disabled_even_if_selected`/
//! `widget_override_is_ignored_when_unselected`), plus `RadioPainter`'s own
//! paint-invocation proof (`inner_dot_is_present_only_when_selected`, a
//! real `Canvas`/`DisplayList` recording).

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size, tight};
use flui_material::{Radio, Theme, ThemeData};
use flui_sdk::widgets::Semantics;
use flui_testing::a11y::Role;

/// The radio's full tap target. Flutter parity: `kMinInteractiveDimension`.
const TAP_TARGET: f32 = 48.0;

fn constraints() -> flui_sdk::rendering::BoxConstraints {
    tight(TAP_TARGET, TAP_TARGET)
}

/// Every `Radio` needs a [`Theme`] ancestor (`Theme::of` panics without
/// one) — mirrors `tests/checkbox.rs`'s/`tests/switch.rs`'s own `themed`
/// helper.
fn themed<T: PartialEq + Clone + 'static>(radio: Radio<T>) -> Theme {
    Theme::new(ThemeData::light(), radio)
}

#[test]
fn mounting_a_radio_creates_a_semantics_annotated_tap_target() {
    let laid = lay_out(
        themed(Radio::new("spring", Some("spring")).on_changed(|_| {})),
        constraints(),
    );

    // The wrapper node is the Radio's own; its `GestureDetector` adds
    // a second, action-only annotation beneath it for assistive technology.
    let semantics = laid
        .find_semantics_wrappers()
        .into_iter()
        .next()
        .expect("Radio must mount a Semantics wrapper");
    assert_eq!(laid.size(semantics), size(TAP_TARGET, TAP_TARGET));
}

/// Every AccessKit role the tree exports for a mounted `Radio`.
///
/// Reads the same tree a platform adapter consumes, so the assertions below
/// cover the whole widget -> configuration -> translation path rather than
/// any one layer of it. `Radio` carries no `semantic_label` builder, so the
/// query is by role rather than by label.
fn announced_roles(radio: Radio<&'static str>) -> Vec<Role> {
    let mut laid = lay_out(themed(radio), constraints());
    laid.enable_semantics();
    laid.pump();
    laid.a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect()
}

#[test]
fn a_mounted_radio_announces_as_a_radio_button() {
    // Issue #1117: `Radio::new(value, group_value)` makes a radio a
    // mutually-exclusive group member by construction (`is_selected` derives
    // from `group_value`), so a node announcing as a checkbox is announcing
    // the wrong control to assistive technology.
    let roles = announced_roles(Radio::new("spring", Some("spring")).on_changed(|_| {}));
    assert!(
        roles.contains(&Role::RadioButton),
        "a mounted Radio must announce as a radio button, got {roles:?}",
    );
}

#[test]
fn a_mounted_radio_does_not_announce_as_a_checkbox() {
    let roles = announced_roles(Radio::new("spring", Some("spring")).on_changed(|_| {}));
    assert!(
        !roles.contains(&Role::CheckBox),
        "a Radio announcing as a checkbox is the #1117 defect, got {roles:?}",
    );
}

#[test]
fn an_unselected_group_member_announces_as_a_radio_button() {
    // Group membership does not depend on which member is currently selected:
    // every member of the group is a radio, selected or not.
    let roles = announced_roles(Radio::new("summer", Some("spring")).on_changed(|_| {}));
    assert!(
        roles.contains(&Role::RadioButton),
        "an unselected group member is still a radio button, got {roles:?}",
    );
}

#[test]
fn a_radio_without_a_tap_handler_still_announces_as_a_radio_button() {
    // A missing `on_changed` makes the radio non-interactive; it does not
    // change what kind of control it is, and a screen reader still has to
    // announce it as a radio.
    let roles = announced_roles(Radio::new("spring", Some("spring")));
    assert!(
        roles.contains(&Role::RadioButton),
        "a disabled Radio is still a radio button, got {roles:?}",
    );
}

#[test]
fn a_radio_nested_under_an_annotated_ancestor_still_announces_as_a_radio_button() {
    // The absorbed composition, and the one a radio mounted as the render root
    // cannot reach: a root forms its own node (`is_root`), so nothing can carry
    // its flags alongside an ancestor's. Nest it under a `Semantics` that
    // publishes `button(true)` and no state flag, and the two configurations
    // merge onto one node carrying `IsButton` *and* the checked/group flags.
    // Role resolution must pick the specific one.
    //
    // Note what this fixture is *not*: it is not `ListTile`'s shape. A real
    // `ListTile` also publishes `.enabled(..)`, which overlaps the radio's own
    // `HasEnabledState`; `is_compatible_with` treats any overlap as a conflict,
    // so the two never merge — and because they do not merge, the radio keeps a
    // node of its own that announces as a radio, pinned by
    // `a_radio_inside_a_list_tile_announces_as_a_radio_button` in
    // `tests/list_tile.rs` (which mounts under the `MediaQuery` a bare mount
    // lacks). Do not read this test as covering the tile; it pins the absorbed
    // composition, where the two configurations *do* merge and the cascade order
    // is what decides.
    let tree = Semantics::new()
        .button(true)
        .child(Radio::new("spring", Some("spring")).on_changed(|_| {}));
    let mut laid = lay_out(Theme::new(ThemeData::light(), tree), constraints());
    laid.enable_semantics();
    laid.pump();

    let roles: Vec<Role> = laid
        .a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect();

    assert!(
        roles.contains(&Role::RadioButton),
        "an absorbed Radio must still resolve to a radio button, got {roles:?}",
    );
    assert!(
        !roles.contains(&Role::CheckBox),
        "an absorbed Radio must not resolve to a checkbox, got {roles:?}",
    );
}

#[test]
fn tap_on_an_unselected_radio_fires_on_changed_with_its_own_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            Radio::new("summer", Some("spring")).on_changed(move |next| {
                *recorder.borrow_mut() = Some(next);
            }),
        ),
        constraints(),
    );

    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);

    assert_eq!(
        *observed.borrow(),
        Some("summer"),
        "tapping an unselected radio must fire on_changed with its own value",
    );
}

#[test]
fn tap_on_an_already_selected_radio_is_a_no_op() {
    let observed: Rc<RefCell<Option<&'static str>>> = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            Radio::new("spring", Some("spring")).on_changed(move |next| {
                *recorder.borrow_mut() = Some(next);
            }),
        ),
        constraints(),
    );

    laid.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);

    assert_eq!(
        *observed.borrow(),
        None,
        "tapping an already-selected radio must not fire on_changed",
    );
}

#[test]
fn disabled_radio_swallows_a_tap_then_resyncs_once_a_handler_is_added() {
    // Same "handler-removal resync" class `tests/checkbox.rs`/
    // `tests/switch.rs` prove for their own controls.
    let taps = Rc::new(RefCell::new(0_u32));

    let laid_disabled = lay_out(themed(Radio::new("summer", Some("spring"))), constraints());
    laid_disabled.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid_disabled.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    // No on_changed at all: nothing to observe going wrong beyond "does not
    // panic" — the InkWell-level swallow behavior itself is already proven
    // by `tests/ink_well.rs`'s disabled-state coverage.

    let mut laid_enabled = laid_disabled;
    let counter = Rc::clone(&taps);
    laid_enabled.pump_widget(themed(Radio::new("summer", Some("spring")).on_changed(
        move |_| {
            *counter.borrow_mut() += 1;
        },
    )));
    laid_enabled.dispatch_pointer_down(TAP_TARGET / 2.0, TAP_TARGET / 2.0);
    laid_enabled.dispatch_pointer_up(TAP_TARGET / 2.0, TAP_TARGET / 2.0);

    assert_eq!(
        *taps.borrow(),
        1,
        "adding on_changed on rebuild must make the very next tap interactive",
    );
}
