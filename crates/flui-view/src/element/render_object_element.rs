//! RenderTreeRootElement - marker trait for the render-tree root element.
//!
//! FLUI has no element-side child-mutation seam (insert / move / remove /
//! attach / detach of a child render object on the parent element): each
//! render element adopts itself into its parent's render object at mount.
//! The live model lives in this crate's
//! `ARCHITECTURE.md` (`## Mapping decisions` → "Render children adopt
//! themselves at mount"); the replacement guarantee for the live contract
//! is `RenderBehavior::on_mount`'s orphaned-mount gate plus its
//! `orphaned_render_mount` test family
//! (`crates/flui-view/tests/orphaned_render_mount.rs`).

/// Marker trait for root elements that bootstrap a new render tree.
///
/// RenderTreeRootElement is special in that it:
/// - Does NOT insert its render object into a parent render object
/// - Instead, sets pipelineOwner.rootNode = renderObject
/// - Creates its own PipelineOwner (or uses a provided one)
///
/// Attach / detach to the pipeline owner is implemented inline in
/// `mount()` / `unmount()` on the concrete root element (see
/// `RootRenderElement::mount` in `view/root.rs`). The pre-Mythos
/// `attach_to_pipeline_owner` / `detach_from_pipeline_owner` trait
/// methods were removed during the framework spine repair: their bodies
/// were panicking placeholders and they had no callers (Constitution
/// Principle 6: no panic in production paths).
pub trait RenderTreeRootElement {
    /// Get the [`PipelineCell`](flui_rendering::pipeline::PipelineCell) for
    /// this render tree.
    ///
    /// The RenderTreeRootElement owns or references the `PipelineOwner`
    /// that manages the render tree rooted at this element.
    fn pipeline_owner(&self) -> Option<flui_rendering::pipeline::PipelineCell>;

    /// Set the [`PipelineCell`](flui_rendering::pipeline::PipelineCell) for
    /// this render tree.
    fn set_pipeline_owner(&mut self, owner: flui_rendering::pipeline::PipelineCell);
}
