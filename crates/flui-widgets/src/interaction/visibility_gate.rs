//! [`VisibilityGate`] — lays its child out but paints it only while visible.

use flui_objects::RenderVisibility;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Keeps its child laid out — occupying its full space — while suppressing
/// the child's paint when `visible` is false.
///
/// Flutter parity: the private `_Visibility` over `_RenderVisibility`
/// (`widgets/indexed_stack.dart`), which `Visibility`'s `maintainSize` branch
/// composes. FLUI exposes it under a name of its own because `Visibility` is
/// already taken by the composing widget, and because a paint gate is useful
/// on its own — but [`Visibility`](crate::Visibility) is what callers
/// normally want, since this widget alone changes neither hit-testing nor
/// focus.
///
/// Deliberately not `Opacity(0.0)`: a fully transparent opacity still leaves
/// an opacity layer in the tree and forces every ancestor to composite, which
/// is the reason the oracle grew a dedicated render object for this.
#[derive(Clone, Debug)]
pub struct VisibilityGate {
    visible: bool,
    child: Child,
}

impl Default for VisibilityGate {
    fn default() -> Self {
        Self {
            visible: true,
            child: Child::empty(),
        }
    }
}

impl VisibilityGate {
    /// Create a gate that paints its child (`visible = true`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether the child is painted.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl_render_view!(VisibilityGate);

impl RenderView for VisibilityGate {
    type Protocol = BoxProtocol;
    type RenderObject = RenderVisibility;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderVisibility::new(self.visible)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_visible(self.visible)
    }

    flui_view::single_child_view_children!();
}
