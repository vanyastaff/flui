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
//! **Not covered here** (see `radio.rs`'s own unit tests instead): the M3
//! default token-table branch order/combined-state pins, and per-field
//! theme-tier-beats-default resolution — both pure-function unit tests, no
//! render tree needed.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, size, tight};
use flui_material::{Radio, Theme, ThemeData};

/// The radio's full tap target. Flutter parity: `kMinInteractiveDimension`.
const TAP_TARGET: f32 = 48.0;

fn constraints() -> flui_rendering::constraints::BoxConstraints {
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

    let semantics = laid
        .find_by_render_type("RenderSemanticsAnnotations")
        .expect("Radio must mount a Semantics wrapper");
    assert_eq!(laid.size(semantics), size(TAP_TARGET, TAP_TARGET));
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
