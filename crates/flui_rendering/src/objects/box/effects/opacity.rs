//! RenderOpacity - applies alpha transparency to its child.
//!
//! This render object multiplies its child's opacity by a given value,
//! creating transparency effects.

use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ProxyBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that applies opacity to its child.
///
/// Opacity values should be between 0.0 (fully transparent) and 1.0 (fully opaque).
/// Values outside this range are clamped.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderOpacity;
///
/// // 50% opacity
/// let mut opacity = RenderOpacity::new(0.5);
///
/// // Fully transparent (invisible)
/// let mut hidden = RenderOpacity::new(0.0);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The opacity value (0.0 to 1.0).
    opacity: f32,

    /// Whether the child should be included in hit testing when invisible.
    always_include_semantics: bool,
}

impl RenderOpacity {
    /// Creates a new opacity render object.
    ///
    /// The opacity is clamped to [0.0, 1.0].
    pub fn new(opacity: f32) -> Self {
        Self {
            proxy: ProxyBox::new(),
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
        }
    }

    /// Creates a fully opaque render object.
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully transparent render object.
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// The value is clamped to [0.0, 1.0].
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() > f32::EPSILON {
            self.opacity = clamped;
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Returns whether semantics are always included.
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Sets whether semantics should always be included.
    pub fn set_always_include_semantics(&mut self, value: bool) {
        if self.always_include_semantics != value {
            self.always_include_semantics = value;
            // In real implementation: self.mark_needs_semantics_update();
        }
    }

    /// Returns whether the child is effectively invisible.
    pub fn is_invisible(&self) -> bool {
        self.opacity < 0.001
    }

    /// Returns whether the opacity creates any effect.
    pub fn is_opaque(&self) -> bool {
        self.opacity > 0.999
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.proxy.set_geometry(child_size);
        child_size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Paints this render object.
    ///
    /// If opacity is 0, nothing is painted.
    /// If opacity is 1, child is painted directly.
    /// Otherwise, child is painted to a layer with the given opacity.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.is_invisible() {
            // Don't paint anything
            return;
        }

        if self.is_opaque() {
            // Paint child directly (no layer needed)
            let _ = (context, offset);
            // In real implementation: context.paint_child(child, offset);
        } else {
            // Paint child through opacity layer
            // In real implementation:
            // context.push_opacity(offset, (self.opacity * 255.0) as i32, |ctx| {
            //     ctx.paint_child(child, offset);
            // });
            let _ = (context, offset);
        }
    }

    /// Hit test - passes through to child unless invisible and not always including semantics.
    pub fn hit_test(&self, position: Offset) -> bool {
        if self.is_invisible() && !self.always_include_semantics {
            return false;
        }
        // In real implementation, would delegate to child
        let _ = position;
        true
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        child_width
            .unwrap_or(0.0)
            .max(0.0)
            .min(height * 0.0 + f32::MAX)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert!((opacity.opacity() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_opacity_clamping() {
        let under = RenderOpacity::new(-0.5);
        assert!((under.opacity() - 0.0).abs() < f32::EPSILON);

        let over = RenderOpacity::new(1.5);
        assert!((over.opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_opacity_opaque() {
        let opaque = RenderOpacity::opaque();
        assert!(opaque.is_opaque());
        assert!(!opaque.is_invisible());
    }

    #[test]
    fn test_opacity_transparent() {
        let transparent = RenderOpacity::transparent();
        assert!(transparent.is_invisible());
        assert!(!transparent.is_opaque());
    }

    #[test]
    fn test_opacity_layout() {
        let mut opacity = RenderOpacity::new(0.5);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = opacity.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }

    #[test]
    fn test_hit_test_invisible() {
        let opacity = RenderOpacity::transparent();
        assert!(!opacity.hit_test(Offset::ZERO));
    }

    #[test]
    fn test_hit_test_always_include() {
        let mut opacity = RenderOpacity::transparent();
        opacity.set_always_include_semantics(true);
        assert!(opacity.hit_test(Offset::ZERO));
    }
}
