//! [`app_root`]: the one place the catalog's application-wide keyboard
//! bindings are installed above a presentation's root.

use flui_view::prelude::*;

use crate::interaction::DefaultFocusTraversal;

/// Wrap `view` in the catalog's standard keyboard bindings, as every
/// presentation root an application shows is wrapped.
///
/// The UI runtime roots the focus tree itself
/// ([`FocusTreeRoot`](flui_view::environment::FocusTreeRoot)) but installs
/// no key bindings: Tab and Shift+Tab traversal, activation, dismissal, Back
/// and the clipboard chords ([`DefaultFocusTraversal`]) are widget-catalog
/// policy. Every composition root passes its root view through this function
/// before attaching it: `flui-app`'s runners for each window they install,
/// and `flui-testing`'s `HeadlessHost` for each window it attaches. One
/// function keeps a second, diverging copy of the application root from
/// appearing in a runner.
///
/// An embedder that drives a UI runtime without `flui-app` calls it the same
/// way; a root attached without it has a focus tree but no keyboard
/// traversal.
#[must_use]
pub fn app_root(view: impl IntoView) -> DefaultFocusTraversal {
    DefaultFocusTraversal::new(view)
}
