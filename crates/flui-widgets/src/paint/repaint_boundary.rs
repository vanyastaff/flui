//! [`RepaintBoundary`] — isolates its child's painting into its own layer.

use flui_objects::RenderRepaintBoundary;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View};

/// Isolates its child into a separate compositing layer so repaints of the
/// child (or its siblings) don't force each other to re-paint.
///
/// Flutter parity: `widgets/basic.dart` `RepaintBoundary` over
/// `RenderRepaintBoundary`. Layout is a pass-through (the child's size); the
/// boundary only affects paint/compositing.
#[derive(Clone, Debug, Default)]
pub struct RepaintBoundary {
    child: Child,
    /// Report the child's key as this boundary's own.
    ///
    /// Off by default: a boundary that borrowed its child's key would change
    /// which element a `GlobalKey` resolves to for every direct user of this
    /// widget.
    ///
    /// The scrolling delegates turn it on. Flutter keeps the boundary keyless
    /// and restores the key OUTSIDE it with `KeyedSubtree`
    /// (`widgets/scroll_delegate.dart:559`, `:572`), which FLUI cannot copy
    /// verbatim: a lazy sliver requires every child element to own a render
    /// node (`sparse_children.rs`'s "a lazy sliver child must own a render
    /// node to carry its logical index"), and a `KeyedSubtree` equivalent is a
    /// stateless view with none. Carrying the key on the boundary — which does
    /// own one — puts it at the same level Flutter puts it: the outermost
    /// element the parent reconciles.
    forwards_child_key: bool,
}

impl RepaintBoundary {
    /// Create a repaint boundary with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the isolated child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Report the child's key as this boundary's own — see
    /// [`Self::forwards_child_key`].
    #[must_use]
    pub(crate) fn forwarding_child_key(mut self) -> Self {
        self.forwards_child_key = true;
        self
    }
}

impl RenderView for RepaintBoundary {
    type Protocol = BoxProtocol;
    type RenderObject = RenderRepaintBoundary;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderRepaintBoundary::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // A repaint boundary carries no configuration — nothing to update.
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

// Hand-written rather than `impl_render_view!`, which emits only
// `create_element` and leaves `key` at the trait default of `None`.
impl View for RepaintBoundary {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        if self.forwards_child_key {
            self.child.as_ref()?.key()
        } else {
            None
        }
    }
}
