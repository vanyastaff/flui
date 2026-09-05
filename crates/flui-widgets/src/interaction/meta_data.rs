//! `MetaData` — attach an opaque payload to a subtree so a hit test can find it.
//!
//! Flutter parity: `widgets/basic.dart`'s `MetaData` over
//! `RenderMetaData` (tag `3.44.0`).

use std::any::Any;
use std::sync::Arc;

use flui_objects::RenderMetaData;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Attaches an opaque payload to its child's position in the render tree, so
/// that a hit test at that position can identify it.
///
/// The payload arrives on the [`HitTestEntry`] for this node, where a searcher
/// downcasts it with `metadata_as::<T>()` — without knowing anything about the
/// widget that put it there. That indirection is the point: it is how a drag
/// discovers the targets it has moved over, given only a list of positions and
/// no way to ask the element tree who lives under one.
///
/// Choose [`HitTestBehavior`] with care, because it decides whether this node
/// is in a hit path at all:
///
/// - `DeferToChild` (default) — found only when the child itself is hit.
/// - `Opaque` — always found within its own bounds, even over empty space.
/// - `Translucent` — found, without stopping siblings beneath from being hit.
///
/// [`HitTestEntry`]: flui_rendering::hit_testing::HitTestEntry
///
/// # Example
///
/// ```ignore
/// #[derive(Debug)]
/// struct Slot(usize);
///
/// MetaData::new(Slot(3)).child(Text::new("drop here"))
/// ```
#[derive(Clone)]
pub struct MetaData {
    payload: Option<Arc<dyn Any + Send + Sync>>,
    behavior: HitTestBehavior,
    child: Child,
}

impl std::fmt::Debug for MetaData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaData")
            .field("has_payload", &self.payload.is_some())
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl Default for MetaData {
    fn default() -> Self {
        Self {
            payload: None,
            behavior: HitTestBehavior::DeferToChild,
            child: Child::empty(),
        }
    }
}

impl MetaData {
    /// Attach `payload` to this subtree.
    #[must_use]
    pub fn new<T>(payload: T) -> Self
    where
        T: Any + Send + Sync + 'static,
    {
        Self {
            payload: Some(Arc::new(payload)),
            ..Self::default()
        }
    }

    /// Attach an already-shared payload, so several nodes can carry one value.
    #[must_use]
    pub fn shared(payload: Arc<dyn Any + Send + Sync>) -> Self {
        Self {
            payload: Some(payload),
            ..Self::default()
        }
    }

    /// Set how this node participates in hit testing.
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Set the child this metadata is attached to.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for MetaData {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMetaData;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderMetaData::new();
        render_object.set_shared_metadata(self.payload.clone());
        render_object.set_behavior(self.behavior);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // Neither the payload nor the behavior affects layout or paint; both
        // are read only during a hit test, which reads live state rather than
        // anything committed by a frame.
        render_object.set_shared_metadata(self.payload.clone());
        render_object.set_behavior(self.behavior);
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(MetaData);
