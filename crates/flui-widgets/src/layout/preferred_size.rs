//! [`PreferredSizeView`] and [`PreferredSize`] — a view that can advertise the
//! size it would prefer if it were otherwise unconstrained.
//!
//! Flutter parity: `widgets/preferred_size.dart` `PreferredSizeWidget` /
//! `PreferredSize` (oracle tag `3.44.0`).

use flui_types::Size;
use flui_view::prelude::*;

/// A view that can report the size it would prefer if it were otherwise
/// unconstrained.
///
/// A parent that needs to size a region *before* laying the child out (e.g.
/// a `Scaffold`'s `app_bar` slot, sized to its app bar's preferred height
/// plus the status-bar inset before the child ever sees a constraint) can
/// require this trait on that slot instead of a plain view. Flutter parity:
/// `PreferredSizeWidget` (`widgets/preferred_size.dart`) — `AppBar` and
/// `TabBar` implement it directly; [`PreferredSize`] adapts an arbitrary view
/// for a caller that needs the trait but has neither.
///
/// **Named divergence**: the oracle re-consults `preferredSize` lazily, at
/// the *parent's* `BuildContext` (`AppBar.preferredHeightFor` re-reads
/// `AppBarTheme.of` there, not at the `AppBar`'s own context), so a
/// component-theme change is picked up without the child rebuilding. FLUI's
/// Material substrate has no component-theme layer yet (see
/// `flui-material`'s crate-root docs, "Scope (V1 — constants-first)"), so
/// there is nothing for a lazy re-consult to observe: a caller resolves
/// [`preferred_size`](Self::preferred_size) once, at construction, and keeps
/// the resulting number. Revisit this once component themes exist.
pub trait PreferredSizeView: View {
    /// The size this view would prefer if it were otherwise unconstrained.
    ///
    /// Callers commonly read only one dimension (e.g. a `Scaffold` reads only
    /// the height) — see the trait docs.
    fn preferred_size(&self) -> Size;
}

/// Advertises a preferred size for an arbitrary child, without imposing any
/// constraint on it or otherwise affecting its layout.
///
/// Flutter parity: `widgets/preferred_size.dart` `PreferredSize`. Use this to
/// give a [`PreferredSizeView`]-requiring slot a child that does not itself
/// implement the trait — a view that already implements it directly (like
/// `flui_material::AppBar`) needs no wrapper.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_types::Size;
/// use flui_widgets::layout::PreferredSize;
/// use flui_widgets::SizedBox;
///
/// let _bar = PreferredSize::new(Size::new(px(f32::INFINITY), px(80.0)), SizedBox::shrink());
/// ```
#[derive(Clone, StatelessView)]
pub struct PreferredSize {
    preferred_size: Size,
    child: BoxedView,
}

impl PreferredSize {
    /// Wrap `child`, advertising `preferred_size` to whatever slot requires
    /// [`PreferredSizeView`].
    pub fn new(preferred_size: Size, child: impl IntoView) -> Self {
        Self {
            preferred_size,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for PreferredSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreferredSize")
            .field("preferred_size", &self.preferred_size)
            .finish_non_exhaustive()
    }
}

impl StatelessView for PreferredSize {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Flutter oracle (`preferred_size.dart`): `PreferredSize.build` just
        // returns `child` — this widget imposes no constraint of its own, it
        // only advertises `preferredSize` to whatever slot required it.
        self.child.clone()
    }
}

impl PreferredSizeView for PreferredSize {
    fn preferred_size(&self) -> Size {
        self.preferred_size
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;
    use crate::layout::SizedBox;

    #[test]
    fn preferred_size_reports_the_configured_size() {
        let size = Size::new(px(f32::INFINITY), px(80.0));
        let wrapped = PreferredSize::new(size, SizedBox::shrink());
        assert_eq!(wrapped.preferred_size(), size);
    }

    #[test]
    fn child_view_type_is_preserved_through_the_wrapper() {
        let wrapped = PreferredSize::new(Size::new(px(0.0), px(80.0)), SizedBox::new(10.0, 20.0));
        assert_eq!(
            wrapped.child.view_type_id(),
            std::any::TypeId::of::<SizedBox>(),
            "PreferredSize must store the exact child view it was given",
        );
    }
}
