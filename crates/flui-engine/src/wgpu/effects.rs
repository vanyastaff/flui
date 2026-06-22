// GPU-accelerated visual effects for FLUI
//
// This module provides types and utilities for advanced rendering effects:
// - Gradients (linear, radial)
// - Shadows (drop shadows, elevation)
// - Blur (Dual Kawase)
//
// All types are designed for GPU instancing and batching.

use bytemuck::{Pod, Zeroable};
use flui_types::styling::Color;
use glam::Vec2;

// =============================================================================
// Gradient Types
// =============================================================================

/// A single color stop in a gradient
///
/// Gradients are defined by a series of stops, each with a color and position.
/// Colors are linearly interpolated between stops.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GradientStop {
    /// RGBA color (0.0 - 1.0 range)
    pub color: [f32; 4],
    /// Position along gradient (0.0 = start, 1.0 = end)
    pub position: f32,
    /// Padding for GPU alignment
    pub padding: [f32; 3],
}

impl GradientStop {
    /// Create a new gradient stop
    pub fn new(color: Color, position: f32) -> Self {
        Self {
            color: color.to_rgba_f32().into(),
            position: position.clamp(0.0, 1.0),
            padding: [0.0; 3],
        }
    }

    /// Create a stop at the start (position = 0.0)
    pub fn start(color: Color) -> Self {
        Self::new(color, 0.0)
    }

    /// Create a stop at the end (position = 1.0)
    pub fn end(color: Color) -> Self {
        Self::new(color, 1.0)
    }
}

/// Linear gradient instance data for GPU instancing
///
/// Each instance represents one gradient-filled rectangle.
/// Multiple instances can be batched into a single draw call.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct LinearGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Gradient start point (local coordinates)
    pub gradient_start: [f32; 2],
    /// Gradient end point (local coordinates)
    pub gradient_end: [f32; 2],
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of gradient stops (1-8)
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Padding for GPU alignment
    pub padding: [u32; 2],
}

impl LinearGradientInstance {
    /// Create a new linear gradient instance
    pub fn new(
        bounds: [f32; 4],
        start: Vec2,
        end: Vec2,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            gradient_start: [start.x, start.y],
            gradient_end: [end.x, end.y],
            corner_radii,
            stop_count: stop_count.min(8),
            stop_offset: 0,
            padding: [0; 2],
        }
    }

    /// Set the offset into the shared gradient stops buffer
    pub fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }

    /// Create a vertical gradient (top to bottom)
    pub fn vertical(bounds: [f32; 4], corner_radii: [f32; 4], stop_count: u32) -> Self {
        let height = bounds[3];
        Self::new(
            bounds,
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, height),
            corner_radii,
            stop_count,
        )
    }

    /// Create a horizontal gradient (left to right)
    pub fn horizontal(bounds: [f32; 4], corner_radii: [f32; 4], stop_count: u32) -> Self {
        let width = bounds[2];
        Self::new(
            bounds,
            Vec2::new(0.0, 0.0),
            Vec2::new(width, 0.0),
            corner_radii,
            stop_count,
        )
    }

    /// Create a diagonal gradient (top-left to bottom-right)
    pub fn diagonal(bounds: [f32; 4], corner_radii: [f32; 4], stop_count: u32) -> Self {
        let width = bounds[2];
        let height = bounds[3];
        Self::new(
            bounds,
            Vec2::new(0.0, 0.0),
            Vec2::new(width, height),
            corner_radii,
            stop_count,
        )
    }
}

/// Radial gradient instance data for GPU instancing
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RadialGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Gradient center point (local coordinates)
    pub center: [f32; 2],
    /// Gradient radius
    pub radius: f32,
    /// Padding
    pub padding1: f32,
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of gradient stops (1-8)
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Padding for GPU alignment
    pub padding2: [u32; 2],
}

impl RadialGradientInstance {
    /// Create a new radial gradient instance
    pub fn new(
        bounds: [f32; 4],
        center: Vec2,
        radius: f32,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            center: [center.x, center.y],
            radius,
            padding1: 0.0,
            corner_radii,
            stop_count: stop_count.min(8),
            stop_offset: 0,
            padding2: [0; 2],
        }
    }

    /// Set the offset into the shared gradient stops buffer
    pub fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }

    /// Create a radial gradient centered in the rectangle
    pub fn centered(
        bounds: [f32; 4],
        radius: f32,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        let width = bounds[2];
        let height = bounds[3];
        let center = Vec2::new(width * 0.5, height * 0.5);
        Self::new(bounds, center, radius, corner_radii, stop_count)
    }
}

/// Sweep (angular/conic) gradient instance data for GPU instancing
///
/// Layout matches the sweep.wgsl InstanceInput struct.
/// Each instance represents one sweep-gradient-filled rectangle.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SweepGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Gradient center point (local coordinates)
    pub center: [f32; 2],
    /// Start and end angles in radians [start_angle, end_angle]
    pub angles: [f32; 2],
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of gradient stops (1-8)
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Padding for GPU alignment
    pub padding: [u32; 2],
}

impl SweepGradientInstance {
    /// Create a new sweep gradient instance
    pub fn new(
        bounds: [f32; 4],
        center: Vec2,
        start_angle: f32,
        end_angle: f32,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            center: [center.x, center.y],
            angles: [start_angle, end_angle],
            corner_radii,
            stop_count: stop_count.min(8),
            stop_offset: 0,
            padding: [0; 2],
        }
    }

    /// Set the offset into the shared gradient stops buffer
    pub fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }

    /// Create a full-circle sweep gradient (0 to 2*PI) centered in the rectangle
    pub fn full_circle(bounds: [f32; 4], corner_radii: [f32; 4], stop_count: u32) -> Self {
        let width = bounds[2];
        let height = bounds[3];
        let center = Vec2::new(width * 0.5, height * 0.5);
        Self::new(
            bounds,
            center,
            0.0,
            std::f32::consts::TAU,
            corner_radii,
            stop_count,
        )
    }
}

// =============================================================================
// Shadow Types
// =============================================================================

/// Shadow parameters for Material Design elevation levels
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

    // `elevation_1` ... `elevation_5` constructor shortcuts were deleted in
    // cycle 4 E-4 — they had zero non-test consumers across the workspace
    // (the only docstring reference at `painter.rs:3938` was migrated to
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

// Blur uniform-parameter buffer (`BlurParams`) + `BlurIntensity` shorthand
// enum + `LinearGradientBuilder` fluent-API helper were deleted in cycle 4
// E-4: zero non-test consumers across the workspace, and the live blur
// uniform struct (`offscreen::BlurParams`) has a different field shape that
// the parallel `effects::BlurParams` never adopted. When a public blur API
// lands on `WgpuPainter`, it will reach for the offscreen-side struct
// directly. `LinearGradientBuilder` returned a `Vec<GradientStop>` capped
// at 8 elements; consumers can build the same vector inline without the
// builder ceremony.

// =============================================================================
// Blur tap-count helper
// =============================================================================

/// Impeller's kernel-radius-per-sigma constant (`kKernelRadiusPerSigma`, sigma.h:24).
///
/// Value: √3 ≈ 1.732 050 8. Chosen so the Gaussian evaluated at ±radius drops
/// below ½ of its peak value — the Impeller standard for "sufficient tap coverage".
///
/// (Impeller's exact formula is `(sigma - 0.5) × √3`; we omit the `−0.5` as a
/// conservative over-estimate documented in the spec.)
const KERNEL_RADIUS_PER_SIGMA: f32 = 1.732_050_8;

/// Gaussian-blur kernel radius (in source pixels) for a given Gaussian sigma.
///
/// Computes `ceil(sigma × √3)` — Impeller's `CalculateBlurRadius` from
/// `impeller/geometry/sigma.h:24`. The integer result is both:
///
/// - the **sampling extent** per sub-pass (H or V scans `[-r..=r]` texels), and
/// - the **coverage radius** for `grown_bounds` expansion in `restore_layer`.
///
/// The full kernel spans `2 × kernel_radius + 1` taps.
///
/// Returns `0` for non-positive sigma (degenerate / no blur).
///
/// Single authoritative home for the blur-pass driver (`apply_blur`) and the
/// CPU oracle in `blur_filter_tests` — do NOT compute `ceil(sigma * N)` inline
/// at other call sites.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // non-negative result: sigma > 0.0 guard ensures (sigma * √3).ceil() ≥ 0;
    // truncation: u32::MAX ≈ 4.3 × 10^9, overflowable only at sigma > ~2.5 × 10^9 px
)]
#[must_use]
pub(crate) fn kernel_radius(sigma: f32) -> u32 {
    if sigma <= 0.0 {
        return 0;
    }
    (sigma * KERNEL_RADIUS_PER_SIGMA).ceil() as u32
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use super::*;

    #[test]
    fn test_gradient_stop_creation() {
        let stop = GradientStop::new(Color::RED, 0.5);
        assert_eq!(stop.position, 0.5);
        assert_eq!(stop.color, [1.0, 0.0, 0.0, 1.0]);
    }

    // `test_gradient_builder`, `test_shadow_elevation_levels`, and
    // `test_blur_intensity` were removed in cycle 4 E-4 alongside the
    // `LinearGradientBuilder`, `ShadowParams::elevation_*`, and
    // `BlurIntensity` items they exercised. The remaining `GradientStop`
    // smoke test covers the only public API in this module that has live
    // consumers (`painter.rs`'s instanced-gradient pipeline).
}

/// CPU-only tests for `kernel_radius`. These run in CI without a GPU.
#[cfg(test)]
mod kernel_radius_tests {
    use super::kernel_radius;

    /// `kernel_radius(0.0)` must return 0 (degenerate — no blur).
    #[test]
    fn zero_sigma_returns_zero() {
        assert_eq!(kernel_radius(0.0), 0);
    }

    /// Negative sigma is treated as no-blur.
    #[test]
    fn negative_sigma_returns_zero() {
        assert_eq!(kernel_radius(-1.0), 0);
    }

    /// `kernel_radius` must be monotonically non-decreasing as sigma grows.
    ///
    /// Tests sigma = 0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0.
    #[test]
    fn monotonically_nondecreasing() {
        let sigmas = [0.1_f32, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0];
        let radii: Vec<u32> = sigmas.iter().map(|&s| kernel_radius(s)).collect();
        for window in radii.windows(2) {
            assert!(
                window[0] <= window[1],
                "kernel_radius not monotone: sigma pair produced radii {} > {}",
                window[0],
                window[1]
            );
        }
    }

    /// sigma = 2.0 → ceil(2.0 × 1.732_050_8) = ceil(3.464_101_6) = 4.
    ///
    /// This is the known-value anchor from the spec: the chief-architect
    /// table entry `(2.0) == 4`.
    #[test]
    fn sigma_two_gives_radius_four() {
        assert_eq!(kernel_radius(2.0), 4);
    }

    /// sigma = 1.0 → ceil(1.0 × 1.732_050_8) = ceil(1.732_050_8) = 2.
    #[test]
    fn sigma_one_gives_radius_two() {
        assert_eq!(kernel_radius(1.0), 2);
    }
}
