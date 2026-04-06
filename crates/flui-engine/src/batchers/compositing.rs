//! Compositing and layer blending operations.
//!
//! Manages offscreen render target operations for SaveLayer/RestoreLayer,
//! ShaderMask, BackdropFilter, etc. This is CPU-side tracking only — actual
//! texture allocation happens later when GpuDevice is available.

/// Filter type for image and backdrop filter operations.
#[derive(Clone, Debug)]
pub enum FilterType {
    /// Gaussian blur with independent sigma per axis.
    Blur {
        /// Horizontal blur radius.
        sigma_x: f32,
        /// Vertical blur radius.
        sigma_y: f32,
    },
    /// Dilation (expand) filter.
    Dilate {
        /// Horizontal dilation radius.
        radius_x: f32,
        /// Vertical dilation radius.
        radius_y: f32,
    },
    /// Erosion (shrink) filter.
    Erode {
        /// Horizontal erosion radius.
        radius_x: f32,
        /// Vertical erosion radius.
        radius_y: f32,
    },
    /// 5x4 color matrix transform.
    Matrix {
        /// The 5x4 color matrix as a flat array of 20 floats.
        matrix: [f32; 20],
    },
}

/// A compositing operation to be submitted to the GPU.
#[derive(Clone, Debug)]
pub enum CompositingOp {
    /// Begin rendering to an offscreen target.
    PushTarget {
        /// Bounding rectangle: x, y, width, height.
        bounds: [f32; 4],
        /// Layer opacity (0.0–1.0).
        opacity: f32,
        /// Numeric blend mode (maps to wgpu blend state later).
        blend_mode: u32,
    },
    /// Composite offscreen target back to parent.
    PopTarget,
    /// Apply shader mask to current offscreen target.
    ShaderMask {
        /// Bounding rectangle: x, y, width, height.
        bounds: [f32; 4],
        /// Numeric blend mode.
        blend_mode: u32,
    },
    /// Apply backdrop filter (read from parent, filter, composite).
    BackdropFilter {
        /// Bounding rectangle: x, y, width, height.
        bounds: [f32; 4],
        /// Filter to apply.
        filter_type: FilterType,
    },
    /// Apply color filter to current offscreen target.
    ColorFilter {
        /// Bounding rectangle: x, y, width, height.
        bounds: [f32; 4],
    },
    /// Apply image filter (blur etc) to current offscreen target.
    ImageFilter {
        /// Bounding rectangle: x, y, width, height.
        bounds: [f32; 4],
        /// Filter to apply.
        filter_type: FilterType,
    },
}

/// Collects compositing operations for offscreen render targets.
///
/// Tracks push/pop nesting depth and accumulates operations for later
/// GPU submission. No GPU resources are allocated here.
pub struct CompositingBatcher {
    ops: Vec<CompositingOp>,
    target_depth: u32,
}

impl CompositingBatcher {
    /// Create a new empty compositing batcher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            target_depth: 0,
        }
    }

    /// Begin rendering to an offscreen target, incrementing nesting depth.
    pub fn push_target(&mut self, bounds: [f32; 4], opacity: f32, blend_mode: u32) {
        self.ops.push(CompositingOp::PushTarget {
            bounds,
            opacity,
            blend_mode,
        });
        self.target_depth += 1;
    }

    /// Composite offscreen target back to parent, decrementing nesting depth.
    ///
    /// Logs a warning if called when depth is already zero (underflow).
    pub fn pop_target(&mut self) {
        if self.target_depth == 0 {
            tracing::warn!("pop_target called at depth 0 — ignoring to prevent underflow");
            return;
        }
        self.ops.push(CompositingOp::PopTarget);
        self.target_depth -= 1;
    }

    /// Add a shader mask operation.
    pub fn add_shader_mask(&mut self, bounds: [f32; 4], blend_mode: u32) {
        self.ops
            .push(CompositingOp::ShaderMask { bounds, blend_mode });
    }

    /// Add a backdrop filter operation.
    pub fn add_backdrop_filter(&mut self, bounds: [f32; 4], filter: FilterType) {
        self.ops.push(CompositingOp::BackdropFilter {
            bounds,
            filter_type: filter,
        });
    }

    /// Add a color filter operation.
    pub fn add_color_filter(&mut self, bounds: [f32; 4]) {
        self.ops.push(CompositingOp::ColorFilter { bounds });
    }

    /// Add an image filter operation.
    pub fn add_image_filter(&mut self, bounds: [f32; 4], filter: FilterType) {
        self.ops.push(CompositingOp::ImageFilter {
            bounds,
            filter_type: filter,
        });
    }

    /// Returns a slice of all accumulated compositing operations.
    #[must_use]
    pub fn ops(&self) -> &[CompositingOp] {
        &self.ops
    }

    /// Returns the number of accumulated operations.
    #[must_use]
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// Returns the current push/pop nesting depth.
    #[must_use]
    pub fn target_depth(&self) -> u32 {
        self.target_depth
    }

    /// Returns `true` if no operations have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Clear all operations and reset depth, keeping allocated memory.
    pub fn clear(&mut self) {
        self.ops.clear();
        self.target_depth = 0;
    }
}

impl Default for CompositingBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_compositing_batcher() {
        let batcher = CompositingBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.op_count(), 0);
        assert_eq!(batcher.target_depth(), 0);
    }

    #[test]
    fn push_pop_target() {
        let mut batcher = CompositingBatcher::new();
        batcher.push_target([0.0, 0.0, 100.0, 100.0], 1.0, 0);
        assert_eq!(batcher.target_depth(), 1);
        assert_eq!(batcher.op_count(), 1);

        batcher.pop_target();
        assert_eq!(batcher.target_depth(), 0);
        assert_eq!(batcher.op_count(), 2);
    }

    #[test]
    fn nested_targets() {
        let mut batcher = CompositingBatcher::new();
        batcher.push_target([0.0, 0.0, 200.0, 200.0], 0.8, 0);
        batcher.push_target([10.0, 10.0, 50.0, 50.0], 0.5, 1);
        assert_eq!(batcher.target_depth(), 2);

        batcher.pop_target();
        assert_eq!(batcher.target_depth(), 1);
        batcher.pop_target();
        assert_eq!(batcher.target_depth(), 0);
    }

    #[test]
    fn pop_at_zero_depth_stays_zero() {
        let mut batcher = CompositingBatcher::new();
        // Pop without any push — should warn and stay at depth 0.
        batcher.pop_target();
        assert_eq!(batcher.target_depth(), 0);
        // The underflow pop should not produce an op.
        assert_eq!(batcher.op_count(), 0);
    }

    #[test]
    fn add_operations() {
        let mut batcher = CompositingBatcher::new();
        batcher.add_shader_mask([0.0, 0.0, 100.0, 100.0], 0);
        batcher.add_backdrop_filter(
            [0.0, 0.0, 100.0, 100.0],
            FilterType::Blur {
                sigma_x: 5.0,
                sigma_y: 5.0,
            },
        );
        assert_eq!(batcher.op_count(), 2);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn clear_resets() {
        let mut batcher = CompositingBatcher::new();
        batcher.push_target([0.0, 0.0, 100.0, 100.0], 1.0, 0);
        batcher.add_color_filter([0.0, 0.0, 50.0, 50.0]);
        batcher.add_image_filter(
            [10.0, 10.0, 30.0, 30.0],
            FilterType::Erode {
                radius_x: 2.0,
                radius_y: 2.0,
            },
        );
        assert!(!batcher.is_empty());
        assert_eq!(batcher.target_depth(), 1);

        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.op_count(), 0);
        assert_eq!(batcher.target_depth(), 0);
    }
}
