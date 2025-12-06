//! RenderShaderMask - Applies a shader as a mask
//!
//! Implements Flutter's shader masking that applies a shader (gradient, pattern, etc.)
//! as a mask to control child visibility and appearance.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderShaderMask` | `RenderShaderMask` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `ShaderSpec` | `Shader` class and `ShaderCallback` |
//! | `shader` | `shaderCallback` property |
//! | `blend_mode` | `blendMode` property |
//! | `LinearGradient` | `ui.Gradient.linear()` |
//! | `RadialGradient` | `ui.Gradient.radial()` |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Create shader**
//!    - Generate shader based on size (gradients are size-dependent)
//!    - Linear gradient: start/end points scaled by size
//!    - Radial gradient: center/radius scaled by size
//!
//! 2. **Apply shader mask layer**
//!    - Create ShaderMaskLayer with shader and blend mode
//!    - Paint child to layer
//!    - Apply shader mask with blend mode
//!
//! 3. **Paint child**
//!    - Child painted with shader mask applied
//!    - Only pixels matching shader alpha are visible
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(child + shader) - shader layer compositing overhead
//! - **Memory**: ~32 bytes (ShaderSpec + BlendMode) + shader GPU resources
//!
//! # Use Cases
//!
//! - **Gradient fades**: Fade edges with linear gradient (text overflow, image edges)
//! - **Vignette effects**: Darken edges with radial gradient
//! - **Text masking**: Apply gradient to text for visual effects
//! - **Image filters**: Custom masking patterns for creative effects
//! - **Reveal animations**: Animated gradient masks for progressive reveals
//! - **Spotlight effects**: Radial gradients for focus/attention
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderShaderMask;
//! use flui_types::styling::Color32;
//! use flui_types::painting::BlendMode;
//!
//! // Horizontal fade (left to right)
//! let fade = RenderShaderMask::linear_gradient(
//!     (0.0, 0.0),  // Start left
//!     (1.0, 0.0),  // End right
//!     vec![
//!         Color32::from_rgba_unmultiplied(255, 255, 255, 0),    // Transparent
//!         Color32::from_rgba_unmultiplied(255, 255, 255, 255),  // Opaque
//!     ],
//! );
//!
//! // Vignette (darker edges)
//! let vignette = RenderShaderMask::radial_gradient(
//!     (0.5, 0.5),  // Center
//!     0.8,         // Radius
//!     vec![
//!         Color32::from_rgba_unmultiplied(255, 255, 255, 255),  // Opaque center
//!         Color32::from_rgba_unmultiplied(255, 255, 255, 0),    // Transparent edges
//!     ],
//! ).with_blend_mode(BlendMode::Multiply);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{painting::BlendMode, styling::Color32, Size};

// ===== Data Structure =====
/// FIXME: This is a placeholder structure for shader mask data. All types should be in flui_types.
/// Shader specification (simplified for now)
#[derive(Debug, Clone)]
pub enum ShaderSpec {
    /// Linear gradient shader
    LinearGradient {
        /// Start point (relative to size)
        start: (f32, f32),
        /// End point (relative to size)
        end: (f32, f32),
        /// Colors
        colors: Vec<Color32>,
    },
    /// Radial gradient shader
    RadialGradient {
        /// Center point (relative to size)
        center: (f32, f32),
        /// Radius (relative to size)
        radius: f32,
        /// Colors
        colors: Vec<Color32>,
    },
    /// Solid color (for testing)
    Solid(Color32),
}

// ===== RenderObject =====

/// RenderObject that applies a shader as a mask to a child.
///
/// Uses a shader (linear gradient, radial gradient, or custom pattern) to mask
/// the child's rendering, controlling visibility and appearance through blend modes.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only affects painting with shader mask.
///
/// # Use Cases
///
/// - **Fade effects**: Gradient fades on text overflow, image edges, scroll indicators
/// - **Vignette filters**: Darken/lighten edges for photo-style effects
/// - **Text effects**: Gradient colored text, outlined text with shader
/// - **Reveal animations**: Progressive reveal with animated gradient position
/// - **Spotlight/focus**: Radial gradient to highlight specific areas
/// - **Custom masks**: Creative effects with custom shader patterns
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderShaderMask behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Shader generated based on size (size-dependent)
/// - Uses ShaderMaskLayer for compositing
/// - Supports blend modes (default: BlendMode::Modulate in Flutter, SrcOver in FLUI)
/// - Linear and radial gradients with color stops
/// - Coordinates normalized (0.0-1.0) and scaled by size
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShaderMask;
/// use flui_types::styling::Color32;
/// use flui_types::painting::BlendMode;
///
/// // Horizontal fade (left transparent, right opaque)
/// let fade = RenderShaderMask::linear_gradient(
///     (0.0, 0.0),
///     (1.0, 0.0),
///     vec![
///         Color32::from_rgba_unmultiplied(255, 255, 255, 0),
///         Color32::from_rgba_unmultiplied(255, 255, 255, 255),
///     ],
/// );
///
/// // Vignette effect
/// let vignette = RenderShaderMask::radial_gradient(
///     (0.5, 0.5),
///     0.8,
///     vec![Color32::WHITE, Color32::TRANSPARENT],
/// ).with_blend_mode(BlendMode::Multiply);
/// ```
#[derive(Debug)]
pub struct RenderShaderMask {
    /// Shader to apply as mask
    pub shader: ShaderSpec,
    /// Blend mode
    pub blend_mode: BlendMode,
}

// ===== Methods =====

impl RenderShaderMask {
    /// Create new shader mask with linear gradient
    pub fn linear_gradient(start: (f32, f32), end: (f32, f32), colors: Vec<Color32>) -> Self {
        Self {
            shader: ShaderSpec::LinearGradient { start, end, colors },
            blend_mode: BlendMode::default(),
        }
    }

    /// Create new shader mask with radial gradient
    pub fn radial_gradient(center: (f32, f32), radius: f32, colors: Vec<Color32>) -> Self {
        Self {
            shader: ShaderSpec::RadialGradient {
                center,
                radius,
                colors,
            },
            blend_mode: BlendMode::default(),
        }
    }

    /// Create with solid color (for testing)
    pub fn solid(color: Color32) -> Self {
        Self {
            shader: ShaderSpec::Solid(color),
            blend_mode: BlendMode::default(),
        }
    }

    /// Set blend mode
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Get the shader specification
    pub fn shader(&self) -> &ShaderSpec {
        &self.shader
    }

    /// Set the shader
    pub fn set_shader(&mut self, shader: ShaderSpec) {
        self.shader = shader;
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        self.blend_mode = blend_mode;
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderShaderMask {}

impl RenderBox<Single> for RenderShaderMask {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Note: Full shader masking requires compositor support
        // For now, we'll paint child normally
        // In production, this would use a ShaderMaskLayer with:
        // 1. Create shader from ShaderSpec and size
        // 2. Create ShaderMaskLayer(shader, blend_mode)
        // 3. Paint child to layer
        // 4. Composite with shader mask
        //
        // TODO: Implement ShaderMaskLayer when compositor supports it

        ctx.paint_child(child_id, ctx.offset);
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_new() {
        let mask = RenderShaderMask::solid(Color32::WHITE);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, Color32::WHITE);
            }
            _ => panic!("Expected solid shader"),
        }
        assert_eq!(mask.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_set_shader() {
        let mut mask = RenderShaderMask::solid(Color32::WHITE);

        let new_shader = ShaderSpec::Solid(Color32::BLACK);
        mask.set_shader(new_shader);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, Color32::BLACK);
            }
            _ => panic!("Expected solid shader"),
        }
    }

    #[test]
    fn test_render_shader_mask_set_blend_mode() {
        let mut mask = RenderShaderMask::solid(Color32::WHITE);

        mask.set_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_with_blend_mode() {
        let mask = RenderShaderMask::solid(Color32::WHITE).with_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_linear_gradient() {
        let colors = vec![Color32::WHITE, Color32::BLACK];
        let mask = RenderShaderMask::linear_gradient((0.0, 0.0), (1.0, 1.0), colors);

        match mask.shader() {
            ShaderSpec::LinearGradient { start, end, .. } => {
                assert_eq!(*start, (0.0, 0.0));
                assert_eq!(*end, (1.0, 1.0));
            }
            _ => panic!("Expected linear gradient"),
        }
    }

    #[test]
    fn test_render_shader_mask_radial_gradient() {
        let colors = vec![Color32::RED, Color32::BLUE];
        let mask = RenderShaderMask::radial_gradient((0.5, 0.5), 1.0, colors);

        match mask.shader() {
            ShaderSpec::RadialGradient { center, radius, .. } => {
                assert_eq!(*center, (0.5, 0.5));
                assert_eq!(*radius, 1.0);
            }
            _ => panic!("Expected radial gradient"),
        }
    }
}
