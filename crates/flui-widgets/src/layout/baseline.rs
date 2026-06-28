//! [`Baseline`] — positions its child so a given text baseline sits at a fixed
//! distance from the top.

use flui_geometry::px;
use flui_objects::RenderBaseline;
use flui_rendering::protocol::BoxProtocol;
use flui_types::typography::TextBaseline;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Positions its child so the child's `baseline_type` baseline sits `baseline`
/// device pixels below this box's top edge.
///
/// Flutter parity: `widgets/basic.dart` `Baseline` over `RenderBaseline`.
#[derive(Clone, Debug)]
pub struct Baseline {
    distance: f32,
    kind: TextBaseline,
    child: Child,
}

impl Baseline {
    /// Place the child's `baseline_type` baseline `baseline` pixels from the top.
    pub fn new(baseline: f32, baseline_type: TextBaseline) -> Self {
        Self {
            distance: baseline,
            kind: baseline_type,
            child: Child::empty(),
        }
    }

    /// Set the aligned child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Baseline {
    type Protocol = BoxProtocol;
    type RenderObject = RenderBaseline;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderBaseline::new(self.kind, px(self.distance))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_baseline(self.kind);
        render_object.set_baseline_offset(px(self.distance));
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

impl_render_view!(Baseline);
