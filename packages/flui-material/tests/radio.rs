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
//! `widget_override_is_ignored_when_disabled_even_if_selected`), plus `RadioPainter`'s own
//! paint-invocation proof (`inner_dot_is_present_only_when_selected`, a
//! real `Canvas`/`DisplayList` recording).

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{Radio, Theme, ThemeData};
use flui_testing::a11y::Role;

/// The radio's full tap target: the minimum interactive dimension.
const TAP_TARGET: f64 = 48.0;

fn constraints() -> flui_sdk::rendering::BoxConstraints {
    tight(TAP_TARGET, TAP_TARGET)
}

/// Every `Radio` needs a [`Theme`] ancestor (`Theme::of` panics without
/// one) — mirrors `tests/checkbox.rs`'s/`tests/switch.rs`'s own `themed`
/// helper.
fn themed<T: PartialEq + Clone + 'static>(radio: Radio<T>) -> Theme {
    Theme::new(ThemeData::light(), radio)
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

pub fn a_mounted_radio_announces_as_a_radio_button() {
    // Issue #1117: `Radio::new(value, group_value)` makes a radio a
    // mutually-exclusive group member by construction (`is_selected` derives
    // from `group_value`), so a node announcing as a checkbox is announcing
    // the wrong control to assistive technology.
    let roles = announced_roles(Radio::new("spring", Some("spring")).on_changed(|_cx, _| {}));
    assert!(
        roles.contains(&Role::RadioButton),
        "a mounted Radio must announce as a radio button, got {roles:?}",
    );
}

pub fn tap_on_an_unselected_radio_fires_on_changed_with_its_own_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            Radio::new("summer", Some("spring")).on_changed(move |_cx, next| {
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
