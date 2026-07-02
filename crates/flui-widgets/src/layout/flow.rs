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

#[cfg(test)]
mod tests {
    use std::any::Any;

    use flui_rendering::constraints::BoxConstraints;
    use flui_types::Size;
    use flui_view::RenderView;
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    /// A minimal delegate -- only needed to satisfy `FlowDelegate`'s object
    /// safety; these tests exercise the widget's own wiring, not delegate
    /// behavior (already covered by `tests/flow.rs`'s `StepDelegate`).
    #[derive(Debug)]
    struct NoopDelegate;

    impl FlowDelegate for NoopDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            constraints: BoxConstraints,
        ) -> BoxConstraints {
            constraints
        }

        fn paint_children(
            &self,
            _context: &mut flui_rendering::delegates::FlowPaintingContext<'_, '_>,
        ) {
        }

        fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }

        fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn delegate() -> Arc<dyn FlowDelegate> {
        Arc::new(NoopDelegate)
    }

    #[test]
    fn create_render_object_defaults_to_hard_edge_clip() {
        let flow: Flow = Flow::new(delegate(), Vec::new());
        let render_object = flow.create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn create_render_object_applies_an_overridden_clip_behavior() {
        let flow: Flow = Flow::new(delegate(), Vec::new()).clip_behavior(Clip::None);
        let render_object = flow.create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn update_render_object_applies_a_changed_clip_behavior() {
        let flow: Flow = Flow::new(delegate(), Vec::new());
        let mut render_object = flow.create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);

        let updated: Flow = Flow::new(delegate(), Vec::new()).clip_behavior(Clip::None);
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn debug_reports_clip_behavior_and_child_count() {
        let flow = Flow::new(delegate(), vec![SizedBox::shrink().boxed()]);
        let debug = format!("{flow:?}");
        assert!(
            debug.contains("clip_behavior: HardEdge") && debug.contains("children: 1"),
            "Debug output must include clip_behavior and children count, got: {debug}",
        );
    }

    #[test]
    fn has_children_reflects_an_empty_child_list() {
        let empty: Flow = Flow::new(delegate(), Vec::new());
        assert!(!empty.has_children());

        let non_empty = Flow::new(delegate(), vec![SizedBox::shrink().boxed()]);
        assert!(non_empty.has_children());
    }
}
