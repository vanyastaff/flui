//! [`Offstage`] — lays out its child but gives it zero size and no paint.

use flui_objects::RenderOffstage;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Keeps its child in the tree (so its state persists) but, while `offstage`,
/// lays it out off-screen at zero size and does not paint or hit-test it.
///
/// Flutter parity: `widgets/basic.dart` `Offstage` over `RenderOffstage`.
/// `offstage` defaults to `true`. Toggle it to show/hide without losing the
/// child's state — cheaper than rebuilding when the child is expensive.
#[derive(Clone, Debug)]
pub struct Offstage {
    offstage: bool,
    child: Child,
}

impl Default for Offstage {
    fn default() -> Self {
        Self {
            offstage: true,
            child: Child::empty(),
        }
    }
}

impl Offstage {
    /// Create an `Offstage` that hides its child (`offstage = true`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether the child is offstage (hidden, zero-size).
    #[must_use]
    pub fn offstage(mut self, offstage: bool) -> Self {
        self.offstage = offstage;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Offstage {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOffstage;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderOffstage::new(self.offstage)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_offstage(self.offstage);
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

impl_render_view!(Offstage);
