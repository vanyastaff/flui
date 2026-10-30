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
    flui_widgets::GestureAlternatives::try_new(flui_widgets::Alternative::ContextMenu(
        flui_widgets::MenuSpec::new(
            flui_widgets::GestureCommand::new("Dismiss", Dismiss),
            |_cx, _request| {},
        ),
    ))
    .expect("one labelled command")
}

/// Alternatives for an `InteractiveViewer` under test whose alternatives are
/// not what the test is about: zoom commands in a context menu nobody
/// opens, so the viewer keeps its whole row.
#[allow(dead_code)]
pub fn viewer_alternatives() -> flui_widgets::GestureAlternatives {
    flui_widgets::GestureAlternatives::try_new(flui_widgets::Alternative::ContextMenu(
        flui_widgets::MenuSpec::new(
            flui_widgets::GestureCommand::new("Zoom in", flui_widgets::ZoomIntent::In),
            |_cx, _request| {},
        )
        .item(flui_widgets::GestureCommand::new(
            "Zoom out",
            flui_widgets::ZoomIntent::Out,
        )),
    ))
    .expect("two labelled commands")
}

/// Alternatives for a `PageView` under test whose alternatives are not what
/// the test is about: page commands in a context menu nobody opens, so the
/// pager keeps its whole row.
#[allow(dead_code)]
pub fn pager_alternatives() -> flui_widgets::GestureAlternatives {
    flui_widgets::GestureAlternatives::try_new(flui_widgets::Alternative::ContextMenu(
        flui_widgets::MenuSpec::new(
            flui_widgets::GestureCommand::new("Previous page", flui_widgets::PageIntent::Previous),
            |_cx, _request| {},
        )
        .item(flui_widgets::GestureCommand::new(
            "Next page",
            flui_widgets::PageIntent::Next,
        )),
    ))
    .expect("two labelled commands")
}
