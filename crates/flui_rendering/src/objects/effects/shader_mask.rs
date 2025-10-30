//! RenderShaderMask - Applies a shader as a mask
//!
//! This render object applies a shader (gradient, pattern, etc.) as a mask
//! to its child_id, controlling which parts are visible.

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Size, painting::BlendMode, Offset, constraints::BoxConstraints};

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
    pub fn linear_gradient(start: (f32, f32), end: (f32, f32), colors: Vec<egui::Color32>) -> Self {
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

// ===== RenderObject =====

/// RenderShaderMask - Applies a shader as a mask to a child_id
///
/// Uses a shader (gradient, pattern, etc.) to mask the child_id's rendering.
/// Common use cases:
/// - Gradient fades
/// - Vignette effects
/// - Custom masking patterns
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShaderMask;
///
/// // Create gradient fade mask
/// let mask = RenderShaderMask::linear_gradient(
///     (0.0, 0.0),
///     (1.0, 0.0),
///     vec![
///         egui::Color32::from_rgba_unmultiplied(255, 255, 255, 0),
///         egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255),
///     ],
/// );
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
    pub fn linear_gradient(start: (f32, f32), end: (f32, f32), colors: Vec<egui::Color32>) -> Self {
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

impl SingleRender for RenderShaderMask {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child_id with same constraints
                tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Capture child_id layer
                // Note: Full shader masking requires compositor support
        // For now, we'll paint child_id normally
        // In production, this would use a ShaderMaskLayer with egui's shader system
        // or a custom compositor
        //
        // TODO: Implement ShaderMaskLayer when compositor supports it

        (tree.paint_child(child_id, offset)) as _
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_mask_data_linear_gradient() {
        let colors = vec![egui::Color32::WHITE, egui::Color32::BLACK];
        let data = ShaderMaskData::linear_gradient((0.0, 0.0), (1.0, 1.0), colors.clone());

        match data.shader {
            ShaderSpec::LinearGradient {
                start,
                end,
                colors: c,
            } => {
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
        let data = ShaderMaskData::solid(egui::Color32::WHITE).with_blend_mode(BlendMode::Multiply);

        assert_eq!(data.blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn test_shader_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_new() {
        let mask = RenderShaderMask::solid(egui::Color32::WHITE);

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
        let mut mask = RenderShaderMask::solid(egui::Color32::WHITE);

        let new_shader = ShaderSpec::Solid(egui::Color32::BLACK);
        mask.set_shader(new_shader);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, egui::Color32::BLACK);
            }
            _ => panic!("Expected solid shader"),
        }
    }

    #[test]
    fn test_render_shader_mask_set_blend_mode() {
        let mut mask = RenderShaderMask::solid(egui::Color32::WHITE);

        mask.set_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_with_blend_mode() {
        let mask =
            RenderShaderMask::solid(egui::Color32::WHITE).with_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_linear_gradient() {
        let colors = vec![egui::Color32::WHITE, egui::Color32::BLACK];
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
        let colors = vec![egui::Color32::RED, egui::Color32::BLUE];
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
