//! A retained scene a test drives a [`PipelineOwner`]'s frames into.
//!
//! Production frames go through the UI runtime, which holds one
//! [`SceneOwner`] per presentation, applies each patch paint buffers and
//! hands the version to its sink (ADR-0304 §7, §8). A test of the pipeline
//! alone holds a [`SceneHost`] instead: it composes the scene's root around
//! the pipeline's frame node, applies the patch, reports the version back
//! with [`PipelineOwner::scene_applied`], and flattens the frame node for
//! assertions.

use std::sync::Arc;

use flui_foundation::SurfaceGeneration;
use flui_foundation::TreeError;
use flui_layer::{
    BoundaryContent, LayerTree, ReleasedContents, SceneNodeId, SceneOp, SceneOwner, SceneVersion,
    SceneWriter, SurfaceKey,
};

use crate::error::RenderError;
use crate::pipeline::{Idle, PipelineCell, PipelineOwner, PipelinePhase};

/// A scene owner a test runs pipeline frames into.
#[derive(Debug)]
pub struct SceneHost {
    owner: SceneOwner,
    surface: SurfaceKey,
}

impl Default for SceneHost {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneHost {
    /// An empty scene on an 800×600 surface.
    #[must_use]
    pub fn new() -> Self {
        Self {
            owner: SceneOwner::new(),
            surface: SurfaceKey {
                size: (800, 600),
                generation: SurfaceGeneration::ZERO,
            },
        }
    }

    /// The scene's owner, for a test that renews it or reads its counters.
    pub fn owner_mut(&mut self) -> &mut SceneOwner {
        &mut self.owner
    }

    /// The last applied version.
    #[must_use]
    pub fn version(&self) -> &SceneVersion {
        self.owner.current()
    }

    /// Runs one frame of `pipeline` by value
    /// ([`PipelineOwner::run_frame`]), applies what it painted and returns
    /// the frame node's tree, `None` for a frame that painted nothing.
    ///
    /// # Panics
    ///
    /// When the patch is refused: a defect in the producer.
    pub fn run_frame(
        &mut self,
        pipeline: PipelineOwner<Idle>,
    ) -> (
        PipelineOwner<Idle>,
        Result<Option<LayerTree>, TreeError<RenderError>>,
    ) {
        let mut held = Some(pipeline);
        let (result, tree) = self.drive_owned(&mut held, |pipeline, writer| {
            let (pipeline, result) = pipeline.run_frame(writer);
            (pipeline, result)
        });
        let pipeline = held.expect("BUG: the pipeline comes back from its frame");
        (
            pipeline,
            result.map(|painted| painted.then_some(tree).flatten()),
        )
    }

    /// Runs one frame of `pipeline` in place
    /// ([`PipelineOwner::run_frame_in_place`]); otherwise as
    /// [`Self::run_frame`].
    ///
    /// # Errors
    ///
    /// The frame's first render error.
    ///
    /// # Panics
    ///
    /// When the patch is refused: a defect in the producer.
    pub fn run_frame_in_place(
        &mut self,
        pipeline: &mut PipelineOwner<Idle>,
    ) -> Result<Option<LayerTree>, TreeError<RenderError>> {
        let (result, tree) = self.drive(pipeline, |pipeline, writer| {
            pipeline.run_frame_in_place(writer)
        });
        result.map(|painted| painted.then_some(tree).flatten())
    }

    /// Runs one frame of the owner `cell` holds
    /// ([`PipelineCell::run_frame`](crate::pipeline::PipelineCell::run_frame));
    /// otherwise as [`Self::run_frame`].
    ///
    /// # Errors
    ///
    /// The frame's first render error.
    ///
    /// # Panics
    ///
    /// When the patch is refused, or a borrow from `cell` is live.
    pub fn run_cell_frame(
        &mut self,
        cell: &crate::pipeline::PipelineCell,
    ) -> Result<Option<LayerTree>, TreeError<RenderError>> {
        // The frame checks the owner out; applying the patch and dropping the
        // version it replaces happen after that checkout ends: an
        // annotation's `Drop` is user code and may reach the cell.
        let (result, tree) = self.drive_cell(cell, |writer| cell.run_frame(writer));
        result.map(|painted| painted.then_some(tree).flatten())
    }

    /// Runs `paint` with a patch open, for a test that drives the phases
    /// itself, then applies what the pipeline buffered, reports the version
    /// and returns the frame node's tree beside `paint`'s answer; `None` when
    /// it buffered nothing.
    ///
    /// # Panics
    ///
    /// When the patch is refused: a defect in the producer.
    pub fn drive<P: PipelinePhase, R>(
        &mut self,
        pipeline: &mut PipelineOwner<P>,
        paint: impl FnOnce(&mut PipelineOwner<P>, &mut SceneWriter<'_>) -> R,
    ) -> (R, Option<LayerTree>) {
        let (answer, applied) = self.apply(pipeline, paint);
        let tree = applied.then(|| self.frame_tree(pipeline)).flatten();
        (answer, tree)
    }

    /// [`Self::drive`] without flattening: whether a version was applied,
    /// for a bench that times what a production frame does.
    ///
    /// # Panics
    ///
    /// As [`Self::drive`].
    pub fn apply<P: PipelinePhase, R>(
        &mut self,
        pipeline: &mut PipelineOwner<P>,
        paint: impl FnOnce(&mut PipelineOwner<P>, &mut SceneWriter<'_>) -> R,
    ) -> (R, bool) {
        let answer = {
            let mut writer = self.owner.writer();
            let answer = paint(pipeline, &mut writer);
            if pipeline.scene_staged() {
                compose_root(&mut writer, pipeline.scene_frame_node());
            }
            answer
        };
        if !pipeline.scene_staged() {
            return (answer, false);
        }
        let released = self.apply_patch();
        drop(pipeline.scene_applied(self.owner.current()));
        self.restock(released);
        (answer, true)
    }

    /// [`Self::drive`] for a pipeline its `cell` holds, which `paint` reaches
    /// through the cell itself (a frame that services layout builders
    /// between passes).
    ///
    /// # Panics
    ///
    /// When the patch is refused, or a borrow from `cell` is live after
    /// `paint` returns.
    pub fn drive_cell<R>(
        &mut self,
        cell: &PipelineCell,
        paint: impl FnOnce(&mut SceneWriter<'_>) -> R,
    ) -> (R, Option<LayerTree>) {
        let (answer, applied) = self.apply_cell(cell, paint);
        let tree = applied
            .then(|| cell.scene_frame_node())
            .flatten()
            .and_then(|node| self.owner.current().flatten_node(node));
        (answer, tree)
    }

    /// [`Self::drive_cell`] without flattening: whether a version was
    /// applied, for a host that flattens [`Self::version`] only when asked.
    ///
    /// # Panics
    ///
    /// As [`Self::drive_cell`].
    pub fn apply_cell<R>(
        &mut self,
        cell: &PipelineCell,
        paint: impl FnOnce(&mut SceneWriter<'_>) -> R,
    ) -> (R, bool) {
        let staged = || cell.with(PipelineOwner::scene_staged);
        let answer = {
            let mut writer = self.owner.writer();
            let answer = paint(&mut writer);
            if staged() {
                compose_root(&mut writer, cell.scene_frame_node());
            }
            answer
        };
        if !staged() {
            return (answer, false);
        }
        let released = self.apply_patch();
        cell.scene_applied(self.owner.current());
        self.restock(released);
        (answer, true)
    }

    /// [`Self::drive`] for a frame that takes the pipeline by value.
    fn drive_owned<R>(
        &mut self,
        held: &mut Option<PipelineOwner<Idle>>,
        paint: impl FnOnce(PipelineOwner<Idle>, &mut SceneWriter<'_>) -> (PipelineOwner<Idle>, R),
    ) -> (R, Option<LayerTree>) {
        let answer = {
            let mut writer = self.owner.writer();
            let pipeline = held
                .take()
                .expect("BUG: the pipeline is held between frames");
            let (pipeline, answer) = paint(pipeline, &mut writer);
            if pipeline.scene_staged() {
                compose_root(&mut writer, pipeline.scene_frame_node());
            }
            *held = Some(pipeline);
            answer
        };
        let pipeline = held.as_mut().expect("BUG: the pipeline is back");
        if !pipeline.scene_staged() {
            return (answer, None);
        }
        let released = self.apply_patch();
        drop(pipeline.scene_applied(self.owner.current()));
        self.restock(released);
        let tree = self.frame_tree(pipeline);
        (answer, tree)
    }

    /// The raster side shows the last applied version, as a window that
    /// presents every frame does: the log before it is let go, and what no
    /// one else holds goes back to the owner for the next frames to write
    /// into ([`SceneOwner::restock`]).
    pub fn shown(&mut self) {
        let shown = self.owner.current().id();
        let released = self.owner.completed(Some(shown));
        self.restock(released);
    }

    /// Applies the patch the frame buffered.
    ///
    /// # Panics
    ///
    /// When the patch is refused: a defect in the producer.
    fn apply_patch(&mut self) -> ReleasedContents {
        match self.owner.apply(self.surface) {
            Ok(applied) => applied.released,
            Err(error) => panic!("BUG: the pipeline's patch was refused: {error}"),
        }
    }

    /// Runs the destructors of what the owner released, with no borrow
    /// held, and hands it back what nobody else holds.
    fn restock(&mut self, released: ReleasedContents) {
        let reclaimed = released.reclaim();
        self.owner.restock(reclaimed);
    }

    /// The content the repaint boundary `id` is published with in the last
    /// applied version: a test compares two frames' contents with
    /// [`Arc::ptr_eq`] to tell a kept boundary from a republished one.
    #[must_use]
    pub fn content_of<P: PipelinePhase>(
        &self,
        pipeline: &PipelineOwner<P>,
        id: flui_foundation::RenderId,
    ) -> Option<Arc<BoundaryContent>> {
        let node = pipeline.boundary_scene_node(id)?;
        Some(Arc::clone(self.owner.current().node(node)?.content()))
    }

    /// The tree the pipeline's frame node paints in the last applied
    /// version, rooted at its device-pixel-ratio layer.
    #[must_use]
    pub fn frame_tree<P: PipelinePhase>(&self, pipeline: &PipelineOwner<P>) -> Option<LayerTree> {
        self.owner
            .current()
            .flatten_node(pipeline.scene_frame_node()?)
    }
}

/// Republishes the scene's root around `frame` when it does not slot it
/// already: the composition the UI runtime makes around a presentation's
/// pipeline.
pub fn compose_root(writer: &mut SceneWriter<'_>, frame: Option<SceneNodeId>) {
    let current = writer.current();
    let root = current.root();
    let slotted = current
        .node(root)
        .and_then(|node| node.content().slots().next());
    if slotted == frame {
        return;
    }
    let mut content = BoundaryContent::builder();
    if let Some(frame) = frame {
        content.slot(frame);
    }
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(content.finish()),
    });
}
