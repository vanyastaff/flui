//! RenderView - the root of the render tree.

use flui_foundation::Variable;
use flui_foundation::geometry::{Matrix4, Offset, Size};
use flui_foundation::inspect::{Fields, Inspect};

use crate::constraints::BoxConstraints;
use crate::context::{BoxLayoutContext, FragmentRecorder, PaintContext};
use crate::error::{RenderError, RenderResult};
use crate::parent_data::BoxParentData;
use crate::protocol::{BoxProtocol, Protocol, RawBoxLayoutContext, RenderObject};
use crate::traits::HitTestOutcome;

/// The root of the render tree.
///
/// It fills whatever constraints the pipeline owner holds for the root
/// ([`PipelineOwner::set_root_constraints`](crate::pipeline::PipelineOwner::set_root_constraints)),
/// lays every child out tight at that size and origin, draws nothing itself and
/// claims no hit of its own. It is a repaint and a relayout boundary.
#[derive(Debug, Default)]
pub struct RenderView;

impl RenderView {
    /// Creates the root render object.
    pub fn new() -> Self {
        Self
    }
}

impl Inspect for RenderView {
    fn kind(&self) -> &'static str {
        "RenderView"
    }

    fn inspect(&self, _out: &mut Fields<'_>) {}
}

impl RenderObject<BoxProtocol> for RenderView {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as Protocol>::ErasedLayoutContext<'_>,
    ) -> RenderResult<Size> {
        let typed_inner = RawBoxLayoutContext::<Variable, BoxParentData>::from_erased(ctx);
        let mut layout_ctx = BoxLayoutContext::<Variable, BoxParentData>::new(typed_inner);

        let constraints = *layout_ctx.constraints();
        let size = constraints.biggest();
        if !size.is_finite() {
            // Root constraints come from the window surface and must be
            // bounded; letting INF through would poison every descendant
            // geometry and paint bound. A typed error keeps the failure
            // diagnosable in release builds.
            tracing::error!(?constraints, "root constraints must be bounded");
            return Err(RenderError::unbounded_constraint("RenderView"));
        }
        let child_constraints = BoxConstraints::tight(size);
        for i in 0..layout_ctx.child_count() {
            layout_ctx.layout_child(i, child_constraints)?;
            layout_ctx.position_child(i, Offset::ZERO);
        }
        Ok(size)
    }

    fn paint_raw(&self, recorder: &mut FragmentRecorder, child_count: usize, size: Size) {
        // The root draws nothing and splices every child subtree in order;
        // `size` is only forwarded to the child-painting context.
        let mut ctx = PaintContext::<Variable>::new(recorder, child_count, size);
        ctx.paint_children();
    }

    fn hit_test_raw(
        &self,
        _position: Offset,
        child_count: usize,
        _size: Size,
        hit_child: &mut dyn FnMut(usize, Option<Offset>, Option<Matrix4>) -> bool,
    ) -> HitTestOutcome {
        // Topmost first: later siblings paint on top. An empty window region
        // reports a miss instead of a phantom root target.
        for index in (0..child_count).rev() {
            if hit_child(index, None, None) {
                return HitTestOutcome::from_hit(true);
            }
        }
        HitTestOutcome::miss()
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn is_relayout_boundary(&self) -> bool {
        true
    }
}
