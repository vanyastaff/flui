//! Drop-shadow parameters and their GPU instance payload.

use bytemuck::{Pod, Zeroable};
use flui_types::styling::Color;
use glam::Vec2;

// =============================================================================
// Shadow Types
// =============================================================================

/// Shadow parameters for Material Design elevation levels
///
/// `#[non_exhaustive]`: a drop-shadow parameter set grows (spread, offset
/// ratio, per-corner radius), and a caller-built literal would break on the
/// next field. Build one through [`Self::new`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug)]
pub struct ShadowParams {
    /// Shadow offset (x, y)
    pub offset: Vec2,
    /// Blur sigma (standard deviation)
    pub blur_sigma: f32,
    /// Shadow color (usually black with alpha)
    pub color: Color,
}

impl ShadowParams {
    /// Create custom shadow parameters
    pub fn new(offset: Vec2, blur_sigma: f32, color: Color) -> Self {
        Self {
            offset,
            blur_sigma,
            color,
        }
    }

    // `elevation_1` ... `elevation_5` constructor shortcuts were deleted —
    // they had zero non-test consumers across the workspace
    // (the only docstring reference was migrated to
    // `ShadowParams::new(...)` literal-construction in the same commit).
    // The Material Design elevation curves they encoded were a higher-level
    // theming concern that does not belong inside the GPU instancing crate;
    // when a widget-level theming layer materializes, the elevation→sigma
    // mapping lands there with the rest of the design tokens.
}

/// Shadow instance data for GPU instancing
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ShadowInstance {
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
    pub fn new(
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &ShadowParams,
    ) -> Self {
        // 3-sigma rule: 99.7% of Gaussian is within 3σ
        let expand = params.blur_sigma * 3.0;

        // Calculate shadow bounds (expanded for blur)
        let shadow_x = rect_pos[0] + params.offset.x - expand;
        let shadow_y = rect_pos[1] + params.offset.y - expand;
        let shadow_width = rect_size[0] + expand * 2.0;
        let shadow_height = rect_size[1] + expand * 2.0;

        Self {
            bounds: [shadow_x, shadow_y, shadow_width, shadow_height],
            rect_pos,
            rect_size,
            corner_radius,
            padding1: [0.0; 3],
            shadow_offset: [params.offset.x, params.offset.y],
            blur_sigma: params.blur_sigma,
            padding2: 0.0,
            shadow_color: params.color.to_rgba_f32().into(),
        }
    }
}
