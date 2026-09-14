//! Negative `ParentDataView` ancestry diagnostics.
//!
//! Misplacing `Expanded` under `Stack` or `Positioned` under `Row` must fail at
//! the element-tree parent-data attach seam — before layout's
//! `BoxLayoutCtx::from_erased` TypeId assert — with a message that names the
//! offending view, the typical ancestor family, and the actual render parent.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::common::{lay_out, tight};
use flui_widgets::row;
use flui_widgets::{Container, Expanded, Positioned, Row, SizedBox, Stack};

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

#[test]
fn expanded_under_stack_panics_at_attach_with_ancestry_diagnostic() {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        lay_out(
            Stack::new(row![Expanded::new(SizedBox::new(20.0, 20.0))]),
            tight(100.0, 100.0),
        );
    }));
    let message = panic_message(outcome.expect_err("Expanded under Stack must panic"));
    assert!(
        message.contains("Incorrect use of ParentDataView `Expanded`"),
        "missing provider name: {message}"
    );
    assert!(
        message.contains("Flex (Row, Column, or Flex)"),
        "missing typical ancestor: {message}"
    );
    assert!(
        message.contains("RenderStack") || message.contains("Stack"),
        "missing actual render parent: {message}"
    );
    assert!(
        !message.contains("BoxLayoutCtx::from_erased"),
        "must fail before layout TypeId assert: {message}"
    );
}

#[test]
fn positioned_under_row_panics_at_attach_with_ancestry_diagnostic() {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        lay_out(
            Row::new(row![Positioned::new(SizedBox::new(20.0, 20.0)).left(4.0)]),
            tight(100.0, 100.0),
        );
    }));
    let message = panic_message(outcome.expect_err("Positioned under Row must panic"));
    assert!(
        message.contains("Incorrect use of ParentDataView `Positioned`"),
        "missing provider name: {message}"
    );
    assert!(
        message.contains("Stack"),
        "missing typical ancestor: {message}"
    );
    assert!(
        message.contains("RenderFlex") || message.contains("Flex") || message.contains("Row"),
        "missing actual render parent: {message}"
    );
    assert!(
        !message.contains("BoxLayoutCtx::from_erased"),
        "must fail before layout TypeId assert: {message}"
    );
}

/// Flutter's identity `Container` builds to the child, so
/// `Row → Container → Expanded` is legal there. Mapping decision 15 keeps a
/// `RenderContainer` in that slot on purpose (stable child for #161698);
/// `Expanded` therefore sees `BoxParentData` and must fail at attach. Put
/// `Expanded` around the container instead.
#[test]
fn identity_container_between_flex_and_expanded_is_not_parent_data_transparent() {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        lay_out(
            Row::new(row![
                Container::new().child(Expanded::new(SizedBox::new(20.0, 20.0)))
            ]),
            tight(100.0, 100.0),
        );
    }));
    let message = panic_message(
        outcome.expect_err("identity Container wrapping Expanded under Row must panic"),
    );
    assert!(
        message.contains("Incorrect use of ParentDataView `Expanded`"),
        "missing provider name: {message}"
    );
    assert!(
        message.contains("Flex (Row, Column, or Flex)"),
        "missing typical ancestor: {message}"
    );
    assert!(
        message.contains("RenderContainer") || message.contains("Container"),
        "missing actual render parent: {message}"
    );
    assert!(
        !message.contains("BoxLayoutCtx::from_erased"),
        "must fail before layout TypeId assert: {message}"
    );
}

/// The supported wrap: `Expanded` applies `FlexParentData` to the
/// `RenderContainer`, which Flex accepts.
#[test]
fn expanded_around_identity_container_lays_out() {
    let laid = lay_out(
        Row::new(row![
            Expanded::new(Container::new().child(SizedBox::new(20.0, 20.0))),
            SizedBox::new(20.0, 20.0),
        ]),
        tight(100.0, 20.0),
    );
    assert_eq!(laid.size(laid.root()).width.get(), 100.0);
}
