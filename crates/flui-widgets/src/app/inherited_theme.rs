//! [`InheritedTheme`] — the `wrap`-only contract for themes.
//!
//! ## Deferred: `capture` / `capture_all`
//!
//! A `capture`/`capture_all` pair and a captured-themes value would
//! walk the element tree between two `BuildContext`s to freeze a set of
//! ambient themes for a widget that is about to be shown in a *different*
//! part of the tree (a new route, an overlay) than the one it was built in —
//! `Navigator`'s route-push machinery is the canonical caller. FLUI's
//! `Overlay`/`Navigator` do not yet re-parent a subtree across such a
//! boundary in a way that needs this, so `capture`/`capture_all` are cut here
//! as speculative (zero consumers) rather than built unused. Adding them is
//! a named follow-up for the material-`Overlay` unit, once a concrete
//! consumer exists to pin the API against.

use flui_view::{BoxedView, BuildContext, InheritedView};

/// An [`InheritedView`] that defines visual properties (colors, text
/// styles, …) which the subtree it wraps depends on, and that knows how to
/// re-wrap an arbitrary child in a fresh copy of itself.
///
/// [`wrap`](Self::wrap) is the primitive a future capture mechanism (see the
/// module docs) would build on: given some `child`, produce a widget that
/// provides this theme to it — used when a widget must be shown outside the
/// subtree it was originally built in, but should still see the ambient
/// theme from where it *was* built.
///
/// Theme capture is deferred (see module docs).
pub trait InheritedTheme: InheritedView {
    /// Return a widget that wraps `child` in a fresh copy of this theme.
    ///
    /// `ctx` is part of the signature for the eventual `capture` call site,
    /// even though every implementation so far ignores it.
    fn wrap(&self, ctx: &dyn BuildContext, child: BoxedView) -> BoxedView;
}
