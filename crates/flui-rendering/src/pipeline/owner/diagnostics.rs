//! Diagnostics dump — Diagnosticable-backed render-tree introspection.

use flui_foundation::{DiagnosticsNode, RenderId};

use crate::{pipeline::phase::PipelinePhase, storage::RenderNode};

use super::PipelineOwner;

// ============================================================================
// Diagnostics dump (Diagnosticable-backed)
// ============================================================================

/// Builds a [`DiagnosticsNode`] for a single render node: the render object
/// self-describes its own properties, and the committed geometry/offset is
/// layered on from `RenderState`.
pub(super) fn node_diagnostics(node: &RenderNode) -> DiagnosticsNode {
    if let Some(entry) = node.as_box() {
        let mut diagnostics = entry.render_object().to_diagnostics_node();
        diagnostics = diagnostics.property("paint_offset", format!("{:?}", node.offset()));
        if let Some(size) = entry.state().geometry() {
            diagnostics = diagnostics.property("size", format!("{size:?}"));
        }
        diagnostics
    } else if let Some(entry) = node.as_sliver() {
        let mut diagnostics = entry.render_object().to_diagnostics_node();
        diagnostics = diagnostics.property("paint_offset", format!("{:?}", node.offset()));
        if let Some(geometry) = entry.state().geometry() {
            diagnostics = diagnostics.property("geometry", format!("{geometry:?}"));
        }
        diagnostics
    } else {
        DiagnosticsNode::new("<unknown>")
    }
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Returns a [`DiagnosticsNode`] tree mirroring the render hierarchy
    /// rooted at [`root_id`](Self::root_id), or `None` if no root is set.
    ///
    /// Each node self-describes via its render object's
    /// [`Diagnosticable`](flui_foundation::Diagnosticable) impl, with the
    /// committed geometry/offset layered on. This is the
    /// `Diagnosticable`-backed counterpart to a plain `{:?}` dump and the
    /// basis for [`debug_dump_pipeline_owner_tree`](crate::binding::debug_dump_pipeline_owner_tree).
    pub fn debug_diagnostics_tree(&self) -> Option<DiagnosticsNode> {
        self.debug_diagnostics_subtree(self.root_id?)
    }

    /// Returns the [`DiagnosticsNode`] for a single node `id` (no children),
    /// or `None` if the id is not live.
    pub fn debug_node_diagnostics(&self, id: RenderId) -> Option<DiagnosticsNode> {
        Some(node_diagnostics(self.render_tree.get(id)?))
    }

    fn debug_diagnostics_subtree(&self, id: RenderId) -> Option<DiagnosticsNode> {
        let node = self.render_tree.get(id)?;
        let mut diagnostics = node_diagnostics(node);
        for &child in node.children() {
            if let Some(child_diagnostics) = self.debug_diagnostics_subtree(child) {
                diagnostics.add_child(child_diagnostics);
            }
        }
        Some(diagnostics)
    }
}
