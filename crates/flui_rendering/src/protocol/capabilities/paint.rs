//! Paint capability trait for the composition-based protocol system.
//!
//! This module defines PaintCapability which composes the 4 foundation painting traits:
//! - Painter: Core drawing operations (shapes, images, text)
//! - Layering: Layer composition (push/pop layers, transforms, clips)
//! - Effects: Visual effects (blur, filters)
//! - Caching: Paint optimization (repaint boundaries, cache hints)
//!
//! PaintCapability is protocol-agnostic and shared between BoxProtocol and SliverProtocol.

use flui_foundation::painting::{
    BlendMode, CacheHint, Caching, ClipBehavior, Effects, Layering, PaintColorFilter, PaintImage,
    PaintParagraph, PaintShader, Painter,
};
use flui_types::geometry::{Matrix4, Offset, Point, RRect, Rect};
use flui_types::painting::{ImageFilter, Paint, Path};

use crate::arity::Arity;
use crate::parent_data::ParentData;

// ============================================================================
// PAINT CAPABILITY TRAIT
// ============================================================================

/// Capability trait for paint operations.
///
/// Unlike Layout and HitTest which are protocol-specific, Paint is shared
/// across all protocols (Box and Sliver use the same painting system).
///
/// The capability composes 4 orthogonal concerns:
/// - **Painter**: What to draw (shapes, images, text)
/// - **Layering**: How layers are composed
/// - **Effects**: Visual effects applied
/// - **Caching**: Optimization strategies
pub trait PaintCapability: Send + Sync + 'static {
    /// The canvas/painter implementation for drawing primitives.
    type Painter: Painter;

    /// The layer composition strategy.
    type Layering: Layering;

    /// Visual effects implementation.
    type Effects: Effects;

    /// Caching strategy for paint optimization.
    type Caching: Caching;

    /// The paint context type, parameterized by lifetime, arity and parent data.
    type Context<'ctx, A: Arity, P: ParentData>: PaintContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}

// ============================================================================
// PAINT CONTEXT API TRAIT
// ============================================================================

/// API that paint contexts must provide.
///
/// This is a minimal interface - the actual PaintContext implementation
/// in flui_rendering/context/paint.rs provides a much richer API with:
/// - Scoped operations (with_save, with_translate, with_opacity)
/// - Chaining API (fluent builder pattern)
/// - Conditional drawing (when, when_else)
/// - Child painting helpers
pub trait PaintContextApi<'ctx, P: PaintCapability + ?Sized, A: Arity, PD: ParentData>:
    Send + Sync
{
    /// Get access to the underlying painter for direct drawing.
    fn painter(&mut self) -> &mut P::Painter;

    /// Get access to layering operations.
    fn layering(&mut self) -> &mut P::Layering;

    /// Get access to effects operations.
    fn effects(&mut self) -> &mut P::Effects;

    /// Get access to caching information.
    fn caching(&self) -> &P::Caching;

    /// Check if this render object is a repaint boundary.
    fn is_repaint_boundary(&self) -> bool;

    /// Get the current paint offset (accumulated from parent transforms).
    fn offset(&self) -> Offset;
}

// ============================================================================
// STANDARD PAINT IMPLEMENTATION
// ============================================================================

/// Standard paint configuration used by most protocols.
///
/// This uses Skia-based Canvas for painting, SceneBuilder for layering,
/// standard effects, and repaint boundary caching.
pub struct StandardPaint;

impl PaintCapability for StandardPaint {
    type Painter = DynPainter;
    type Layering = DynLayering;
    type Effects = DynEffects;
    type Caching = DynCaching;
    type Context<'ctx, A: Arity, P: ParentData>
        = StandardPaintCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

// ============================================================================
// DYNAMIC WRAPPER TYPES
// ============================================================================

/// Dynamic wrapper for Painter trait object.
pub struct DynPainter {
    inner: Box<dyn Painter>,
}

impl DynPainter {
    pub fn new(painter: impl Painter + 'static) -> Self {
        Self {
            inner: Box::new(painter),
        }
    }
}

impl Painter for DynPainter {
    fn save(&mut self) {
        self.inner.save()
    }

    fn restore(&mut self) {
        self.inner.restore()
    }

    fn save_count(&self) -> usize {
        self.inner.save_count()
    }

    fn restore_to_count(&mut self, count: usize) {
        self.inner.restore_to_count(count)
    }

    fn translate(&mut self, dx: f32, dy: f32) {
        self.inner.translate(dx, dy)
    }

    fn rotate(&mut self, radians: f32) {
        self.inner.rotate(radians)
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        self.inner.scale(sx, sy)
    }

    fn skew(&mut self, sx: f32, sy: f32) {
        self.inner.skew(sx, sy)
    }

    fn transform(&mut self, matrix: &Matrix4) {
        self.inner.transform(matrix)
    }

    fn reset_transform(&mut self) {
        self.inner.reset_transform()
    }

    fn get_transform(&self) -> Matrix4 {
        self.inner.get_transform()
    }

    fn clip_rect(&mut self, rect: Rect) {
        self.inner.clip_rect(rect)
    }

    fn clip_rrect(&mut self, rrect: RRect) {
        self.inner.clip_rrect(rrect)
    }

    fn clip_path(&mut self, path: &Path) {
        self.inner.clip_path(path)
    }

    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.inner.draw_rect(rect, paint)
    }

    fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.inner.draw_rrect(rrect, paint)
    }

    fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.inner.draw_circle(center, radius, paint)
    }

    fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        self.inner.draw_oval(rect, paint)
    }

    fn draw_line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.inner.draw_line(p1, p2, paint)
    }

    fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.inner.draw_path(path, paint)
    }

    fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        self.inner
            .draw_arc(rect, start_angle, sweep_angle, use_center, paint)
    }

    fn draw_points(&mut self, points: &[Point], paint: &Paint) {
        self.inner.draw_points(points, paint)
    }

    fn draw_image(&mut self, image: &dyn PaintImage, offset: Offset, paint: &Paint) {
        self.inner.draw_image(image, offset, paint)
    }

    fn draw_image_rect(&mut self, image: &dyn PaintImage, src: Rect, dst: Rect, paint: &Paint) {
        self.inner.draw_image_rect(image, src, dst, paint)
    }

    fn draw_image_nine(&mut self, image: &dyn PaintImage, center: Rect, dst: Rect, paint: &Paint) {
        self.inner.draw_image_nine(image, center, dst, paint)
    }

    fn draw_paragraph(&mut self, paragraph: &dyn PaintParagraph, offset: Offset) {
        self.inner.draw_paragraph(paragraph, offset)
    }

    fn save_layer(&mut self, bounds: Option<Rect>, paint: Option<&Paint>) {
        self.inner.save_layer(bounds, paint)
    }

    fn save_layer_alpha(&mut self, bounds: Option<Rect>, alpha: u8) {
        self.inner.save_layer_alpha(bounds, alpha)
    }

    fn clear(&mut self, color: u32) {
        self.inner.clear(color)
    }

    fn draw_shadow(&mut self, path: &Path, color: u32, elevation: f32, transparent_occluder: bool) {
        self.inner
            .draw_shadow(path, color, elevation, transparent_occluder)
    }
}

/// Dynamic wrapper for Layering trait object.
pub struct DynLayering {
    inner: Box<dyn Layering>,
}

impl DynLayering {
    pub fn new(layering: impl Layering + 'static) -> Self {
        Self {
            inner: Box::new(layering),
        }
    }
}

impl Layering for DynLayering {
    fn push_layer(&mut self, bounds: Rect, paint: Option<&Paint>) {
        self.inner.push_layer(bounds, paint)
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer()
    }

    fn push_clip_rect(&mut self, rect: Rect, clip_behavior: ClipBehavior) {
        self.inner.push_clip_rect(rect, clip_behavior)
    }

    fn push_clip_rrect(&mut self, rrect: RRect, clip_behavior: ClipBehavior) {
        self.inner.push_clip_rrect(rrect, clip_behavior)
    }

    fn push_clip_path(&mut self, path: &Path, clip_behavior: ClipBehavior) {
        self.inner.push_clip_path(path, clip_behavior)
    }

    fn push_transform(&mut self, matrix: Matrix4) {
        self.inner.push_transform(matrix)
    }

    fn push_opacity(&mut self, opacity: f32, bounds: Option<Rect>) {
        self.inner.push_opacity(opacity, bounds)
    }

    fn push_backdrop_filter(&mut self, filter: &ImageFilter, bounds: Rect) {
        self.inner.push_backdrop_filter(filter, bounds)
    }

    fn push_shader_mask(&mut self, shader: &dyn PaintShader, bounds: Rect, blend_mode: BlendMode) {
        self.inner.push_shader_mask(shader, bounds, blend_mode)
    }

    fn pop(&mut self) {
        self.inner.pop()
    }

    fn depth(&self) -> usize {
        self.inner.depth()
    }
}

/// Dynamic wrapper for Effects trait object.
pub struct DynEffects {
    inner: Box<dyn Effects>,
}

impl DynEffects {
    pub fn new(effects: impl Effects + 'static) -> Self {
        Self {
            inner: Box::new(effects),
        }
    }
}

impl Effects for DynEffects {
    fn apply_blur(&mut self, sigma_x: f32, sigma_y: f32, bounds: Rect) {
        self.inner.apply_blur(sigma_x, sigma_y, bounds)
    }

    fn apply_color_filter(&mut self, filter: &dyn PaintColorFilter, bounds: Rect) {
        self.inner.apply_color_filter(filter, bounds)
    }

    fn apply_backdrop_filter(&mut self, filter: &ImageFilter, bounds: Rect) {
        self.inner.apply_backdrop_filter(filter, bounds)
    }

    fn apply_shader(&mut self, shader: &dyn PaintShader, bounds: Rect) {
        self.inner.apply_shader(shader, bounds)
    }

    fn apply_drop_shadow(&mut self, offset: Offset, blur_radius: f32, color: u32, bounds: Rect) {
        self.inner
            .apply_drop_shadow(offset, blur_radius, color, bounds)
    }

    fn apply_inner_shadow(&mut self, offset: Offset, blur_radius: f32, color: u32, bounds: Rect) {
        self.inner
            .apply_inner_shadow(offset, blur_radius, color, bounds)
    }
}

/// Dynamic wrapper for Caching trait.
pub struct DynCaching {
    is_repaint_boundary: bool,
    cache_hint: CacheHint,
    needs_repaint: bool,
    cache_valid: bool,
    cache_key: Option<u64>,
}

impl DynCaching {
    pub fn new(is_repaint_boundary: bool) -> Self {
        Self {
            is_repaint_boundary,
            cache_hint: CacheHint::None,
            needs_repaint: true,
            cache_valid: false,
            cache_key: None,
        }
    }

    pub fn with_cache_key(mut self, key: u64) -> Self {
        self.cache_key = Some(key);
        self
    }
}

impl Caching for DynCaching {
    fn should_cache(&self) -> bool {
        self.is_repaint_boundary
    }

    fn cache_hint(&self) -> CacheHint {
        self.cache_hint
    }

    fn invalidate(&mut self) {
        self.cache_valid = false;
    }

    fn mark_needs_repaint(&mut self) {
        self.needs_repaint = true;
    }

    fn is_repaint_boundary(&self) -> bool {
        self.is_repaint_boundary
    }

    fn set_repaint_boundary(&mut self, is_boundary: bool) {
        self.is_repaint_boundary = is_boundary;
    }

    fn cache_key(&self) -> Option<u64> {
        self.cache_key
    }

    fn is_cache_valid(&self) -> bool {
        self.cache_valid
    }
}

// ============================================================================
// STANDARD PAINT CONTEXT
// ============================================================================

/// Standard paint context implementation.
///
/// This provides the basic PaintContextApi implementation. The rich API
/// with scoped operations, chaining, etc. will be in context/paint.rs
pub struct StandardPaintCtx<'ctx, A: Arity, P: ParentData> {
    painter: &'ctx mut DynPainter,
    layering: &'ctx mut DynLayering,
    effects: &'ctx mut DynEffects,
    caching: &'ctx DynCaching,
    offset: Offset,
    _arity: std::marker::PhantomData<A>,
    _parent_data: std::marker::PhantomData<P>,
}

impl<'ctx, A: Arity, P: ParentData> StandardPaintCtx<'ctx, A, P> {
    /// Create a new standard paint context.
    pub fn new(
        painter: &'ctx mut DynPainter,
        layering: &'ctx mut DynLayering,
        effects: &'ctx mut DynEffects,
        caching: &'ctx DynCaching,
        offset: Offset,
    ) -> Self {
        Self {
            painter,
            layering,
            effects,
            caching,
            offset,
            _arity: std::marker::PhantomData,
            _parent_data: std::marker::PhantomData,
        }
    }
}

impl<'ctx, A: Arity, P: ParentData> PaintContextApi<'ctx, StandardPaint, A, P>
    for StandardPaintCtx<'ctx, A, P>
{
    fn painter(&mut self) -> &mut DynPainter {
        self.painter
    }

    fn layering(&mut self) -> &mut DynLayering {
        self.layering
    }

    fn effects(&mut self) -> &mut DynEffects {
        self.effects
    }

    fn caching(&self) -> &DynCaching {
        self.caching
    }

    fn is_repaint_boundary(&self) -> bool {
        self.caching.is_repaint_boundary()
    }

    fn offset(&self) -> Offset {
        self.offset
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_paint_types() {
        // Verify StandardPaint implements PaintCapability
        fn assert_paint_capability<P: PaintCapability>() {}
        assert_paint_capability::<StandardPaint>();
    }

    #[test]
    fn test_dyn_caching() {
        let caching = DynCaching::new(true);
        assert!(caching.is_repaint_boundary());
        assert!(caching.should_cache());

        let caching = DynCaching::new(false);
        assert!(!caching.is_repaint_boundary());
        assert!(!caching.should_cache());
    }

    #[test]
    fn test_dyn_caching_with_key() {
        let caching = DynCaching::new(true).with_cache_key(12345);
        assert_eq!(caching.cache_key(), Some(12345));
    }
}
