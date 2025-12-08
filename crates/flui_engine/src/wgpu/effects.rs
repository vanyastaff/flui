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
    pub _padding: [f32; 3],
}

impl GradientStop {
    /// Create a new gradient stop
    pub fn new(color: Color, position: f32) -> Self {
        Self {
            color: color.to_rgba_f32().into(),
            position: position.clamp(0.0, 1.0),
            _padding: [0.0; 3],
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
    /// Padding for GPU alignment
    pub _padding: [u32; 3],
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
            stop_count: stop_count.min(8), // Max 8 stops
            _padding: [0; 3],
        }
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
    pub _padding1: f32,
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of gradient stops (1-8)
    pub stop_count: u32,
    /// Padding for GPU alignment
    pub _padding2: [u32; 3],
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
            _padding1: 0.0,
            corner_radii,
            stop_count: stop_count.min(8),
            _padding2: [0; 3],
        }
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

    /// Material Design Elevation 1 (subtle depth)
    pub fn elevation_1() -> Self {
        Self {
            offset: Vec2::new(0.0, 1.0),
            blur_sigma: 2.0,
            color: Color::rgba(0, 0, 0, 31), // ~0.12 alpha
        }
    }

    /// Material Design Elevation 2 (raised surface)
    pub fn elevation_2() -> Self {
        Self {
            offset: Vec2::new(0.0, 2.0),
            blur_sigma: 4.0,
            color: Color::rgba(0, 0, 0, 41), // ~0.16 alpha
        }
    }

    /// Material Design Elevation 3 (floating surface)
    pub fn elevation_3() -> Self {
        Self {
            offset: Vec2::new(0.0, 4.0),
            blur_sigma: 8.0,
            color: Color::rgba(0, 0, 0, 51), // ~0.20 alpha
        }
    }

    /// Material Design Elevation 4 (dialog)
    pub fn elevation_4() -> Self {
        Self {
            offset: Vec2::new(0.0, 8.0),
            blur_sigma: 12.0,
            color: Color::rgba(0, 0, 0, 61), // ~0.24 alpha
        }
    }

    /// Material Design Elevation 5 (modal)
    pub fn elevation_5() -> Self {
        Self {
            offset: Vec2::new(0.0, 16.0),
            blur_sigma: 16.0,
            color: Color::rgba(0, 0, 0, 71), // ~0.28 alpha
        }
    }
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
    pub _padding1: [f32; 3],
    /// Shadow offset
    pub shadow_offset: [f32; 2],
    /// Blur sigma
    pub blur_sigma: f32,
    /// Padding
    pub _padding2: f32,
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
            _padding1: [0.0; 3],
            shadow_offset: [params.offset.x, params.offset.y],
            blur_sigma: params.blur_sigma,
            _padding2: 0.0,
            shadow_color: params.color.to_rgba_f32().into(),
        }
    }
}

// =============================================================================
// Blur Types
// =============================================================================

/// Blur parameters for Dual Kawase algorithm
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct BlurParams {
    /// Input texture size
    pub texture_size: [f32; 2],
    /// Sample offset multiplier
    pub offset: f32,
    /// Padding for GPU alignment
    pub _padding: f32,
}

impl BlurParams {
    /// Create new blur parameters
    pub fn new(texture_size: [f32; 2], offset: f32) -> Self {
        Self {
            texture_size,
            offset,
            _padding: 0.0,
        }
    }
}

/// Blur intensity levels
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlurIntensity {
    /// Light blur (~4px radius) - glass effect
    Light,
    /// Medium blur (~8px radius) - backdrop
    Medium,
    /// Heavy blur (~16px radius) - strong glass
    Heavy,
    /// Extreme blur (~32px radius) - background defocus
    Extreme,
}

impl BlurIntensity {
    /// Get the number of iterations for this intensity
    pub fn iterations(self) -> u32 {
        match self {
            BlurIntensity::Light => 1,
            BlurIntensity::Medium => 2,
            BlurIntensity::Heavy => 3,
            BlurIntensity::Extreme => 4,
        }
    }

    /// Get approximate blur radius in pixels
    pub fn radius(self) -> f32 {
        match self {
            BlurIntensity::Light => 4.0,
            BlurIntensity::Medium => 8.0,
            BlurIntensity::Heavy => 16.0,
            BlurIntensity::Extreme => 32.0,
        }
    }
}

// =============================================================================
// Builder Patterns
// =============================================================================

/// Builder for linear gradients with fluent API
pub struct LinearGradientBuilder {
    stops: Vec<GradientStop>,
}

impl LinearGradientBuilder {
    /// Create a new gradient builder
    pub fn new() -> Self {
        Self { stops: Vec::new() }
    }

    /// Add a color stop
    pub fn add_stop(mut self, color: Color, position: f32) -> Self {
        self.stops.push(GradientStop::new(color, position));
        self
    }

    /// Add a stop at the start (position = 0.0)
    pub fn start(mut self, color: Color) -> Self {
        self.stops.push(GradientStop::start(color));
        self
    }

    /// Add a stop at the end (position = 1.0)
    pub fn end(mut self, color: Color) -> Self {
        self.stops.push(GradientStop::end(color));
        self
    }

    /// Build the gradient stops (max 8)
    pub fn build(self) -> Vec<GradientStop> {
        self.stops.into_iter().take(8).collect()
    }
}

impl Default for LinearGradientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gradient_stop_creation() {
        let stop = GradientStop::new(Color::RED, 0.5);
        assert_eq!(stop.position, 0.5);
        assert_eq!(stop.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_gradient_builder() {
        let stops = LinearGradientBuilder::new()
            .start(Color::RED)
            .end(Color::BLUE)
            .build();

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].position, 0.0);
        assert_eq!(stops[1].position, 1.0);
    }

    #[test]
    fn test_shadow_elevation_levels() {
        let shadow1 = ShadowParams::elevation_1();
        let shadow3 = ShadowParams::elevation_3();

        assert!(shadow3.blur_sigma > shadow1.blur_sigma);
        assert!(shadow3.offset.y > shadow1.offset.y);
    }

    #[test]
    fn test_blur_intensity() {
        assert_eq!(BlurIntensity::Light.iterations(), 1);
        assert_eq!(BlurIntensity::Extreme.iterations(), 4);
        assert!(BlurIntensity::Heavy.radius() > BlurIntensity::Light.radius());
    }
}
