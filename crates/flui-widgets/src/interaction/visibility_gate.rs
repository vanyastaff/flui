//! [`VisibilityGate`] — lays its child out but paints it only while visible.

use flui_objects::RenderVisibility;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Keeps its child laid out — occupying its full space — while suppressing
/// the child's paint and semantics when `visible` is false. Explicit
/// [`maintain_semantics`](Self::maintain_semantics) keeps hidden semantics.
///
/// `Visibility`'s `maintainSize` branch composes it. It is a public widget
/// under a name of its own because `Visibility` is already taken by the
/// composing widget, and because a paint gate is useful
/// on its own — but [`Visibility`](crate::Visibility) is what callers
/// normally want, since this widget alone changes neither hit-testing nor
/// focus.
///
/// The gate emits no opacity layer and changes no layout geometry.
#[derive(Clone, Debug)]
pub struct VisibilityGate {
    visible: bool,
    maintain_semantics: bool,
    child: Child,
}

impl Default for VisibilityGate {
    fn default() -> Self {
        Self {
            visible: true,
            maintain_semantics: false,
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

    /// Keep the child in the accessibility tree while hidden (default `false`).
    #[must_use]
    pub fn maintain_semantics(mut self, maintain_semantics: bool) -> Self {
        self.maintain_semantics = maintain_semantics;
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
        let mut render_object = RenderVisibility::new(self.visible);
        // The object is not attached yet; its first semantics pass reads this policy.
        let _ = render_object.set_maintain_semantics(self.maintain_semantics);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_visible(self.visible)
            | render_object.set_maintain_semantics(self.maintain_semantics)
    }

    flui_view::single_child_view_children!();
}
