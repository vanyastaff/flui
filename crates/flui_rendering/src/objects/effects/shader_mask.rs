//! RenderShaderMask - Applies a shader as a mask
//!
//! This render object applies a shader (gradient, pattern, etc.) as a mask
//! to its child, controlling which parts are visible.

use flui_core::DynRenderObject;
use flui_types::{Offset, Size, Rect, constraints::BoxConstraints, painting::BlendMode};

use crate::core::{RenderBoxMixin, SingleRenderBox};
use crate::delegate_to_mixin;

// ===== Data Structure =====

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
        colors: Vec<egui::Color32>,
    },
    /// Radial gradient shader
    RadialGradient {
        /// Center point (relative to size)
        center: (f32, f32),
        /// Radius (relative to size)
        radius: f32,
        /// Colors
        colors: Vec<egui::Color32>,
    },
    /// Solid color (for testing)
    Solid(egui::Color32),
}

/// Data for RenderShaderMask
#[derive(Debug, Clone)]
pub struct ShaderMaskData {
    /// Shader to apply as mask
    pub shader: ShaderSpec,
    /// Blend mode
    pub blend_mode: BlendMode,
}

impl ShaderMaskData {
    /// Create new shader mask with linear gradient
    pub fn linear_gradient(
        start: (f32, f32),
        end: (f32, f32),
        colors: Vec<egui::Color32>,
    ) -> Self {
        Self {
            shader: ShaderSpec::LinearGradient { start, end, colors },
            blend_mode: BlendMode::default(),
        }
    }

    /// Create new shader mask with radial gradient
    pub fn radial_gradient(center: (f32, f32), radius: f32, colors: Vec<egui::Color32>) -> Self {
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
    pub fn solid(color: egui::Color32) -> Self {
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
}

// ===== Type Alias =====

/// RenderShaderMask - Applies a shader as a mask to a child
///
/// Uses a shader (gradient, pattern, etc.) to mask the child's rendering.
/// Common use cases:
/// - Gradient fades
/// - Vignette effects
/// - Custom masking patterns
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderShaderMask, ShaderMaskData};
///
/// // Create gradient fade mask
/// let data = ShaderMaskData::linear_gradient(
///     (0.0, 0.0),
///     (1.0, 0.0),
///     vec![
///         egui::Color32::from_rgba_unmultiplied(255, 255, 255, 0),
///         egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255),
///     ],
/// );
/// let mut mask = RenderShaderMask::new(data);
/// ```
pub type RenderShaderMask = SingleRenderBox<ShaderMaskData>;

// ===== Methods =====

impl RenderShaderMask {
    /// Get the shader specification
    pub fn shader(&self) -> &ShaderSpec {
        &self.data().shader
    }

    /// Set the shader
    pub fn set_shader(&mut self, shader: ShaderSpec) {
        self.data_mut().shader = shader;
        self.mark_needs_paint();
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.data().blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        if self.data().blend_mode != blend_mode {
            self.data_mut().blend_mode = blend_mode;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderShaderMask {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        *state.constraints.lock() = Some(constraints);

        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            // Layout child with same constraints
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        if let Some(size) = *state.size.lock() {
            let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
                // Note: Full shader masking requires compositor support
                // For now, we'll paint child normally and add a visual indicator
                // In production, this would use egui's shader system or a custom compositor

                ctx.paint_child(child_id, painter, offset);

                // Visual debug overlay (in production, this would be the actual shader mask)
                match &self.data().shader {
                    ShaderSpec::Solid(color) => {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(offset.dx, offset.dy),
                            egui::vec2(size.width, size.height),
                        );
                        painter.rect_filled(rect, 0.0, *color);
                    }
                    ShaderSpec::LinearGradient { .. } => {
                        // Placeholder: would draw actual gradient mask
                    }
                    ShaderSpec::RadialGradient { .. } => {
                        // Placeholder: would draw actual radial mask
                    }
                }
            }
        }
    }

    // Delegate all other methods to the mixin
    delegate_to_mixin!();
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_mask_data_linear_gradient() {
        let colors = vec![
            egui::Color32::WHITE,
            egui::Color32::BLACK,
        ];
        let data = ShaderMaskData::linear_gradient((0.0, 0.0), (1.0, 1.0), colors.clone());

        match data.shader {
            ShaderSpec::LinearGradient { start, end, colors: c } => {
                assert_eq!(start, (0.0, 0.0));
                assert_eq!(end, (1.0, 1.0));
                assert_eq!(c.len(), 2);
            }
            _ => panic!("Expected linear gradient"),
        }
        assert_eq!(data.blend_mode, BlendMode::SrcOver);
    }

    #[test]
    fn test_shader_mask_data_radial_gradient() {
        let colors = vec![egui::Color32::RED, egui::Color32::BLUE];
        let data = ShaderMaskData::radial_gradient((0.5, 0.5), 1.0, colors.clone());

        match data.shader {
            ShaderSpec::RadialGradient {
                center,
                radius,
                colors: c,
            } => {
                assert_eq!(center, (0.5, 0.5));
                assert_eq!(radius, 1.0);
                assert_eq!(c.len(), 2);
            }
            _ => panic!("Expected radial gradient"),
        }
    }

    #[test]
    fn test_shader_mask_data_solid() {
        let data = ShaderMaskData::solid(egui::Color32::RED);

        match data.shader {
            ShaderSpec::Solid(color) => {
                assert_eq!(color, egui::Color32::RED);
            }
            _ => panic!("Expected solid color"),
        }
    }

    #[test]
    fn test_shader_mask_data_with_blend_mode() {
        let data = ShaderMaskData::solid(egui::Color32::WHITE)
            .with_blend_mode(BlendMode::Multiply);

        assert_eq!(data.blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn test_shader_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_new() {
        let data = ShaderMaskData::solid(egui::Color32::WHITE);
        let mut mask = SingleRenderBox::new(data);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, egui::Color32::WHITE);
            }
            _ => panic!("Expected solid shader"),
        }
        assert_eq!(mask.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_set_shader() {
        let data = ShaderMaskData::solid(egui::Color32::WHITE);
        let mut mask = SingleRenderBox::new(data);

        let new_shader = ShaderSpec::Solid(egui::Color32::BLACK);
        mask.set_shader(new_shader);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, egui::Color32::BLACK);
            }
            _ => panic!("Expected solid shader"),
        }
        assert!(mask.needs_paint());
    }

    #[test]
    fn test_render_shader_mask_set_blend_mode() {
        use flui_core::testing::mock_render_context;

        let data = ShaderMaskData::solid(egui::Color32::WHITE);
        let mut mask = SingleRenderBox::new(data);

        // Do layout first to clear initial needs_paint
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let (_tree, ctx) = mock_render_context();
        mask.layout(constraints, &ctx);

        mask.set_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
        assert!(mask.needs_paint());
    }

    #[test]
    fn test_render_shader_mask_layout() {
        use flui_core::testing::mock_render_context;

        let data = ShaderMaskData::solid(egui::Color32::WHITE);
        let mut mask = SingleRenderBox::new(data);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let size = mask.layout(constraints, &ctx);

        // Without child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
        assert_eq!(mask.size(), Size::new(0.0, 0.0));
    }
}
