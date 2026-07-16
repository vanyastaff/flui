//! [`Theme`] — publishes [`ThemeData`] to a subtree via FLUI's
//! inherited-data mechanism.
//!
//! Flutter parity: `material/theme.dart` `Theme` (oracle tag `3.44.0`).
//! Implements `flui-widgets`' [`InheritedTheme`] trait so a future
//! capture/re-parent mechanism (see that trait's module docs) can wrap a
//! `Theme` the same way it wraps any other ambient theme.

use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};
use flui_widgets::InheritedTheme;

use crate::theme_data::ThemeData;

/// Provides [`ThemeData`] to its subtree via FLUI's inherited-data mechanism.
///
/// Place a `Theme` near the root of the application subtree to supply a
/// consistent Material visual identity. Any descendant reads the current
/// theme with [`Theme::of`] / [`Theme::maybe_of`].
///
/// Flutter parity: `Theme` (`material/theme.dart`, oracle tag `3.44.0`).
///
/// # Example
///
/// ```rust
/// use flui_material::{Theme, ThemeData};
/// use flui_widgets::SizedBox;
///
/// let _themed = Theme::new(ThemeData::dark(), SizedBox::shrink());
/// ```
#[derive(Clone)]
pub struct Theme {
    /// The style data this node provides to descendants.
    data: ThemeData,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl Theme {
    /// Wrap `child` in a `Theme` that provides `data` to all descendants.
    #[must_use]
    pub fn new(data: ThemeData, child: impl IntoView) -> Self {
        Self {
            data,
            child: child.into_view().boxed(),
        }
    }

    /// Access the [`ThemeData`] from the nearest ancestor [`Theme`],
    /// registering a dependency so this element rebuilds when the theme
    /// changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no [`Theme`] ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    ///
    /// **Documented divergence from Flutter**: the oracle's `Theme.of`
    /// (`material/theme.dart`, oracle tag `3.44.0`) never panics — with no
    /// `Theme` ancestor it falls back to `ThemeData.fallback()` (baked in as
    /// `_kFallbackTheme`). This crate does not implement `ThemeData::fallback`
    /// (every FLUI app is expected to wrap its tree in a `Theme`, so a
    /// themeless default has no consumer yet), so the missing-ancestor case
    /// panics instead — a defensible choice for surfacing the mistake loudly
    /// during development, but a deliberate divergence, not a port of the
    /// oracle's fallback behavior.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> ThemeData {
        Self::maybe_of(ctx).expect(
            "Theme::of called with no Theme ancestor in the tree — wrap the subtree in a \
             Theme, or use Theme::maybe_of with a caller-chosen default",
        )
    }

    /// Look up the nearest ancestor [`Theme`]'s data, registering a
    /// dependency. Returns `None` if there is no [`Theme`] ancestor.
    ///
    /// The oracle's `Theme` has no `maybeOf` — its `Theme.of` never returns
    /// `null` (see [`of`](Self::of)'s doc comment on the fallback it uses
    /// instead), so there is no method to port here. `maybe_of` is this
    /// crate's own non-panicking counterpart to [`of`](Self::of)'s panicking
    /// behavior, not a parity port.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<ThemeData> {
        ctx.depend_on::<Self, _>(|t| t.data.clone())
    }
}

impl std::fmt::Debug for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Theme")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl InheritedView for Theme {
    type Data = ThemeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Rebuild descendants whenever any style field changes — same
        // contract as Flutter's `ThemeData.==`.
        self.data != old.data
    }
}

impl_inherited_view!(Theme);

impl InheritedTheme for Theme {
    fn wrap(&self, _ctx: &dyn BuildContext, child: BoxedView) -> BoxedView {
        Theme::new(self.data.clone(), child).boxed()
    }
}

#[cfg(test)]
mod tests {
    use flui_widgets::SizedBox;

    use super::*;

    #[test]
    fn new_stores_data_and_child() {
        let theme = Theme::new(ThemeData::dark(), SizedBox::shrink());
        assert_eq!(theme.data, ThemeData::dark());
    }

    #[test]
    fn update_should_notify_true_when_data_differs() {
        let a = Theme::new(ThemeData::light(), SizedBox::shrink());
        let b = Theme::new(ThemeData::dark(), SizedBox::shrink());
        assert!(a.update_should_notify(&b));
    }

    #[test]
    fn update_should_notify_false_when_data_equal() {
        let a = Theme::new(ThemeData::light(), SizedBox::shrink());
        let b = Theme::new(ThemeData::light(), SizedBox::shrink());
        assert!(!a.update_should_notify(&b));
    }

    #[test]
    fn debug_format_does_not_panic() {
        let theme = Theme::new(ThemeData::light(), SizedBox::shrink());
        let rendered = format!("{theme:?}");
        assert!(rendered.contains("Theme"));
    }
}
