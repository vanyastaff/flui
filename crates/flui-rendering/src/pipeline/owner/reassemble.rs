//! Hot-reload hooks on the render pipeline owner.
//!
//! Flutter parity: `PipelineOwner.flushReassemble()` / `RenderObject.reassemble()`.

use flui_foundation::RenderId;

use super::PipelineOwner;
use crate::pipeline::phase::{Idle, PipelinePhase};

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Reassemble every render object in the tree and mark layout + paint dirty.
    ///
    /// Called during hot reload after the worker dylib is swapped. Preserves
    /// render object identity (same `RenderId`, same in-tree state) while
    /// forcing a full rebuild pass on the next frame.
    pub fn reassemble(&mut self) {
        let Some(root_id) = self.root_id else {
            return;
        };

        let ids: Vec<RenderId> = self.render_tree.collect_subtree_ids(root_id);
        let node_count = ids.len();

        for id in ids {
            let depth = self
                .render_tree
                .get(id)
                .map_or(0, |node| node.depth() as usize);
            if let Some(node) = self.render_tree.get_mut(id) {
                node.reassemble();
            }
            self.add_node_needing_layout(id, depth);
            self.add_node_needing_paint(id, depth);
        }

        tracing::info!(
            nodes = node_count,
            "PipelineOwner::reassemble — render objects marked for hot reload"
        );
    }
}

/// Convenience impl for the common idle owner handle.
impl PipelineOwner<Idle> {
    /// See [`PipelineOwner::reassemble`].
    pub fn reassemble_idle(&mut self) {
        self.reassemble();
    }
}
