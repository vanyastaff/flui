//! Effect layers for visual transformations.

use std::any::Any;

use flui_types::{Offset, Point, Rect};

use super::base::{EngineLayer, Layer, LayerId, SceneBuilder};
use super::container::ContainerLayer;

// ============================================================================
// OpacityLayer
// ============================================================================

/// A layer that applies opacity to its children.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `OpacityLayer` class.
#[derive(Debug)]
pub struct OpacityLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The opacity value (0-255).
    alpha: u8,

    /// The offset to apply.
    offset: Offset,
}

impl OpacityLayer {
    /// Creates a new opacity layer.
    pub fn new(alpha: u8, offset: Offset) -> Self {
        Self {
            container: ContainerLayer::new(),
            alpha,
            offset,
        }
    }

    /// Creates an opacity layer with a fraction (0.0 to 1.0).
    pub fn from_opacity(opacity: f32, offset: Offset) -> Self {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        Self::new(alpha, offset)
    }

    /// Returns the alpha value (0-255).
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// Sets the alpha value (0-255).
    pub fn set_alpha(&mut self, alpha: u8) {
        if self.alpha != alpha {
            self.alpha = alpha;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the opacity as a fraction (0.0 to 1.0).
    pub fn opacity(&self) -> f32 {
        self.alpha as f32 / 255.0
    }

    /// Sets the opacity as a fraction (0.0 to 1.0).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.set_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
    }

    /// Returns the offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the offset.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }
}

impl Layer for OpacityLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // Push opacity
        let effective_offset = layer_offset + self.offset;
        builder.push_opacity(self.alpha, effective_offset);

        // Add children
        self.container.add_to_scene(builder, effective_offset);

        // Pop
        builder.pop();
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Transform the offset and search children
        let local_offset = offset - self.offset;
        self.container.find(local_offset)
    }

    fn bounds(&self) -> Rect {
        let child_bounds = self.container.bounds();
        child_bounds.translate_offset(self.offset)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// ColorFilterLayer
// ============================================================================

/// Color filter type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorFilter {
    /// Blend mode color filter.
    Mode {
        /// The color to use.
        color: u32,
        /// The blend mode (as index).
        blend_mode: u8,
    },
    /// Matrix color filter (5x4 matrix in row-major order).
    Matrix {
        /// The color matrix values.
        matrix: [f32; 20],
    },
    /// Linear to sRGB gamma.
    LinearToSrgbGamma,
    /// sRGB to linear gamma.
    SrgbToLinearGamma,
}

/// A layer that applies a color filter to its children.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ColorFilterLayer` class.
#[derive(Debug)]
pub struct ColorFilterLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The color filter to apply.
    color_filter: ColorFilter,
}

impl ColorFilterLayer {
    /// Creates a new color filter layer.
    pub fn new(color_filter: ColorFilter) -> Self {
        Self {
            container: ContainerLayer::new(),
            color_filter,
        }
    }

    /// Returns the color filter.
    pub fn color_filter(&self) -> &ColorFilter {
        &self.color_filter
    }

    /// Sets the color filter.
    pub fn set_color_filter(&mut self, filter: ColorFilter) {
        self.color_filter = filter;
        self.container.mark_needs_add_to_scene();
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }
}

impl Layer for ColorFilterLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // TODO: Add color filter operation when supported
        // For now, just add children
        self.container.add_to_scene(builder, layer_offset);
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        self.container.bounds()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// BackdropFilterLayer
// ============================================================================

/// Image filter type for backdrop effects.
#[derive(Debug, Clone)]
pub enum ImageFilter {
    /// Gaussian blur.
    Blur {
        /// Sigma X.
        sigma_x: f32,
        /// Sigma Y.
        sigma_y: f32,
    },
    /// Matrix convolution.
    Matrix {
        /// Filter matrix.
        kernel: Vec<f32>,
        /// Kernel width.
        kernel_width: u32,
        /// Kernel height.
        kernel_height: u32,
    },
    /// Dilate morphology.
    Dilate {
        /// Radius X.
        radius_x: f32,
        /// Radius Y.
        radius_y: f32,
    },
    /// Erode morphology.
    Erode {
        /// Radius X.
        radius_x: f32,
        /// Radius Y.
        radius_y: f32,
    },
}

impl Default for ImageFilter {
    fn default() -> Self {
        Self::Blur {
            sigma_x: 0.0,
            sigma_y: 0.0,
        }
    }
}

/// Blend mode for compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BlendMode {
    /// Default blend mode.
    #[default]
    SrcOver = 0,
    /// Source only.
    Src = 1,
    /// Destination only.
    Dst = 2,
    /// Source in destination.
    SrcIn = 3,
    /// Destination in source.
    DstIn = 4,
    /// Source out of destination.
    SrcOut = 5,
    /// Destination out of source.
    DstOut = 6,
    /// Source atop destination.
    SrcATop = 7,
    /// Destination atop source.
    DstATop = 8,
    /// XOR of source and destination.
    Xor = 9,
    /// Plus (saturating add).
    Plus = 10,
    /// Multiply.
    Multiply = 11,
    /// Screen.
    Screen = 12,
    /// Overlay.
    Overlay = 13,
    /// Darken.
    Darken = 14,
    /// Lighten.
    Lighten = 15,
}

/// A layer that applies a filter to the content beneath it.
///
/// This is used for effects like blur that affect what's behind the layer.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `BackdropFilterLayer` class.
#[derive(Debug)]
pub struct BackdropFilterLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The image filter to apply.
    filter: ImageFilter,

    /// The blend mode for compositing.
    blend_mode: BlendMode,
}

impl BackdropFilterLayer {
    /// Creates a new backdrop filter layer.
    pub fn new(filter: ImageFilter, blend_mode: BlendMode) -> Self {
        Self {
            container: ContainerLayer::new(),
            filter,
            blend_mode,
        }
    }

    /// Creates a blur backdrop filter.
    pub fn blur(sigma_x: f32, sigma_y: f32) -> Self {
        Self::new(ImageFilter::Blur { sigma_x, sigma_y }, BlendMode::SrcOver)
    }

    /// Returns the image filter.
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// Sets the image filter.
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
        self.container.mark_needs_add_to_scene();
    }

    /// Returns the blend mode.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Sets the blend mode.
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        if self.blend_mode != mode {
            self.blend_mode = mode;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }
}

impl Layer for BackdropFilterLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // TODO: Add backdrop filter operation when supported
        // For now, just add children
        self.container.add_to_scene(builder, layer_offset);
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        self.container.bounds()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// ShaderMaskLayer
// ============================================================================

/// Shader for masking.
#[derive(Debug, Clone)]
pub enum Shader {
    /// Solid color.
    Color(u32),
    /// Linear gradient.
    LinearGradient {
        /// Start point.
        from: (f32, f32),
        /// End point.
        to: (f32, f32),
        /// Colors (as u32 ARGB).
        colors: Vec<u32>,
        /// Color stops (0.0 to 1.0).
        stops: Vec<f32>,
    },
    /// Radial gradient.
    RadialGradient {
        /// Center point.
        center: (f32, f32),
        /// Radius.
        radius: f32,
        /// Colors (as u32 ARGB).
        colors: Vec<u32>,
        /// Color stops (0.0 to 1.0).
        stops: Vec<f32>,
    },
}

impl Default for Shader {
    fn default() -> Self {
        Self::Color(0xFFFFFFFF)
    }
}

/// A layer that masks its children with a shader.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ShaderMaskLayer` class.
#[derive(Debug)]
pub struct ShaderMaskLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The shader to use as mask.
    shader: Shader,

    /// The mask rect.
    mask_rect: Rect,

    /// The blend mode.
    blend_mode: BlendMode,
}

impl ShaderMaskLayer {
    /// Creates a new shader mask layer.
    pub fn new(shader: Shader, mask_rect: Rect, blend_mode: BlendMode) -> Self {
        Self {
            container: ContainerLayer::new(),
            shader,
            mask_rect,
            blend_mode,
        }
    }

    /// Returns the shader.
    pub fn shader(&self) -> &Shader {
        &self.shader
    }

    /// Sets the shader.
    pub fn set_shader(&mut self, shader: Shader) {
        self.shader = shader;
        self.container.mark_needs_add_to_scene();
    }

    /// Returns the mask rect.
    pub fn mask_rect(&self) -> Rect {
        self.mask_rect
    }

    /// Sets the mask rect.
    pub fn set_mask_rect(&mut self, rect: Rect) {
        if self.mask_rect != rect {
            self.mask_rect = rect;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the blend mode.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Sets the blend mode.
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        if self.blend_mode != mode {
            self.blend_mode = mode;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }
}

impl Layer for ShaderMaskLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // TODO: Add shader mask operation when supported
        // For now, just add children
        self.container.add_to_scene(builder, layer_offset);
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Only find if within mask rect
        let point = Point::new(offset.dx, offset.dy);
        if !self.mask_rect.contains(point) {
            return None;
        }
        self.container.find(offset)
    }

    fn bounds(&self) -> Rect {
        let child_bounds = self.container.bounds();
        child_bounds.intersect(self.mask_rect).unwrap_or(Rect::ZERO)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// TransformLayer
// ============================================================================

/// A layer that applies a transformation matrix to its children.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `TransformLayer` class.
#[derive(Debug)]
pub struct TransformLayer {
    /// The container layer data.
    container: ContainerLayer,

    /// The transformation matrix (4x4, column-major).
    transform: [f32; 16],

    /// The offset to apply before the transform.
    offset: Offset,
}

impl TransformLayer {
    /// Creates a new transform layer with the given transformation matrix.
    pub fn new(transform: [f32; 16]) -> Self {
        Self {
            container: ContainerLayer::new(),
            transform,
            offset: Offset::ZERO,
        }
    }

    /// Creates an identity transform layer.
    pub fn identity() -> Self {
        Self::new([
            1.0, 0.0, 0.0, 0.0, // col 0
            0.0, 1.0, 0.0, 0.0, // col 1
            0.0, 0.0, 1.0, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ])
    }

    /// Creates a scale transform layer.
    pub fn scale(sx: f32, sy: f32, sz: f32) -> Self {
        Self::new([
            sx, 0.0, 0.0, 0.0, // col 0
            0.0, sy, 0.0, 0.0, // col 1
            0.0, 0.0, sz, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ])
    }

    /// Creates a translation transform layer.
    pub fn translation(tx: f32, ty: f32, tz: f32) -> Self {
        Self::new([
            1.0, 0.0, 0.0, 0.0, // col 0
            0.0, 1.0, 0.0, 0.0, // col 1
            0.0, 0.0, 1.0, 0.0, // col 2
            tx, ty, tz, 1.0, // col 3
        ])
    }

    /// Returns the transformation matrix.
    pub fn transform(&self) -> &[f32; 16] {
        &self.transform
    }

    /// Sets the transformation matrix.
    pub fn set_transform(&mut self, transform: [f32; 16]) {
        if self.transform != transform {
            self.transform = transform;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the offset.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.container.mark_needs_add_to_scene();
        }
    }

    /// Returns the first child layer.
    pub fn first_child(&self) -> Option<&dyn Layer> {
        self.container.first_child()
    }

    /// Returns the last child layer.
    pub fn last_child(&self) -> Option<&dyn Layer> {
        self.container.last_child()
    }

    /// Appends a child layer.
    pub fn append(&mut self, child: Box<dyn Layer>) {
        self.container.append(child);
    }

    /// Removes all child layers.
    pub fn remove_all_children(&mut self) {
        self.container.remove_all_children();
    }

    /// Transforms a point using the inverse of the transform matrix.
    fn inverse_transform_point(&self, point: Offset) -> Option<Offset> {
        // Simple case: if close to identity, just return the point
        let is_identity = (self.transform[0] - 1.0).abs() < 1e-6
            && (self.transform[5] - 1.0).abs() < 1e-6
            && (self.transform[10] - 1.0).abs() < 1e-6
            && (self.transform[15] - 1.0).abs() < 1e-6
            && self.transform[1].abs() < 1e-6
            && self.transform[2].abs() < 1e-6
            && self.transform[4].abs() < 1e-6
            && self.transform[6].abs() < 1e-6;

        if is_identity {
            return Some(Offset::new(
                point.dx - self.transform[12],
                point.dy - self.transform[13],
            ));
        }

        // For general case, compute inverse (2D affine subset)
        let a = self.transform[0];
        let b = self.transform[1];
        let c = self.transform[4];
        let d = self.transform[5];
        let tx = self.transform[12];
        let ty = self.transform[13];

        let det = a * d - b * c;
        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;
        let local_x = (d * (point.dx - tx) - c * (point.dy - ty)) * inv_det;
        let local_y = (-b * (point.dx - tx) + a * (point.dy - ty)) * inv_det;

        Some(Offset::new(local_x, local_y))
    }
}

impl Layer for TransformLayer {
    fn id(&self) -> LayerId {
        self.container.id()
    }

    fn engine_layer(&self) -> Option<&EngineLayer> {
        self.container.engine_layer()
    }

    fn set_engine_layer(&mut self, layer: Option<EngineLayer>) {
        self.container.set_engine_layer(layer);
    }

    fn parent(&self) -> Option<&dyn Layer> {
        self.container.parent()
    }

    fn remove(&mut self) {
        self.container.remove();
    }

    fn needs_add_to_scene(&self) -> bool {
        self.container.needs_add_to_scene()
    }

    fn mark_needs_add_to_scene(&mut self) {
        self.container.mark_needs_add_to_scene();
    }

    fn update_subtree_needs_add_to_scene(&mut self) {
        self.container.update_subtree_needs_add_to_scene();
    }

    fn add_to_scene(&mut self, builder: &mut SceneBuilder, layer_offset: Offset) {
        // Apply offset first, then transform
        let mut matrix = self.transform;
        matrix[12] += layer_offset.dx + self.offset.dx;
        matrix[13] += layer_offset.dy + self.offset.dy;

        builder.push_transform(matrix);

        // Add children at zero offset since transform includes it
        self.container.add_children_to_scene(builder, Offset::ZERO);

        builder.pop();

        // Mark as added to scene - the container tracks this internally
    }

    fn find(&self, offset: Offset) -> Option<&dyn Layer> {
        // Transform the point using inverse transform
        let local_offset = self.inverse_transform_point(offset - self.offset)?;
        self.container.find(local_offset)
    }

    fn bounds(&self) -> Rect {
        // Transform child bounds (simplified: just return child bounds for now)
        // A full implementation would transform all 4 corners
        let child_bounds = self.container.bounds();
        child_bounds.translate_offset(self.offset)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_layer_new() {
        let layer = OpacityLayer::new(128, Offset::ZERO);
        assert_eq!(layer.alpha(), 128);
        assert!((layer.opacity() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_opacity_layer_from_opacity() {
        let layer = OpacityLayer::from_opacity(0.5, Offset::ZERO);
        assert_eq!(layer.alpha(), 127);
    }

    #[test]
    fn test_opacity_layer_set_values() {
        let mut layer = OpacityLayer::new(255, Offset::ZERO);

        layer.set_alpha(100);
        assert_eq!(layer.alpha(), 100);

        layer.set_opacity(1.0);
        assert_eq!(layer.alpha(), 255);

        layer.set_offset(Offset::new(10.0, 20.0));
        assert_eq!(layer.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_color_filter_layer() {
        let filter = ColorFilter::LinearToSrgbGamma;
        let layer = ColorFilterLayer::new(filter);
        assert!(matches!(
            layer.color_filter(),
            ColorFilter::LinearToSrgbGamma
        ));
    }

    #[test]
    fn test_backdrop_filter_layer_blur() {
        let layer = BackdropFilterLayer::blur(10.0, 10.0);
        assert!(matches!(
            layer.filter(),
            ImageFilter::Blur {
                sigma_x: 10.0,
                sigma_y: 10.0
            }
        ));
        assert_eq!(layer.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_shader_mask_layer() {
        let shader = Shader::Color(0xFF0000FF);
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let layer = ShaderMaskLayer::new(shader, rect, BlendMode::SrcIn);
        assert_eq!(layer.mask_rect(), rect);
        assert_eq!(layer.blend_mode(), BlendMode::SrcIn);
    }

    #[test]
    fn test_transform_layer_identity() {
        let layer = TransformLayer::identity();
        let transform = layer.transform();
        assert!((transform[0] - 1.0).abs() < 1e-6);
        assert!((transform[5] - 1.0).abs() < 1e-6);
        assert!((transform[10] - 1.0).abs() < 1e-6);
        assert!((transform[15] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_layer_scale() {
        let layer = TransformLayer::scale(2.0, 3.0, 1.0);
        let transform = layer.transform();
        assert!((transform[0] - 2.0).abs() < 1e-6);
        assert!((transform[5] - 3.0).abs() < 1e-6);
        assert!((transform[10] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_layer_translation() {
        let layer = TransformLayer::translation(10.0, 20.0, 0.0);
        let transform = layer.transform();
        assert!((transform[12] - 10.0).abs() < 1e-6);
        assert!((transform[13] - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_layer_inverse_identity() {
        let layer = TransformLayer::identity();
        let point = Offset::new(100.0, 200.0);
        let result = layer.inverse_transform_point(point);
        assert!(result.is_some());
        let local = result.unwrap();
        assert!((local.dx - 100.0).abs() < 1e-6);
        assert!((local.dy - 200.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_layer_inverse_translation() {
        let layer = TransformLayer::translation(10.0, 20.0, 0.0);
        let point = Offset::new(110.0, 220.0);
        let result = layer.inverse_transform_point(point);
        assert!(result.is_some());
        let local = result.unwrap();
        assert!((local.dx - 100.0).abs() < 1e-6);
        assert!((local.dy - 200.0).abs() < 1e-6);
    }
}
