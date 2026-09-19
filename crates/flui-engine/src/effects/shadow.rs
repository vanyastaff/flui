//! Drop-shadow parameters and their GPU instance payload.

use bytemuck::{Pod, Zeroable};
use flui_types::styling::Color;
use glam::Vec2;

// =============================================================================
// Shadow Types
// =============================================================================

/// A drop shadow: offset, Gaussian blur, colour.
///
/// Fields are private so the set can grow (spread, per-corner radius) with
/// `with_*` methods and so `blur_sigma` cannot be set to a negative or `NaN`
/// value after construction; [`Self::new`] normalises it.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ShadowParams {
    offset: Vec2,
    blur_sigma: f32,
    color: Color,
}

impl ShadowParams {
    /// A shadow offset by `offset` device pixels with Gaussian `blur_sigma`
    /// (clamped to `>= 0.0`; `NaN` becomes `0.0`) in `color`.
    #[must_use]
    pub(crate) fn new(offset: Vec2, blur_sigma: f32, color: Color) -> Self {
        Self {
            offset,
            blur_sigma: if blur_sigma.is_nan() {
                0.0
            } else {
                blur_sigma.max(0.0)
            },
            color,
        }
    }

    /// The shadow's offset in device pixels.
    #[must_use]
    pub(crate) fn offset(&self) -> Vec2 {
        self.offset
    }

    /// The Gaussian blur's standard deviation, `>= 0.0`.
    #[must_use]
    pub(crate) fn blur_sigma(&self) -> f32 {
        self.blur_sigma
    }

    /// The shadow colour.
    #[must_use]
    pub(crate) fn color(&self) -> Color {
        self.color
    }
}

/// Shadow instance data for GPU instancing
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct ShadowInstance {
    /// Shadow bounds (expanded by blur radius)
    pub bounds: [f32; 4],
    /// Actual rectangle position
    pub rect_pos: [f32; 2],
    /// Actual rectangle size
    pub rect_size: [f32; 2],
    /// Corner radius (uniform for now)
    pub corner_radius: f32,
    /// Padding
    pub padding1: [f32; 3],
    /// Shadow offset
    pub shadow_offset: [f32; 2],
    /// Blur sigma
    pub blur_sigma: f32,
    /// Padding
    pub padding2: f32,
    /// Shadow color
    pub shadow_color: [f32; 4],
}

impl ShadowInstance {
    /// Create a new shadow instance
    ///
    /// Automatically calculates expanded shadow bounds using 3-sigma rule.
    pub(crate) fn new(
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &ShadowParams,
    ) -> Self {
        // 3-sigma rule: 99.7% of Gaussian is within 3σ
        let expand = params.blur_sigma() * 3.0;

        // Calculate shadow bounds (expanded for blur)
        let shadow_x = rect_pos[0] + params.offset().x - expand;
        let shadow_y = rect_pos[1] + params.offset().y - expand;
        let shadow_width = rect_size[0] + expand * 2.0;
        let shadow_height = rect_size[1] + expand * 2.0;

        Self {
            bounds: [shadow_x, shadow_y, shadow_width, shadow_height],
            rect_pos,
            rect_size,
            corner_radius,
            padding1: [0.0; 3],
            shadow_offset: [params.offset().x, params.offset().y],
            blur_sigma: params.blur_sigma(),
            padding2: 0.0,
            shadow_color: params.color().to_rgba_f32().into(),
        }
    }
}
