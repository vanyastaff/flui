//! [`PreferredSizeView`] and [`PreferredSize`] — a view that can advertise the
//! size it would prefer if it were otherwise unconstrained.

use flui_foundation::geometry::Size;
use flui_view::prelude::*;

/// A view that can report the size it would prefer if it were otherwise
/// unconstrained.
///
/// A parent that needs to size a region *before* laying the child out (e.g.
/// a `Scaffold`'s `app_bar` slot, sized to its app bar's preferred height
/// plus the status-bar inset before the child ever sees a constraint) can
/// require this trait on that slot instead of a plain view. `AppBar` and
/// `TabBar` implement it directly; [`PreferredSize`] adapts an arbitrary view
/// for a caller that needs the trait but has neither.
///
/// **Known limit**: `preferred_size` is not re-consulted lazily at the
/// *parent's* `BuildContext`, which is what would let a component-theme
/// change be picked up without the child rebuilding. FLUI's
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
/// Use this to
/// give a [`PreferredSizeView`]-requiring slot a child that does not itself
/// implement the trait — a view that already implements it directly (like
/// `flui_material::AppBar`) needs no wrapper.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::Size;
/// use flui_widgets::layout::PreferredSize;
/// use flui_widgets::SizedBox;
///
/// let _bar = PreferredSize::new(Size::new((f64::INFINITY), 80.0), SizedBox::shrink());
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
        // Just returns `child` — this widget imposes no constraint of its
        // own, it only advertises `preferred_size` to whatever slot required it.
        self.child.clone()
    }
}

impl PreferredSizeView for PreferredSize {
    fn preferred_size(&self) -> Size {
        self.preferred_size
    }
}
