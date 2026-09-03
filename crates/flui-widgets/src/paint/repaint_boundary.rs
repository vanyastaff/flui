//! [`RepaintBoundary`] — isolates its child's painting into its own layer.

use flui_foundation::SaltedKey;
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
    /// The item's key, salted, when this boundary is the per-item wrapper of
    /// a lazy sliver.
    ///
    /// The scrolling delegates set it. Flutter keeps the boundary keyless and
    /// restores the item's key OUTSIDE it with a `KeyedSubtree` carrying a
    /// `_SaltedValueKey` (`widgets/scroll_delegate.dart:559`, `:572`). FLUI
    /// cannot copy the wrapper: a lazy sliver requires every child element to
    /// own a render node, and a `KeyedSubtree` equivalent is a stateless view
    /// with none. So the boundary — which does own one — carries the key at
    /// the level Flutter puts it, and carries it *salted* for the same two
    /// reasons Flutter does: it is not the item's key (two elements may not
    /// answer to one key in one parent), and it is never a `GlobalKey`, so the
    /// boundary registers nothing and the item inside registers its own key
    /// exactly once. A sliver looking an item up by key sees through the salt
    /// (`SaltedKey::unsalt`).
    salted_child_key: Option<SaltedKey>,
    /// Whether this boundary carries its child's key at all (the scrolling
    /// delegates ask for it; a plain `RepaintBoundary` stays keyless).
    salts_child_key: bool,
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
        self.refresh_salted_key();
        self
    }

    /// Carry the child's key, salted, as this boundary's own — see
    /// [`Self::salted_child_key`]. Order-independent with [`Self::child`]:
    /// whichever is set last re-derives the salt.
    #[must_use]
    pub(crate) fn salting_child_key(mut self) -> Self {
        self.salts_child_key = true;
        self.refresh_salted_key();
        self
    }

    fn refresh_salted_key(&mut self) {
        self.salted_child_key = if self.salts_child_key {
            self.child
                .as_ref()
                .and_then(|c| c.key())
                .map(SaltedKey::new)
        } else {
            None
        };
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
        self.salted_child_key
            .as_ref()
            .map(|key| key as &dyn flui_foundation::ViewKey)
    }
}
