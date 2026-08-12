//! [`SliverPersistentHeader`] — a sliver whose child is rebuilt as the header
//! collapses, pinnable and floatable.

use std::rc::Rc;

use flui_view::element::{
    FloatingPersistentHeaderView, FloatingPinnedPersistentHeaderView, PinnedPersistentHeaderView,
    ScrollingPersistentHeaderView, SliverPersistentHeaderDelegate,
};
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView, ViewExt};

/// A sliver that keeps a header of delegate-controlled extent at the leading
/// edge, rebuilding its content as the header collapses from
/// [`max_extent`](SliverPersistentHeaderDelegate::max_extent) toward
/// [`min_extent`](SliverPersistentHeaderDelegate::min_extent).
///
/// The [`SliverPersistentHeaderDelegate`]'s `build` receives the header's real
/// published collapse state — `shrink_offset` and `overlaps_content` — every
/// time it changes, in the same frame it changed (the build-during-layout
/// fixpoint `LayoutBuilder` also rides). This is the foundation collapsing app
/// bars sit on.
///
/// `pinned` keeps the collapsed header on screen; `floating` re-reveals it on
/// any scroll toward the start. The four combinations map to four distinct
/// render objects, so flipping a flag replaces the element rather than
/// mutating it — exactly Flutter's shape, where each combination is its own
/// internal widget.
///
/// # Deferred, deliberately
///
/// Snap (`FloatingHeaderSnapConfiguration`) and over-scroll stretch
/// (`OverScrollHeaderStretchConfiguration`) are implemented at the render
/// layer but not yet reachable from this widget: both need an
/// `AnimationController` / vsync plumbed through the element, which is its
/// own slice. Until then floating headers scroll freely and never snap.
///
/// Flutter parity: `widgets/sliver_persistent_header.dart`
/// `SliverPersistentHeader`.
#[derive(Clone, StatelessView)]
pub struct SliverPersistentHeader {
    delegate: Rc<dyn SliverPersistentHeaderDelegate>, // PORT-CHECK-OK-DYN: carries flui-view's SharedHeaderDelegate erasure (justified at its declaration) through the facade
    pinned: bool,
    floating: bool,
}

impl SliverPersistentHeader {
    /// A scrolling (neither pinned nor floating) header over `delegate`.
    pub fn new(delegate: impl SliverPersistentHeaderDelegate + 'static) -> Self {
        Self {
            delegate: Rc::new(delegate),
            pinned: false,
            floating: false,
        }
    }

    /// Keep the collapsed header visible at the leading edge instead of
    /// letting it scroll away.
    #[must_use]
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Re-reveal the header as soon as the user scrolls toward the start,
    /// regardless of how far away it was.
    #[must_use]
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }
}

impl std::fmt::Debug for SliverPersistentHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverPersistentHeader")
            .field("pinned", &self.pinned)
            .field("floating", &self.floating)
            .field("min_extent", &self.delegate.min_extent())
            .field("max_extent", &self.delegate.max_extent())
            .finish_non_exhaustive()
    }
}

impl StatelessView for SliverPersistentHeader {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let delegate = Rc::clone(&self.delegate);
        // Four distinct view TYPES, not one view with flags: the reconciler
        // answers a flag flip by replacing the element, which is the only
        // correct answer when each variant owns a different render object.
        match (self.pinned, self.floating) {
            (false, false) => ScrollingPersistentHeaderView::new(delegate).boxed(),
            (true, false) => PinnedPersistentHeaderView::new(delegate).boxed(),
            (false, true) => FloatingPersistentHeaderView::new(delegate).boxed(),
            (true, true) => FloatingPinnedPersistentHeaderView::new(delegate).boxed(),
        }
    }
}
