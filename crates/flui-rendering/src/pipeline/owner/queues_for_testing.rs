//! The dirty queues' contents as ids, for tests that assert what an operation queued.

use flui_foundation::RenderId;

use super::PipelineOwner;
use crate::pipeline::phase::PipelinePhase;

impl<P: PipelinePhase> PipelineOwner<P> {
    /// The nodes queued for layout, in queue order.
    pub fn queued_for_layout(&self) -> Vec<RenderId> {
        self.scheduler
            .nodes_needing_layout()
            .iter()
            .map(|node| node.id)
            .collect()
    }

    /// The nodes queued for paint, in queue order.
    pub fn queued_for_paint(&self) -> Vec<RenderId> {
        self.scheduler
            .nodes_needing_paint()
            .iter()
            .map(|entry| entry.id)
            .collect()
    }

    /// The nodes queued for a compositing-bits update, in queue order.
    pub fn queued_for_compositing_bits(&self) -> Vec<RenderId> {
        self.scheduler
            .nodes_needing_compositing_bits_update()
            .iter()
            .map(|node| node.id)
            .collect()
    }

    /// The nodes queued for a semantics update, in queue order.
    pub fn queued_for_semantics(&self) -> Vec<RenderId> {
        self.scheduler
            .nodes_needing_semantics()
            .iter()
            .map(|node| node.id)
            .collect()
    }
}
