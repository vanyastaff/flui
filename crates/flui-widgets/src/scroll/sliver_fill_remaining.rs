//! [`SliverFillRemaining`], [`SliverFillRemainingWithScrollable`], and
//! [`SliverFillRemainingAndOverscroll`] — slivers that fill the remaining
//! viewport space after preceding slivers.
//!
//! These map to the three `RenderSliverFillRemaining*` render objects — Flutter's
//! `SliverFillRemaining` collapses the `hasScrollBody`/`fillOverscroll` bool
//! combinations, but FLUI exposes each as a distinct type (illegal states
//! unrepresentable):
//!
//! | Widget                                | Render object                             | Flutter flags |
//! |---------------------------------------|-------------------------------------------|---------------|
//! | [`SliverFillRemaining`]               | `RenderSliverFillRemaining`               | `hasScrollBody=false` |
//! | [`SliverFillRemainingWithScrollable`] | `RenderSliverFillRemainingWithScrollable` | `hasScrollBody=true` (default) |
//! | [`SliverFillRemainingAndOverscroll`]  | `RenderSliverFillRemainingAndOverscroll`  | `hasScrollBody=false, fillOverscroll=true` |

use flui_objects::{
    RenderSliverFillRemaining, RenderSliverFillRemainingAndOverscroll,
    RenderSliverFillRemainingWithScrollable,
};
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

// ============================================================================
// SliverFillRemaining
// ============================================================================

/// A sliver that sizes its **non-scrollable** box child to fill the remaining
/// main-axis space in the viewport after all preceding slivers.
///
/// When the child is intrinsically larger than the remaining space the sliver
/// expands to the child's max-intrinsic main-axis extent; the viewport then
/// becomes scrollable to expose the overflow.
///
/// For a child that is itself a scrollable widget (e.g.
/// [`SingleChildScrollView`], [`ListView`]) use
/// [`SliverFillRemainingWithScrollable`] instead.
///
/// Flutter parity: `widgets/sliver.dart` `SliverFillRemaining` with
/// `hasScrollBody = false, fillOverscroll = false` over
/// `RenderSliverFillRemaining`.
///
/// [`SingleChildScrollView`]: crate::SingleChildScrollView
/// [`ListView`]: crate::ListView
#[derive(Clone, Debug, Default)]
pub struct SliverFillRemaining {
    child: Child,
}

impl SliverFillRemaining {
    /// Create a fill-remaining sliver with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the box child to fill remaining viewport space.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverFillRemaining {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFillRemaining;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverFillRemaining::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        // No configurable fields on RenderSliverFillRemaining.
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(SliverFillRemaining);

// ============================================================================
// SliverFillRemainingWithScrollable
// ============================================================================

/// A sliver that sizes its child — which must be a **scrollable** widget — to
/// the remaining paint extent of the viewport.
///
/// Unlike [`SliverFillRemaining`], this variant sizes to the remaining *paint*
/// extent (not intrinsic extent) and does not expand when the child is larger;
/// it is meant to host a self-contained scroller (e.g. `ListView`,
/// `SingleChildScrollView`).
///
/// Flutter parity: `widgets/sliver.dart` `SliverFillRemaining` with
/// `hasScrollBody = true` (the default) over
/// `RenderSliverFillRemainingWithScrollable`.
///
/// [`SingleChildScrollView`]: crate::SingleChildScrollView
/// [`ListView`]: crate::ListView
#[derive(Clone, Debug, Default)]
pub struct SliverFillRemainingWithScrollable {
    child: Child,
}

impl SliverFillRemainingWithScrollable {
    /// Create a fill-remaining-with-scrollable sliver with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the scrollable box child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverFillRemainingWithScrollable {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFillRemainingWithScrollable;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverFillRemainingWithScrollable::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        // No configurable fields on RenderSliverFillRemainingWithScrollable.
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(SliverFillRemainingWithScrollable);

// ============================================================================
// SliverFillRemainingAndOverscroll
// ============================================================================

/// A non-scrollable fill-remaining sliver that additionally grows into the
/// viewport's **overscroll** area (beyond the remaining paint extent), so the
/// child paints across the bounce/stretch region on platforms that overscroll.
///
/// Like [`SliverFillRemaining`] the child is a non-scrollable box; the
/// difference is the extra overscroll extent (`fillOverscroll = true`).
///
/// Flutter parity: `widgets/sliver.dart` `SliverFillRemaining` with
/// `hasScrollBody = false, fillOverscroll = true` over
/// `RenderSliverFillRemainingAndOverscroll`.
#[derive(Clone, Debug, Default)]
pub struct SliverFillRemainingAndOverscroll {
    child: Child,
}

impl SliverFillRemainingAndOverscroll {
    /// Create an overscroll-aware fill-remaining sliver with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the box child to fill the remaining viewport space (plus overscroll).
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverFillRemainingAndOverscroll {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFillRemainingAndOverscroll;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverFillRemainingAndOverscroll::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        // No configurable fields on RenderSliverFillRemainingAndOverscroll.
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(SliverFillRemainingAndOverscroll);
