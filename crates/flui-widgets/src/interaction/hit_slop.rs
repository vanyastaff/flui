//! [`HitSlop`] — extends a child's hit area past its bounds, and
//! [`ContentShape`] — hit-tests a child by a shape instead of its box.

use flui_foundation::geometry::EdgeInsets;
use flui_objects::{HitShape, MinTarget, RenderContentShape, RenderHitSlop};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

use super::gesture_arena_scope::GestureArenaScope;

/// Makes its child hittable within an area larger than the child, as React
/// Native's `hitSlop` does: the child's box grown by [`slop`](Self::slop) on
/// each side, then to at least [`min_target`](Self::min_target) on each axis.
///
/// Layout, paint and the semantics rect are the child's; only pointers see the
/// larger area. Where areas compete, the pipeline resolves them in order:
///
/// 1. a tap that visually lands on another target goes to that target;
/// 2. otherwise the area whose child is nearest to the tap wins;
/// 3. on a tie, the topmost one does.
///
/// An area never reaches past an ancestor's bounds or through a widget drawn
/// above it that takes the tap. A tap that visually lands on content in another
/// composited layer (below a different [`RepaintBoundary`](crate::RepaintBoundary))
/// or on an [`AbsorbPointer`](crate::AbsorbPointer) barrier stays there; a tap
/// that only lands on shared ancestors — the gap between list items — goes to
/// the nearest area.
///
/// ```ignore
/// HitSlop::new()
///     .slop(EdgeInsets::all(8.0))
///     .child(GestureDetector::new().on_tap(close).child(close_icon))
/// ```
#[derive(Clone, Debug, StatelessView)]
pub struct HitSlop {
    slop: EdgeInsets,
    min_target: Option<f64>,
    child: Child,
}

impl Default for HitSlop {
    fn default() -> Self {
        Self {
            slop: EdgeInsets::ZERO,
            min_target: None,
            child: Child::empty(),
        }
    }
}

impl HitSlop {
    /// A slop area grown to the gesture settings' minimum target only.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grow the hit area by `slop` on each side. Negative or non-finite
    /// insets count as zero.
    #[must_use]
    pub fn slop(mut self, slop: EdgeInsets) -> Self {
        self.slop = slop;
        self
    }

    /// The smallest side of the hit area, in logical pixels.
    ///
    /// Defaults to the enclosing [`GestureArenaScope`]'s settings
    /// (`GestureSettings::min_target`, from the platform preset), or the
    /// framework default outside a scope. The default is read on every hit
    /// test, so a live host update applies without rebuilding this widget.
    #[must_use]
    pub fn min_target(mut self, min_target: f64) -> Self {
        self.min_target = Some(min_target);
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatelessView for HitSlop {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // The provider, not a copy of its value: the render object reads it on
        // every hit test, so a live host update needs no rebuild.
        let min_target = self.min_target.map_or_else(
            || MinTarget::Settings(GestureArenaScope::settings_or_default_of(ctx)),
            MinTarget::Fixed,
        );
        HitSlopArea {
            slop: self.slop,
            min_target,
            child: self.child.clone(),
        }
    }
}

/// The resolved [`HitSlop`]: its minimum target source chosen.
#[derive(Clone, Debug)]
struct HitSlopArea {
    slop: EdgeInsets,
    min_target: MinTarget,
    child: Child,
}

impl RenderView for HitSlopArea {
    type Protocol = BoxProtocol;
    type RenderObject = RenderHitSlop;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderHitSlop::new(self.slop, self.min_target.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_slop(self.slop) | render_object.set_min_target(self.min_target.clone())
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(HitSlopArea);

/// Hit-tests its child by `shape` rather than its bounding box, as SwiftUI's
/// `contentShape` does: inside the shape the child is hit even where it paints
/// nothing; outside it, the tap reaches what is below.
///
/// Unlike [`ClipOval`](crate::ClipOval) and its siblings it clips nothing and
/// opens no layer; layout, paint and the semantics rect are the child's.
#[derive(Clone, Debug)]
pub struct ContentShape {
    shape: HitShape,
    child: Child,
}

impl ContentShape {
    /// Hit-test the child by `shape`.
    #[must_use]
    pub fn new(shape: HitShape) -> Self {
        Self {
            shape,
            child: Child::empty(),
        }
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ContentShape {
    type Protocol = BoxProtocol;
    type RenderObject = RenderContentShape;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderContentShape::new(self.shape.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_shape(self.shape.clone())
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ContentShape);
