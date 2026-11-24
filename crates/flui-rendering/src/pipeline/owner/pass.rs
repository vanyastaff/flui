//! [`FramePass`]: the pipeline owner lent to one phase of a frame (ADR-0399).

use std::marker::PhantomData;
use std::ops::Deref;

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_layer::SceneWriter;

use crate::constraints::BoxConstraints;
use crate::error::RenderResult;
use crate::pipeline::phase::{Compositing, Layout, PaintPhase, PipelinePhase, Semantics};

use super::PipelineOwner;

/// The pipeline owner lent to phase `P` of a frame.
///
/// [`PipelineOwner::layout_pass`] lends it to layout, and each pass hands
/// the same borrow to the next phase: layout, compositing, paint, semantics.
/// Each phase's `run_*` is a method of its own pass only, so a phase run out
/// of order does not compile, and while any pass is alive the owner's own
/// `&mut self` operations (a frame, relocation, a root change) are a second
/// borrow the compiler refuses.
///
/// A pass owes nothing when it ends: each `run_*` has committed its phase's
/// work, or kept it queued, before it returns or unwinds, so dropping a pass
/// early, or forgetting it, leaves the owner as the last finished phase left
/// it. A pass reads the owner through [`Deref`]; a mutation between phases
/// ends the pass and takes a new one.
#[must_use = "a pass runs nothing until its phase's `run_*` is called"]
pub struct FramePass<'o, P: PipelinePhase> {
    owner: &'o mut PipelineOwner,
    phase: PhantomData<P>,
}

impl<'o, P: PipelinePhase> FramePass<'o, P> {
    fn new(owner: &'o mut PipelineOwner) -> Self {
        Self {
            owner,
            phase: PhantomData,
        }
    }

    fn next<Q: PipelinePhase>(self) -> FramePass<'o, Q> {
        FramePass::new(self.owner)
    }
}

impl<P: PipelinePhase> Deref for FramePass<'_, P> {
    type Target = PipelineOwner;

    fn deref(&self) -> &PipelineOwner {
        self.owner
    }
}

impl<P: PipelinePhase> std::fmt::Debug for FramePass<'_, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FramePass")
            .field("phase", &P::NAME)
            .field("owner", &self.owner)
            .finish()
    }
}

impl PipelineOwner {
    /// Lends this owner to the layout phase of a frame, the first of its
    /// passes. [`run_frame`](Self::run_frame) drives every phase; a caller
    /// that drives them by hand starts here.
    pub fn layout_pass(&mut self) -> FramePass<'_, Layout> {
        FramePass::new(self)
    }
}

impl<'o> FramePass<'o, Layout> {
    /// Lays out every dirty render object, shallow first, so parents are
    /// laid out before their children.
    ///
    /// # Errors
    ///
    /// The first layout error of the pass.
    pub fn run_layout(&mut self) -> RenderResult<()> {
        self.owner.flush_layout()
    }

    /// Lays out the subtree under one dirty root at `constraints`, as each
    /// root of [`run_layout`](Self::run_layout) is.
    ///
    /// # Errors
    ///
    /// The walk's first error.
    pub fn layout_dirty_root(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> RenderResult<Size> {
        self.owner.layout_dirty_root(id, constraints)
    }

    /// Hands the owner on to the compositing phase.
    pub fn into_compositing(self) -> FramePass<'o, Compositing> {
        self.next()
    }
}

impl<'o> FramePass<'o, Compositing> {
    /// Recomputes the compositing bits of every node that asked for it.
    ///
    /// # Errors
    ///
    /// The first error of the walk.
    pub fn run_compositing(&mut self) -> RenderResult<()> {
        self.owner.flush_compositing_bits()
    }

    /// Hands the owner on to the paint phase.
    pub fn into_paint(self) -> FramePass<'o, PaintPhase> {
        self.next()
    }
}

impl<'o> FramePass<'o, PaintPhase> {
    /// Paints the boundaries the queue owes and buffers the patch in
    /// `writer`.
    ///
    /// # Errors
    ///
    /// [`RenderError::PaintBeforeLayout`](crate::error::RenderError::PaintBeforeLayout)
    /// when layout work is still queued, or the paint walk's first error.
    pub fn run_paint(&mut self, writer: &mut SceneWriter<'_>) -> RenderResult<()> {
        self.owner.flush_paint(writer)
    }

    /// Hands the owner on to the semantics phase.
    pub fn into_semantics(self) -> FramePass<'o, Semantics> {
        self.next()
    }
}

impl FramePass<'_, Semantics> {
    /// Assembles the semantics of every node that asked for it.
    ///
    /// # Errors
    ///
    /// The first error of the assembly.
    pub fn run_semantics(&mut self) -> RenderResult<()> {
        self.owner.flush_semantics()
    }
}

#[cfg(any(test, feature = "testing"))]
impl FramePass<'_, Semantics> {
    /// The one-shot semantics failure a test asked for with
    /// [`PipelineOwner::fail_next_semantics_after_paint_for_test`], taken
    /// once a frame reaches its semantics phase.
    pub(super) fn take_semantics_error_for_test(&mut self) -> Option<crate::error::RenderError> {
        self.owner.semantics_error_once_for_test.take()
    }
}
