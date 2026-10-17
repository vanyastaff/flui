//! Thin re-export of the canonical headless harness.
//!
//! The harness itself lives in [`flui_testing::widgets`], so that
//! `flui-material` and `flui-cupertino` share the exact same
//! mount/pointer/clock machinery instead of drifted copies. Test files keep
//! importing through `crate::common::…`; only the definition moved.

pub use flui_testing::widgets::*;

#[allow(dead_code)]
pub mod cases;
#[allow(dead_code)]
pub mod child_process;

/// Alternatives for a `Dismissible` under test whose alternatives are not
/// what the test is about: a context menu nobody opens, so the card keeps
/// its whole row. Its long press competes with a finger's drag, not a
/// mouse's.
#[allow(dead_code)]
pub fn dismiss_alternatives() -> flui_widgets::GestureAlternatives {
    struct Dismiss;
    impl flui_widgets::Intent for Dismiss {}
    flui_widgets::GestureAlternatives::new(flui_widgets::Alternative::ContextMenu(
        flui_widgets::MenuSpec::new(
            flui_widgets::GestureCommand::new("Dismiss", Dismiss),
            |_cx, _request| {},
        ),
    ))
    .expect("one labelled command")
}
