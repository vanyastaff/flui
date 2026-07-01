//! [`Flow`] — positions children with paint-time transform matrices chosen
//! by a [`FlowDelegate`].

use std::fmt;
use std::sync::Arc;

use flui_objects::RenderFlow;
use flui_rendering::delegates::FlowDelegate;
use flui_rendering::protocol::BoxProtocol;
use flui_types::painting::Clip;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Positions its children using transformation matrices chosen by a
/// [`FlowDelegate`], instead of the layout-time offsets every other
/// multi-child layout widget uses.
///
/// Flutter parity: `widgets/basic.dart` `Flow` over `RenderFlow`. Defaults
/// match Flutter: `clip_behavior = Clip::HardEdge`.
///
/// Generic over `C: ViewSeq`, like [`Stack`](crate::Stack) — a dynamic
/// `Vec<BoxedView>` or a `stack!`/`column!`-style tuple.
#[derive(Clone)]
pub struct Flow<C = Vec<BoxedView>> {
    delegate: Arc<dyn FlowDelegate>,
    clip_behavior: Clip,
    children: C,
}

impl<C> Flow<C> {
    /// A flow of the given children, driven by `delegate`, with Flutter's
    /// default `Clip::HardEdge` clip behavior.
    pub fn new(delegate: Arc<dyn FlowDelegate>, children: C) -> Self {
        Self {
            delegate,
            clip_behavior: Clip::HardEdge,
            children,
        }
    }

    /// Overrides the default clip behavior.
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }
}

impl<C: ViewSeq> fmt::Debug for Flow<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Flow")
            .field("clip_behavior", &self.clip_behavior)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for Flow<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlow;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderFlow::new(self.delegate.clone()).with_clip_behavior(self.clip_behavior)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        // The `DelegateChange` return is discarded today — same
        // "framework marks paint/layout unconditionally, future-proofing
        // only" caveat `CustomPaint::update_render_object` already
        // accepts for `set_painter`'s bool.
        render_object.set_delegate(self.delegate.clone());
        render_object.set_clip_behavior(self.clip_behavior);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Flow);
