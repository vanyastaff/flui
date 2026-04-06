//! Draw command dispatch from render commands to GPU pipelines.
//!
//! This module routes [`DrawCommand`] variants to the appropriate batcher
//! and traverses the [`Scene`]'s layer tree, dispatching all commands
//! encountered during traversal.
//!
//! This is CPU-side logic only -- it operates on references to batchers
//! and state stacks, with no GPU code.

use flui_foundation::LayerId;
use flui_layer::{Layer, Scene};
use flui_layer::tree::LayerTree;
use flui_painting::display_list::{DrawCommand, DisplayListCore};
use flui_types::geometry::{Matrix4, Pixels, RRect, Rect};
use flui_types::painting::Paint;
use flui_types::styling::Color;

use crate::batchers::compositing::{CompositingBatcher, FilterType};
use crate::batchers::effects::{EffectBatcher, ShadowInstance};
use crate::batchers::images::ImageBatcher;
use crate::batchers::paths::PathBatcher;
use crate::batchers::shapes::ShapeBatcher;
use crate::batchers::text::TextBatcher;
use crate::frame::state_stack::{ClipRect, StateStack};
use crate::text::TextCacheKey;

// ===========================================================================
// Batchers collection
// ===========================================================================

/// Collection of all batchers used during frame dispatch.
///
/// Each batcher accumulates instances for a specific primitive type.
/// After traversal, the batchers are consumed by the GPU submission step.
pub struct Batchers {
    /// Rectangle, circle, oval, arc, and line batching.
    pub shapes: ShapeBatcher,
    /// Tessellated vector path batching.
    pub paths: PathBatcher,
    /// Glyph/text run batching.
    pub text: TextBatcher,
    /// Image quad batching grouped by texture ID.
    pub images: ImageBatcher,
    /// Gradient, shadow, and blur effect batching.
    pub effects: EffectBatcher,
    /// Offscreen target and filter operation batching.
    pub compositing: CompositingBatcher,
}

impl Batchers {
    /// Create a new collection with pre-allocated capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shapes: ShapeBatcher::new(),
            paths: PathBatcher::new(),
            text: TextBatcher::new(),
            images: ImageBatcher::new(),
            effects: EffectBatcher::new(),
            compositing: CompositingBatcher::new(),
        }
    }

    /// Clear all batchers, keeping allocated memory for reuse.
    pub fn clear_all(&mut self) {
        self.shapes.clear();
        self.paths.clear();
        self.text.clear();
        self.images.clear();
        self.effects.clear();
        self.compositing.clear();
    }

    /// Returns `true` if every batcher is empty.
    #[must_use]
    pub fn is_all_empty(&self) -> bool {
        self.shapes.is_empty()
            && self.paths.is_empty()
            && self.text.is_empty()
            && self.images.is_empty()
            && self.effects.is_empty()
            && self.compositing.is_empty()
    }
}

impl Default for Batchers {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Helper functions
// ===========================================================================

/// Convert a [`Paint`]'s color to an `[f32; 4]` RGBA array.
fn paint_to_color(paint: &Paint) -> [f32; 4] {
    color_to_array(&paint.color)
}

/// Convert a [`Color`] (u8 channels) to an `[f32; 4]` RGBA array.
fn color_to_array(color: &Color) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    ]
}

/// Extract a 2D affine transform from a [`Matrix4`] as `[scale_x, skew_x, skew_y, scale_y]`.
///
/// Currently returns identity -- full extraction will be added when
/// the rendering pipeline consumes non-identity transforms.
fn extract_transform_2d(_m: &Matrix4) -> [f32; 4] {
    // Matrix4 is column-major [col0..col3] = 16 floats.
    // 2D affine: scale_x = m[0], skew_x = m[4], skew_y = m[1], scale_y = m[5]
    // Placeholder identity for now.
    [1.0, 0.0, 0.0, 1.0]
}

/// Extract per-corner radii from an [`RRect`] as `[tl, tr, br, bl]`.
///
/// Uses the *x* component of each corner radius (assumes circular corners
/// for the GPU shader).
fn rrect_corner_radii(rrect: &RRect) -> [f32; 4] {
    [
        rrect.top_left.x.get(),
        rrect.top_right.x.get(),
        rrect.bottom_right.x.get(),
        rrect.bottom_left.x.get(),
    ]
}

/// Helper to extract `[x, y, w, h]` from a `Rect<Pixels>`.
fn rect_to_xywh(r: &Rect<Pixels>) -> [f32; 4] {
    [
        r.left().get(),
        r.top().get(),
        r.width().get(),
        r.height().get(),
    ]
}

// ===========================================================================
// Command dispatch
// ===========================================================================

/// Route a single [`DrawCommand`] to the appropriate batcher.
///
/// This is an exhaustive match -- every variant is handled. Variants that
/// require tessellation or shader decomposition log a debug/warn message
/// and are skipped for now.
#[allow(unused_variables)] // transform / path params not yet consumed
pub fn dispatch_command(
    cmd: &DrawCommand,
    batchers: &mut Batchers,
    state: &mut StateStack,
    _scale_factor: f32,
) {
    match cmd {
        // =================================================================
        // Primitives -> ShapeBatcher
        // =================================================================
        DrawCommand::DrawRect {
            rect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers.shapes.add_rect(
                rect.left().get(),
                rect.top().get(),
                rect.width().get(),
                rect.height().get(),
                color,
                [0.0; 4],
                t,
            );
        }

        DrawCommand::DrawRRect {
            rrect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            let radii = rrect_corner_radii(rrect);
            batchers.shapes.add_rect(
                rrect.rect.left().get(),
                rrect.rect.top().get(),
                rrect.rect.width().get(),
                rrect.rect.height().get(),
                color,
                radii,
                t,
            );
        }

        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers
                .shapes
                .add_circle(center.x.get(), center.y.get(), radius.get(), color, t);
        }

        DrawCommand::DrawOval {
            rect,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let t = extract_transform_2d(transform);
            batchers.shapes.add_oval(
                rect.left().get(),
                rect.top().get(),
                rect.width().get(),
                rect.height().get(),
                color,
                t,
            );
        }

        DrawCommand::DrawLine {
            p1,
            p2,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let width = paint.stroke_width.max(1.0);
            batchers.shapes.add_line(
                p1.x.get(),
                p1.y.get(),
                p2.x.get(),
                p2.y.get(),
                color,
                width,
            );
        }

        DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center: _,
            paint,
            transform,
        } => {
            let color = paint_to_color(paint);
            let cx = rect.left().get() + rect.width().get() / 2.0;
            let cy = rect.top().get() + rect.height().get() / 2.0;
            let r = rect.width().get().min(rect.height().get()) / 2.0;
            batchers.shapes.add_arc(cx, cy, r, *start_angle, *sweep_angle, color);
        }

        // =================================================================
        // Path primitives -> PathBatcher (pending tessellation)
        // =================================================================
        DrawCommand::DrawPath {
            path,
            paint,
            transform,
        } => {
            tracing::debug!("DrawPath: path tessellation pending");
        }

        DrawCommand::DrawDRRect {
            outer,
            inner,
            paint,
            transform,
        } => {
            tracing::debug!("DrawDRRect: tessellation pending");
        }

        DrawCommand::DrawPoints {
            mode,
            points,
            paint,
            transform,
        } => {
            tracing::debug!("DrawPoints: tessellation pending");
        }

        DrawCommand::DrawVertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
            transform,
        } => {
            tracing::debug!("DrawVertices: direct upload pending");
        }

        // =================================================================
        // Text -> TextBatcher
        // =================================================================
        DrawCommand::DrawText {
            text,
            offset,
            size: _,
            style,
            paint,
            transform,
        } => {
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let font_weight = style
                .font_weight
                .map(|w| w.value())
                .unwrap_or(400);
            let key = TextCacheKey::new(text, font_size, "", font_weight);
            let color = paint_to_color(paint);
            batchers
                .text
                .add_run(key, [offset.dx.get(), offset.dy.get()], color, None);
        }

        DrawCommand::DrawTextSpan {
            span,
            offset,
            text_scale_factor,
            transform,
        } => {
            tracing::warn!("DrawTextSpan: rich text not yet implemented");
        }

        // =================================================================
        // Images -> ImageBatcher
        // =================================================================
        DrawCommand::DrawImage {
            image,
            dst,
            paint,
            transform,
        } => {
            let color = paint
                .as_ref()
                .map(paint_to_color)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            batchers.images.add_image(
                0, // texture_id placeholder
                rect_to_xywh(dst),
                [0.0, 0.0, 1.0, 1.0],
                color,
                extract_transform_2d(transform),
            );
        }

        DrawCommand::DrawTexture {
            texture_id,
            dst,
            src,
            filter_quality: _,
            opacity,
            transform,
        } => {
            let src_uv = src
                .as_ref()
                .map(|s| {
                    [
                        s.left().get(),
                        s.top().get(),
                        s.right().get(),
                        s.bottom().get(),
                    ]
                })
                .unwrap_or([0.0, 0.0, 1.0, 1.0]);
            batchers.images.add_image(
                texture_id.get(),
                rect_to_xywh(dst),
                src_uv,
                [1.0, 1.0, 1.0, *opacity],
                extract_transform_2d(transform),
            );
        }

        DrawCommand::DrawAtlas {
            image,
            sprites,
            transforms: _,
            colors,
            blend_mode,
            paint,
            transform,
        } => {
            tracing::debug!("DrawAtlas: sprite batch pending");
        }

        DrawCommand::DrawImageRepeat {
            image,
            dst,
            repeat,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageRepeat: tiled texture pending");
        }

        DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageNineSlice: 9-slice pending");
        }

        DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint,
            transform,
        } => {
            tracing::debug!("DrawImageFiltered: filtered image pending");
        }

        // =================================================================
        // Effects -> EffectBatcher
        // =================================================================
        DrawCommand::DrawShadow {
            path,
            color,
            elevation,
            transform,
        } => {
            batchers.effects.add_shadow(ShadowInstance {
                bounds: [0.0, 0.0, 100.0, 100.0], // placeholder bounds from path
                color: color_to_array(color),
                offset: [0.0, *elevation],
                blur_radius: *elevation * 2.0,
                spread: 0.0,
            });
        }

        DrawCommand::DrawGradient {
            rect,
            shader,
            transform,
        } => {
            tracing::debug!("DrawGradient: gradient dispatch pending shader decomposition");
        }

        DrawCommand::DrawGradientRRect {
            rrect,
            shader,
            transform,
        } => {
            tracing::debug!("DrawGradientRRect: gradient dispatch pending");
        }

        DrawCommand::DrawColor {
            color,
            blend_mode: _,
            transform,
        } => {
            let c = color_to_array(color);
            // Full-viewport rect with color.
            batchers
                .shapes
                .add_rect(0.0, 0.0, f32::MAX, f32::MAX, c, [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        }

        // =================================================================
        // Compositing -> CompositingBatcher / StateStack
        // =================================================================
        DrawCommand::ShaderMask {
            child: _,
            shader: _,
            bounds,
            blend_mode: _,
            transform,
        } => {
            batchers
                .compositing
                .add_shader_mask(rect_to_xywh(bounds), 0);
        }

        DrawCommand::BackdropFilter {
            child: _,
            filter: _,
            bounds,
            blend_mode: _,
            transform,
        } => {
            batchers.compositing.add_backdrop_filter(
                rect_to_xywh(bounds),
                FilterType::Blur {
                    sigma_x: 10.0,
                    sigma_y: 10.0,
                }, // placeholder
            );
        }

        DrawCommand::SaveLayer {
            bounds,
            paint: _,
            transform,
        } => {
            let b = bounds
                .as_ref()
                .map(rect_to_xywh)
                .unwrap_or([0.0, 0.0, f32::MAX, f32::MAX]);
            batchers.compositing.push_target(b, 1.0, 0);
        }

        DrawCommand::RestoreLayer { transform } => {
            batchers.compositing.pop_target();
        }

        // =================================================================
        // Clipping -> StateStack
        // =================================================================
        DrawCommand::ClipRect { rect, transform } => {
            state.clip.push_rect(ClipRect {
                x: rect.left().get(),
                y: rect.top().get(),
                width: rect.width().get(),
                height: rect.height().get(),
            });
        }

        DrawCommand::ClipRRect { rrect, transform } => {
            state.clip.push_rect(ClipRect {
                x: rrect.rect.left().get(),
                y: rrect.rect.top().get(),
                width: rrect.rect.width().get(),
                height: rrect.rect.height().get(),
            });
        }

        DrawCommand::ClipPath { path, transform } => {
            tracing::debug!("ClipPath: stencil-based clipping pending");
        }
    }
}

// ===========================================================================
// Layer traversal
// ===========================================================================

/// Traverse a [`Scene`]'s layer tree and dispatch all draw commands.
pub fn traverse_scene(
    scene: &Scene,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
) {
    if let Some(root_id) = scene.root() {
        traverse_layer(scene.layer_tree(), root_id, batchers, state, scale_factor);
    }
}

/// Recursively traverse a single layer and its children.
fn traverse_layer(
    tree: &LayerTree,
    layer_id: LayerId,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
) {
    let Some(node) = tree.get(layer_id) else {
        return;
    };
    let layer = node.layer();

    match layer {
        // =====================================================================
        // Leaf layers -- dispatch display list commands
        // =====================================================================
        Layer::Canvas(canvas) => {
            for cmd in canvas.display_list().commands() {
                dispatch_command(cmd, batchers, state, scale_factor);
            }
        }

        Layer::Picture(picture) => {
            for cmd in picture.picture().commands() {
                dispatch_command(cmd, batchers, state, scale_factor);
            }
        }

        Layer::Texture(_) => {
            tracing::debug!("TextureLayer rendering pending");
        }

        Layer::PlatformView(_) => {
            // Platform views are rendered by the platform; engine skips.
        }

        Layer::PerformanceOverlay(_) => {
            tracing::debug!("PerformanceOverlay rendering pending");
        }

        // =====================================================================
        // Clip layers
        // =====================================================================
        Layer::ClipRect(clip) => {
            let r = clip.clip_rect();
            state.clip.push_rect(ClipRect {
                x: r.left().get(),
                y: r.top().get(),
                width: r.width().get(),
                height: r.height().get(),
            });
            traverse_children(tree, layer_id, batchers, state, scale_factor);
            state.clip.pop();
        }

        Layer::ClipRRect(clip) => {
            let r = &clip.clip_rrect().rect;
            state.clip.push_rect(ClipRect {
                x: r.left().get(),
                y: r.top().get(),
                width: r.width().get(),
                height: r.height().get(),
            });
            traverse_children(tree, layer_id, batchers, state, scale_factor);
            state.clip.pop();
        }

        Layer::ClipPath(_) => {
            tracing::debug!("ClipPath layer: stencil clipping pending");
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        Layer::ClipSuperellipse(_) => {
            tracing::debug!("ClipSuperellipse layer: falling back to no clip");
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        // =====================================================================
        // Transform layers
        // =====================================================================
        Layer::Offset(offset_layer) => {
            let dx = offset_layer.offset().dx.get();
            let dy = offset_layer.offset().dy.get();
            state
                .transform
                .push(glam::Mat4::from_translation(glam::Vec3::new(dx, dy, 0.0)));
            traverse_children(tree, layer_id, batchers, state, scale_factor);
            state.transform.pop();
        }

        Layer::Transform(_transform_layer) => {
            // Full Matrix4 -> glam::Mat4 conversion to be added.
            state.transform.push(glam::Mat4::IDENTITY);
            traverse_children(tree, layer_id, batchers, state, scale_factor);
            state.transform.pop();
        }

        // =====================================================================
        // Effect layers
        // =====================================================================
        Layer::Opacity(opacity_layer) => {
            state.opacity.push(opacity_layer.alpha());
            traverse_children(tree, layer_id, batchers, state, scale_factor);
            state.opacity.pop();
        }

        Layer::ColorFilter(_) => {
            batchers.compositing.add_color_filter([0.0; 4]);
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        Layer::ImageFilter(_) => {
            batchers.compositing.add_image_filter(
                [0.0; 4],
                FilterType::Blur {
                    sigma_x: 0.0,
                    sigma_y: 0.0,
                },
            );
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        Layer::ShaderMask(_) => {
            batchers.compositing.add_shader_mask([0.0; 4], 0);
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        Layer::BackdropFilter(_) => {
            batchers.compositing.add_backdrop_filter(
                [0.0; 4],
                FilterType::Blur {
                    sigma_x: 10.0,
                    sigma_y: 10.0,
                },
            );
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        // =====================================================================
        // Linking layers
        // =====================================================================
        Layer::Leader(_) => {
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        Layer::Follower(_) => {
            traverse_children(tree, layer_id, batchers, state, scale_factor);
        }

        // =====================================================================
        // Annotation layers (no visual effect)
        // =====================================================================
        Layer::AnnotatedRegion(_) => {
            // Skip -- metadata only.
        }
    }
}

/// Traverse all children of `parent_id` in order.
fn traverse_children(
    tree: &LayerTree,
    parent_id: LayerId,
    batchers: &mut Batchers,
    state: &mut StateStack,
    scale_factor: f32,
) {
    if let Some(children) = tree.children(parent_id) {
        // Clone the slice to avoid holding a borrow on `tree` while recursing.
        let child_ids: Vec<_> = children.to_vec();
        for child_id in child_ids {
            traverse_layer(tree, child_id, batchers, state, scale_factor);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_layer::layer::{CanvasLayer, ClipRectLayer, OffsetLayer, OpacityLayer};
    use flui_types::geometry::{px, Point};

    // -- Batchers tests -------------------------------------------------------

    #[test]
    fn batchers_new_is_empty() {
        let b = Batchers::new();
        assert!(b.is_all_empty());
    }

    #[test]
    fn batchers_clear_all() {
        let mut b = Batchers::new();
        b.shapes
            .add_rect(0.0, 0.0, 10.0, 10.0, [1.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        assert!(!b.is_all_empty());
        b.clear_all();
        assert!(b.is_all_empty());
    }

    #[test]
    fn batchers_default_is_empty() {
        let b = Batchers::default();
        assert!(b.is_all_empty());
    }

    // -- Helper tests ---------------------------------------------------------

    #[test]
    fn color_to_array_converts_correctly() {
        let c = Color::rgba(255, 128, 0, 255);
        let arr = color_to_array(&c);
        assert!((arr[0] - 1.0).abs() < f32::EPSILON);
        assert!((arr[1] - 128.0 / 255.0).abs() < 0.001);
        assert!((arr[2] - 0.0).abs() < f32::EPSILON);
        assert!((arr[3] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn paint_to_color_uses_paint_color() {
        let paint = Paint::fill(Color::rgb(0, 255, 0));
        let arr = paint_to_color(&paint);
        assert!((arr[0] - 0.0).abs() < f32::EPSILON);
        assert!((arr[1] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rrect_corner_radii_extracts_x() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(8.0),
        );
        let radii = rrect_corner_radii(&rrect);
        assert!((radii[0] - 8.0).abs() < f32::EPSILON);
        assert!((radii[1] - 8.0).abs() < f32::EPSILON);
        assert!((radii[2] - 8.0).abs() < f32::EPSILON);
        assert!((radii[3] - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rect_to_xywh_extracts_fields() {
        let r = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let xywh = rect_to_xywh(&r);
        assert!((xywh[0] - 10.0).abs() < f32::EPSILON);
        assert!((xywh[1] - 20.0).abs() < f32::EPSILON);
        assert!((xywh[2] - 100.0).abs() < f32::EPSILON);
        assert!((xywh[3] - 50.0).abs() < f32::EPSILON);
    }

    // -- Dispatch tests -------------------------------------------------------

    #[test]
    fn dispatch_draw_rect_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawRect {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(30.0)),
            paint: Paint::fill(Color::rgb(255, 0, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_draw_rrect_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawRRect {
            rrect: RRect::from_rect_circular(
                Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
                px(10.0),
            ),
            paint: Paint::fill(Color::rgb(0, 0, 255)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_draw_circle_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawCircle {
            center: Point::new(px(50.0), px(50.0)),
            radius: px(25.0),
            paint: Paint::fill(Color::rgb(0, 255, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.circle_count(), 1);
    }

    #[test]
    fn dispatch_draw_oval_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawOval {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(80.0), px(40.0)),
            paint: Paint::fill(Color::rgb(128, 128, 128)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        // Ovals are dispatched as circle instances.
        assert_eq!(batchers.shapes.circle_count(), 1);
    }

    #[test]
    fn dispatch_draw_line_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawLine {
            p1: Point::new(px(0.0), px(0.0)),
            p2: Point::new(px(100.0), px(100.0)),
            paint: Paint::stroke(Color::rgb(255, 255, 255), 2.0),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.line_count(), 1);
    }

    #[test]
    fn dispatch_draw_arc_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawArc {
            rect: Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            start_angle: 0.0,
            sweep_angle: std::f32::consts::PI,
            use_center: false,
            paint: Paint::fill(Color::rgb(255, 255, 0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.arc_count(), 1);
    }

    #[test]
    fn dispatch_draw_text_routes_to_text() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawText {
            text: "Hello".to_string(),
            offset: flui_types::Offset::ZERO,
            size: flui_types::Size::new(px(50.0), px(14.0)),
            style: flui_types::typography::TextStyle::default(),
            paint: Paint::fill(Color::BLACK),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.text.run_count(), 1);
    }

    #[test]
    fn dispatch_draw_shadow_routes_to_effects() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawShadow {
            path: flui_types::painting::Path::new(),
            color: Color::rgba(0, 0, 0, 128),
            elevation: 4.0,
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.effects.shadow_count(), 1);
    }

    #[test]
    fn dispatch_draw_color_routes_to_shapes() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::DrawColor {
            color: Color::rgb(255, 0, 0),
            blend_mode: flui_types::painting::BlendMode::SrcOver,
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn dispatch_clip_rect_pushes_state() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::ClipRect {
            rect: Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        let clip = state.clip.current_clip().expect("clip should be active");
        assert!((clip.x - 10.0).abs() < f32::EPSILON);
        assert!((clip.y - 20.0).abs() < f32::EPSILON);
        assert!((clip.width - 100.0).abs() < f32::EPSILON);
        assert!((clip.height - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dispatch_clip_rrect_pushes_state() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        let cmd = DrawCommand::ClipRRect {
            rrect: RRect::from_rect_circular(
                Rect::from_xywh(px(5.0), px(5.0), px(80.0), px(60.0)),
                px(8.0),
            ),
            transform: Matrix4::identity(),
        };
        dispatch_command(&cmd, &mut batchers, &mut state, 1.0);
        let clip = state.clip.current_clip().expect("clip should be active");
        assert!((clip.x - 5.0).abs() < f32::EPSILON);
        assert!((clip.width - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dispatch_save_restore_layer() {
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();

        let save = DrawCommand::SaveLayer {
            bounds: Some(Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(200.0))),
            paint: Paint::fill(Color::BLACK),
            transform: Matrix4::identity(),
        };
        dispatch_command(&save, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.compositing.target_depth(), 1);

        let restore = DrawCommand::RestoreLayer {
            transform: Matrix4::identity(),
        };
        dispatch_command(&restore, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.compositing.target_depth(), 0);
    }

    // -- Traversal tests ------------------------------------------------------

    #[test]
    fn traverse_empty_scene_is_noop() {
        let scene = Scene::empty(flui_types::Size::new(px(800.0), px(600.0)));
        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);
        assert!(batchers.is_all_empty());
    }

    #[test]
    fn traverse_scene_with_canvas_layer() {
        use flui_painting::Canvas;

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0)),
            &Paint::fill(Color::rgb(255, 0, 0)),
        );
        let canvas_layer = CanvasLayer::from_canvas(canvas);

        let scene = Scene::from_layer(
            flui_types::Size::new(px(800.0), px(600.0)),
            Layer::Canvas(canvas_layer),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);
        assert_eq!(batchers.shapes.rect_count(), 1);
    }

    #[test]
    fn traverse_clip_rect_pushes_and_pops() {
        use flui_types::painting::Clip;

        let mut tree = LayerTree::new();
        let clip_layer = Layer::ClipRect(ClipRectLayer::new(
            Rect::from_xywh(px(10.0), px(10.0), px(200.0), px(200.0)),
            Clip::HardEdge,
        ));
        let clip_id = tree.insert(clip_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(clip_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(clip_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Clip should have been popped after traversal.
        assert!(state.clip.current_clip().is_none());
    }

    #[test]
    fn traverse_offset_layer_pushes_and_pops_transform() {
        let mut tree = LayerTree::new();
        let offset_layer = Layer::Offset(OffsetLayer::new(flui_types::Offset::new(px(10.0), px(20.0))));
        let offset_id = tree.insert(offset_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(offset_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(offset_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Transform should have been popped after traversal.
        assert_eq!(state.transform.depth(), 0);
    }

    #[test]
    fn traverse_opacity_layer_pushes_and_pops() {
        let mut tree = LayerTree::new();
        let opacity_layer = Layer::Opacity(OpacityLayer::new(0.5));
        let opacity_id = tree.insert(opacity_layer);

        let canvas_layer = Layer::Canvas(CanvasLayer::new());
        let canvas_id = tree.insert(canvas_layer);
        tree.add_child(opacity_id, canvas_id);

        let scene = Scene::new(
            flui_types::Size::new(px(800.0), px(600.0)),
            tree,
            Some(opacity_id),
            0,
        );

        let mut batchers = Batchers::new();
        let mut state = StateStack::new();
        traverse_scene(&scene, &mut batchers, &mut state, 1.0);

        // Opacity should have been popped after traversal.
        assert!((state.opacity.current() - 1.0).abs() < f32::EPSILON);
    }
}
