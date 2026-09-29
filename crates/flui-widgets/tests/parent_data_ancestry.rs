//! Negative `ParentDataView` ancestry diagnostics.
//!
//! Misplacing `Expanded` under `Stack` or `Positioned` under `Row` must fail at
//! the element-tree parent-data attach seam — before layout's
//! `BoxLayoutCtx::from_erased` TypeId assert — with a message that names the
//! offending view, the typical ancestor family, and the actual render parent.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::common::{lay_out, tight};
use flui_widgets::row;
use flui_widgets::{Expanded, SizedBox, Stack};

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

pub(crate) fn expanded_under_stack_panics_at_attach_with_ancestry_diagnostic() {
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
