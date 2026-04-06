//! Frame submission and GPU synchronization.
//!
//! Contains the [`ScissorRect`] type for GPU clipping and the [`BatchedDraw`]
//! enum which represents individual GPU draw commands produced by batcher
//! flush operations.

use crate::pipelines::registry::PipelineId;

/// A scissor rectangle in physical pixel coordinates for GPU clipping.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScissorRect {
    /// Left edge in physical pixels.
    pub x: u32,
    /// Top edge in physical pixels.
    pub y: u32,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

/// A single GPU draw command, produced by batcher flush.
///
/// These commands form an ordered list that is replayed inside a render pass.
/// The encoder iterates this list, setting pipeline state and issuing draws.
#[derive(Debug)]
pub enum BatchedDraw {
    /// Instanced draw (shapes, images).
    Instanced {
        /// Which pipeline to bind.
        pipeline: PipelineId,
        /// Number of instances to draw.
        instance_count: u32,
    },
    /// Indexed draw (tessellated paths).
    Indexed {
        /// Which pipeline to bind.
        pipeline: PipelineId,
        /// Number of indices to draw.
        index_count: u32,
    },
    /// Text rendering (glyphon manages its own state).
    Text {
        /// Index into the text pass list.
        pass_index: u32,
    },
    /// Set scissor rect for subsequent draws.
    SetScissor(ScissorRect),
    /// Clear scissor (restore full viewport).
    ClearScissor,
    /// Push an offscreen render target.
    PushRenderTarget {
        /// Index into the offscreen texture list.
        texture_index: u32,
    },
    /// Pop and composite an offscreen target back.
    PopRenderTarget {
        /// Which pipeline to use for compositing.
        pipeline: PipelineId,
        /// Opacity multiplier for the composited layer.
        opacity: f32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_draw_variants() {
        let draw = BatchedDraw::Instanced {
            pipeline: PipelineId::RectInstanced,
            instance_count: 100,
        };
        assert!(matches!(draw, BatchedDraw::Instanced { .. }));
    }

    #[test]
    fn scissor_rect_default() {
        let s = ScissorRect::default();
        assert_eq!(s.x, 0);
        assert_eq!(s.width, 0);
    }

    #[test]
    fn batched_draw_indexed() {
        let draw = BatchedDraw::Indexed {
            pipeline: PipelineId::PathFill,
            index_count: 300,
        };
        assert!(matches!(draw, BatchedDraw::Indexed { index_count: 300, .. }));
    }

    #[test]
    fn batched_draw_text() {
        let draw = BatchedDraw::Text { pass_index: 0 };
        assert!(matches!(draw, BatchedDraw::Text { pass_index: 0 }));
    }

    #[test]
    fn batched_draw_scissor() {
        let scissor = ScissorRect { x: 10, y: 20, width: 100, height: 200 };
        let draw = BatchedDraw::SetScissor(scissor);
        assert!(matches!(draw, BatchedDraw::SetScissor(ScissorRect { x: 10, .. })));
    }

    #[test]
    fn batched_draw_render_target() {
        let push = BatchedDraw::PushRenderTarget { texture_index: 0 };
        let pop = BatchedDraw::PopRenderTarget {
            pipeline: PipelineId::Image,
            opacity: 0.5,
        };
        assert!(matches!(push, BatchedDraw::PushRenderTarget { .. }));
        assert!(matches!(pop, BatchedDraw::PopRenderTarget { .. }));
    }
}
