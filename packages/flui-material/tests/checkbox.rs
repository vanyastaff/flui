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
//! **Not covered here** (see `checkbox.rs`'s own unit test instead, since
//! it needs no render tree): `active_color`'s `!Disabled && Selected` gate,
//! exercised directly against `resolve_checkbox_fill_color` (extracted out
//! of `build` specifically so this cascade is unit-testable without
//! mounting a widget tree; see
//! `widget_override_is_ignored_when_disabled_even_if_selected`). The illegal
//! `(None, tristate: false)` pair is unrepresentable at the type level
//! (`CheckboxMode`); the a11y case below proves indeterminate still exports
//! `Toggled::Mixed`.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{Checkbox, Theme, ThemeData};
use flui_testing::a11y::Toggled;

/// The checkbox's full tap target — the minimum interactive dimension
/// (`48.0`), the branch `Checkbox` always takes in this V1 (no
/// `materialTapTargetSize` override yet).
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

pub fn tristate_cycle_survives_a_rebuild_between_each_tap() {
    // The tristate cycle is false -> true -> null -> false. Each tap here rebuilds
    // the tree with the previously-observed value (mirroring how a real
    // caller's `setState` re-renders `Checkbox` with the new `value` from
    // `onChanged`), proving the cycle end to end through real dispatch, not
    // just `Checkbox::next_value`'s unit-level pure function.
    let observed: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));

    let build = |value: Option<bool>, sink: Rc<RefCell<Option<bool>>>| {
        themed(Checkbox::tristate(value).on_changed(move |_ctx, next| {
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
        themed(checkbox.semantic_label(label).on_changed(|_ctx, _| {})),
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

pub fn indeterminate_tristate_exports_mixed_semantics() {
    // Issue #1102 AC: valid tristate `None` paints the dash
    // AND exports mixed — never the old release hole of dash + unchecked.
    assert_eq!(
        announced_toggled(Checkbox::tristate(None), "indeterminate"),
        Some(Toggled::Mixed),
    );
}

/// Each toggle and each pressable chip is one node, whatever its state: its
/// role, its checked state,
/// its label when it has one, disabled without a handler, and with one the
/// click and the focus a screen reader acts on. `Switch`, `Radio` and the
/// chips describe themselves on their well as `Checkbox` does, so they are
/// rows here too.
pub fn toggles_and_chips_are_one_node_with_their_state() {
    use flui_material::{Chip, FilterChip, Radio, Switch};
    use flui_sdk::view::prelude::*;
    use flui_sdk::widgets::Text;
    use flui_testing::a11y::{Action, Role};

    /// What the row is, the node it must form, and the control.
    struct Row {
        name: &'static str,
        role: Role,
        toggled: Option<Toggled>,
        enabled: bool,
        label: Option<&'static str>,
        view: fn() -> BoxedView,
    }
    let rows = [
        Row {
            name: "checkbox off",
            role: Role::CheckBox,
            toggled: Some(Toggled::False),
            enabled: true,
            label: None,
            view: || Checkbox::new(false).on_changed(|_ctx, _value| {}).boxed(),
        },
        Row {
            name: "checkbox labelled",
            role: Role::CheckBox,
            toggled: Some(Toggled::True),
            enabled: true,
            label: Some("Agree"),
            view: || {
                Checkbox::new(true)
                    .on_changed(|_ctx, _value| {})
                    .semantic_label("Agree")
                    .boxed()
            },
        },
        Row {
            name: "checkbox disabled",
            role: Role::CheckBox,
            toggled: Some(Toggled::True),
            enabled: false,
            label: None,
            view: || Checkbox::new(true).boxed(),
        },
        Row {
            name: "switch on",
            role: Role::Switch,
            toggled: Some(Toggled::True),
            enabled: true,
            label: None,
            view: || Switch::new(true).on_changed(|_ctx, _value| {}).boxed(),
        },
        Row {
            name: "switch disabled",
            role: Role::Switch,
            toggled: Some(Toggled::False),
            enabled: false,
            label: None,
            view: || Switch::new(false).boxed(),
        },
        Row {
            name: "radio selected",
            role: Role::RadioButton,
            toggled: Some(Toggled::True),
            enabled: true,
            label: None,
            view: || {
                Radio::new("a", Some("a"))
                    .on_changed(|_ctx, _value| {})
                    .boxed()
            },
        },
        Row {
            name: "radio disabled",
            role: Role::RadioButton,
            toggled: Some(Toggled::False),
            enabled: false,
            label: None,
            view: || Radio::new("a", Some("b")).boxed(),
        },
        Row {
            name: "chip",
            role: Role::Button,
            toggled: None,
            enabled: true,
            label: Some("Tag"),
            view: || Chip::new(Text::new("Tag")).on_pressed(|_ctx| {}).boxed(),
        },
        Row {
            name: "chip disabled",
            role: Role::Button,
            toggled: None,
            enabled: false,
            label: Some("Tag"),
            view: || {
                Chip::new(Text::new("Tag"))
                    .on_pressed(|_ctx| {})
                    .enabled(false)
                    .boxed()
            },
        },
        Row {
            name: "filter chip selected",
            role: Role::Button,
            toggled: Some(Toggled::True),
            enabled: true,
            label: Some("Tag"),
            view: || {
                FilterChip::new(Text::new("Tag"))
                    .selected(true)
                    .on_selected(|_ctx, _selected| {})
                    .boxed()
            },
        },
        Row {
            name: "filter chip disabled",
            role: Role::Button,
            toggled: Some(Toggled::False),
            enabled: false,
            label: Some("Tag"),
            view: || FilterChip::new(Text::new("Tag")).boxed(),
        },
    ];

    let mut failed = Vec::new();
    for row in rows {
        let mut laid = lay_out(
            Theme::new(ThemeData::light(), (row.view)()),
            tight(200.0, 56.0),
        );
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let nodes: Vec<_> = tree
            .nodes()
            .filter(|node| node.role() != Role::Window)
            .collect();
        let [node] = nodes.as_slice() else {
            failed.push(format!(
                "{}: {} nodes\n{}",
                row.name,
                nodes.len(),
                tree.describe()
            ));
            continue;
        };
        let formed = (
            node.role(),
            node.toggled(),
            node.label(),
            node.is_disabled(),
            node.supports_action(Action::Click),
            node.supports_action(Action::Focus),
            node.child_ids().len(),
        );
        let wanted = (
            row.role,
            row.toggled,
            row.label,
            !row.enabled,
            row.enabled,
            row.enabled,
            0,
        );
        if formed != wanted {
            failed.push(format!("{}: {formed:?}, not {wanted:?}", row.name));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
