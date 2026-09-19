//! Hot-reload hooks on the render pipeline owner.
//!
//! Flutter parity: `PipelineOwner.flushReassemble()` / `RenderObject.reassemble()`.

use flui_foundation::RenderId;

use super::PipelineOwner;
use crate::pipeline::phase::{Idle, PipelinePhase};

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Reassemble every render object in the tree and mark layout, compositing
    /// bits, paint, and semantics dirty.
    ///
    /// Called during hot reload after the worker dylib is swapped. Preserves
    /// render object identity (same `RenderId`, same in-tree state) while
    /// forcing a full rebuild pass on the next frame.
    ///
    /// All four marks mirror Flutter's `RenderObject.reassemble()`
    /// (`rendering/object.dart`), which does exactly:
    /// `markNeedsLayout` / `markNeedsCompositingBitsUpdate` / `markNeedsPaint` /
    /// `markNeedsSemanticsUpdate` plus a child walk. Reassemble is the one place
    /// a hot reload can change code that affects compositing or semantics
    /// without touching layout or paint, so omitting those two marks would leave
    /// the changed behaviour invisible until something else dirtied those phases
    /// — the mark-and-skip divergence this method exists to avoid.
    ///
    /// `mark_needs_semantics` remains gated on semantics being enabled
    /// ([`PipelineOwner::semantics_enabled`]); that gate is the pipeline's own
    /// contract, not a departure from Flutter, and marking is a no-op when
    /// semantics are off.
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
            // Enqueue and set the layout flag — `run_layout` skips queue
            // entries whose `needs_layout()` is already false (layout.rs).
            if let Some(node) = self.render_tree.get(id) {
                node.mark_layout_flag();
            }
            self.add_node_needing_layout(id, depth);
            self.mark_needs_compositing_bits_update(id);
            self.mark_needs_paint(id);
            self.mark_needs_semantics(id);
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
