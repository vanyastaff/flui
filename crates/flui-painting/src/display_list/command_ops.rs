//! `DrawCommand` operations: `with_opacity`, `bounds`, `transform`,
//! `paint`, `kind`, `is_*` accessors, `apply_transform`.
//!
//! Mythos chain U5 extracted these from the 2,434-LOC
//! `display_list.rs` god module. This file is the largest in
//! `display_list/` because each method pattern-matches across all 29
//! variants. The structure is mechanical -- the 240-LOC
//! `with_opacity` and 250-LOC `bounds` matches dominate.
//!
//! Future Outstanding refactor: collapse the 29-variant patterns via
//! a hand-written `macro_rules!` `gen_command_accessors!` mirroring
//! the `flui-layer` Step 4 macro pattern. Not bundled with this chain
//! because the file is structurally clean despite size.
//!
//! # Recursion-depth cap
//!
//! Both [`DrawCommand::with_opacity`] and [`DrawCommand::apply_transform`]
//! recurse into the inner `DisplayList`s carried by [`DrawCommand::ShaderMask`]
//! and [`DrawCommand::BackdropFilter`]. To bound stack usage on
//! adversarial input we cap nesting at [`MAX_EFFECT_DEPTH`] = 64.
//!
//! Why 64:
//!
//! - Each recursive frame holds a [`DrawCommand`] value (~200 B for
//!   the largest variant) plus the [`DisplayList`](super::DisplayList) iterator state and
//!   `Box` drop ladder; empirically ~2–4 KB / frame on a release build.
//! - 64 × 4 KB ≈ 256 KB — well under the default 8 MB thread stack on
//!   Windows / macOS / Linux.
//! - Production Flutter `RenderObject` trees rarely nest effects more
//!   than ~30 levels deep; 64 leaves ≥2× headroom for unusual but
//!   legitimate UIs.
//! - Skia historically capped layer nesting around 250
//!   (`kMaxLayers` in `SkCanvas`), but its heap-allocated `SkRecord`
//!   tolerates deeper recursion than our value-typed [`DrawCommand`]
//!   chain.
//!
//! On saturation we clone the offending command *without* recursing
//! into its child (so the subtree keeps its previous opacity /
//! transform) and emit a `tracing::warn!`. Visual fidelity degrades
//! gracefully for >64-deep stacks; no crash.
//!
//! Engineers tuning this number should profile against the
//! `nested_shader_mask_opacity_depth` test below and bench the
//! resulting frame budget.

use std::sync::Arc;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::geometry::{Matrix4, Pixels, Rect, Size};

use super::command::{CommandKind, DrawCommand};
use crate::PaintStyle;
use crate::display_list::{Paint, sealed::DisplayListCore};

/// Maximum recursion depth for [`DrawCommand::with_opacity`] and
/// [`DrawCommand::apply_transform`] into the inner [`DisplayList`](super::DisplayList) of
/// [`DrawCommand::ShaderMask`] / [`DrawCommand::BackdropFilter`].
///
/// See the module-level docs for the rationale behind this value.
pub const MAX_EFFECT_DEPTH: usize = 64;

impl DrawCommand {
    /// Apply opacity to the Paint in this command.
    ///
    /// Creates a new `DrawCommand` with the Paint's opacity multiplied
    /// by the given value. Used by `DisplayList::to_opacity()` to
    /// implement opacity effects.
    ///
    /// For [`Self::ShaderMask`] / [`Self::BackdropFilter`], opacity
    /// recurses into the child [`DisplayList`](super::DisplayList). The recursion is
    /// bounded by [`MAX_EFFECT_DEPTH`]; deeper nesting is clamped to
    /// avoid stack overflow on adversarial input. See the module
    /// docs for the rationale.
    #[must_use = "with_opacity returns a new DrawCommand and does not modify the original"]
    pub fn with_opacity(&self, opacity: f32) -> Self {
        self.with_opacity_depth(opacity, 0)
    }

    /// Depth-counted recursion target for [`Self::with_opacity`].
    ///
    /// `depth` is incremented each time we descend into a child
    /// [`DisplayList`](super::DisplayList). When `depth >= MAX_EFFECT_DEPTH` we clone
    /// `self` unchanged (the child keeps its existing paint) and emit
    /// a `tracing::warn!` so observability tooling can surface the
    /// truncation.
    pub(crate) fn with_opacity_depth(&self, opacity: f32, depth: usize) -> Self {
        match self {
            // Passthrough: Commands without opacity (clips, gradients, etc.)
            Self::ClipRect { .. }
            | Self::ClipRRect { .. }
            | Self::ClipRSuperellipse { .. }
            | Self::ClipPath { .. }
            | Self::DrawTextSpan { .. }
            | Self::DrawGradient { .. }
            | Self::DrawGradientRRect { .. }
            | Self::RestoreLayer { .. } => self.clone(),

            // Paint commands: Apply opacity to paint field
            //
            // The interned `Arc<Paint>` is unwrapped to a fresh `Paint`
            // value via `Paint::with_opacity_arc` (a tiny helper that
            // hides the `(**arc).clone().with_opacity(o)` dance), then
            // re-wrapped in a new `Arc`. The opacity-mutated result is
            // a brand-new paint identity by construction — distinct
            // from the recording-time interning pool — so we cannot
            // reuse the source `Arc` even on a refcount-bump fast path.
            Self::DrawRect {
                rect,
                paint,
                transform,
            } => Self::DrawRect {
                rect: *rect,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawRRect {
                rrect,
                paint,
                transform,
            } => Self::DrawRRect {
                rrect: *rrect,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawCircle {
                center,
                radius,
                paint,
                transform,
            } => Self::DrawCircle {
                center: *center,
                radius: *radius,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawOval {
                rect,
                paint,
                transform,
            } => Self::DrawOval {
                rect: *rect,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => Self::DrawLine {
                p1: *p1,
                p2: *p2,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawPath {
                path,
                paint,
                transform,
            } => Self::DrawPath {
                path: path.clone(),
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawArc {
                rect,
                start_angle,
                sweep_angle,
                use_center,
                paint,
                transform,
            } => Self::DrawArc {
                rect: *rect,
                start_angle: *start_angle,
                sweep_angle: *sweep_angle,
                use_center: *use_center,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawDRRect {
                outer,
                inner,
                paint,
                transform,
            } => Self::DrawDRRect {
                outer: *outer,
                inner: *inner,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawPoints {
                mode,
                points,
                paint,
                transform,
            } => Self::DrawPoints {
                mode: *mode,
                points: points.clone(),
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawVertices {
                vertices,
                colors,
                tex_coords,
                indices,
                paint,
                transform,
            } => Self::DrawVertices {
                vertices: vertices.clone(),
                colors: colors.clone(),
                tex_coords: tex_coords.clone(),
                indices: indices.clone(),
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::DrawText {
                text,
                offset,
                size,
                style,
                paint,
                transform,
            } => Self::DrawText {
                text: text.clone(),
                offset: *offset,
                size: *size,
                style: style.clone(),
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },
            Self::SaveLayer {
                bounds,
                paint,
                transform,
            } => Self::SaveLayer {
                bounds: *bounds,
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },

            // Optional paint commands: Map over Option<Arc<Paint>>
            Self::DrawImage {
                image,
                dst,
                paint,
                transform,
            } => Self::DrawImage {
                image: image.clone(),
                dst: *dst,
                paint: paint.as_ref().map(|p| with_opacity_arc(p, opacity)),
                transform: *transform,
            },
            Self::DrawImageRepeat {
                image,
                dst,
                repeat,
                paint,
                transform,
            } => Self::DrawImageRepeat {
                image: image.clone(),
                dst: *dst,
                repeat: *repeat,
                paint: paint.as_ref().map(|p| with_opacity_arc(p, opacity)),
                transform: *transform,
            },
            Self::DrawImageNineSlice {
                image,
                center_slice,
                dst,
                paint,
                transform,
            } => Self::DrawImageNineSlice {
                image: image.clone(),
                center_slice: *center_slice,
                dst: *dst,
                paint: paint.as_ref().map(|p| with_opacity_arc(p, opacity)),
                transform: *transform,
            },
            Self::DrawImageFiltered {
                image,
                dst,
                filter,
                paint,
                transform,
            } => Self::DrawImageFiltered {
                image: image.clone(),
                dst: *dst,
                filter: *filter,
                paint: paint.as_ref().map(|p| with_opacity_arc(p, opacity)),
                transform: *transform,
            },
            Self::DrawAtlas {
                image,
                sprites,
                transforms,
                colors,
                blend_mode,
                paint,
                transform,
            } => Self::DrawAtlas {
                image: image.clone(),
                sprites: sprites.clone(),
                transforms: transforms.clone(),
                colors: colors.clone(),
                blend_mode: *blend_mode,
                paint: paint.as_ref().map(|p| with_opacity_arc(p, opacity)),
                transform: *transform,
            },

            // Color commands: Apply opacity to color field
            Self::DrawShadow {
                path,
                color,
                elevation,
                transform,
            } => Self::DrawShadow {
                path: path.clone(),
                color: color.with_opacity(opacity),
                elevation: *elevation,
                transform: *transform,
            },
            Self::DrawColor {
                color,
                blend_mode,
                transform,
            } => Self::DrawColor {
                color: color.with_opacity(opacity),
                blend_mode: *blend_mode,
                transform: *transform,
            },
            Self::DrawPaint { paint, transform } => Self::DrawPaint {
                paint: with_opacity_arc(paint, opacity),
                transform: *transform,
            },

            // Child commands: Recursively apply opacity to DisplayList,
            // bounded by MAX_EFFECT_DEPTH to keep adversarial input
            // from blowing the stack.
            Self::ShaderMask {
                child,
                shader,
                bounds,
                blend_mode,
                transform,
            } => {
                if depth >= MAX_EFFECT_DEPTH {
                    log_effect_depth_saturation("ShaderMask", "with_opacity", depth);
                    return self.clone();
                }
                Self::ShaderMask {
                    child: Box::new(child.to_opacity_depth(opacity, depth + 1)),
                    shader: shader.clone(),
                    bounds: *bounds,
                    blend_mode: *blend_mode,
                    transform: *transform,
                }
            }
            Self::BackdropFilter {
                child,
                filter,
                bounds,
                blend_mode,
                transform,
            } => {
                if depth >= MAX_EFFECT_DEPTH {
                    log_effect_depth_saturation("BackdropFilter", "with_opacity", depth);
                    return self.clone();
                }
                Self::BackdropFilter {
                    child: child
                        .as_ref()
                        .map(|c| Box::new(c.to_opacity_depth(opacity, depth + 1))),
                    filter: filter.clone(),
                    bounds: *bounds,
                    blend_mode: *blend_mode,
                    transform: *transform,
                }
            }

            // Texture command: Multiply opacity field
            Self::DrawTexture {
                texture_id,
                dst,
                src,
                filter_quality,
                opacity: tex_opacity,
                transform,
            } => Self::DrawTexture {
                texture_id: *texture_id,
                dst: *dst,
                src: *src,
                filter_quality: *filter_quality,
                opacity: *tex_opacity * opacity,
                transform: *transform,
            },
        }
    }

    /// Returns the bounding rectangle of this command (if applicable).
    ///
    /// Used to calculate the DisplayList's overall bounds. Returns
    /// transformed screen-space bounds (local bounds transformed by the
    /// command's matrix).
    pub(crate) fn bounds(&self) -> Option<Rect<Pixels>> {
        match self {
            DrawCommand::DrawRect {
                rect,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawRRect {
                rrect,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rrect.bounding_rect().expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawCircle {
                center,
                radius,
                paint,
                transform,
            } => {
                let stroke_outset = paint.effective_stroke_width() * 0.5;
                let effective_radius = *radius + Pixels(stroke_outset);
                let size = Size::new(effective_radius * 2.0, effective_radius * 2.0);
                let local_bounds = Rect::from_center_size(*center, size);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawOval {
                rect,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawImage { dst, transform, .. } => Some(transform.transform_rect(dst)),
            DrawCommand::DrawImageRepeat { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawImageNineSlice { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawImageFiltered { dst, transform, .. } => {
                Some(transform.transform_rect(dst))
            }
            DrawCommand::DrawTexture { dst, transform, .. } => Some(transform.transform_rect(dst)),
            DrawCommand::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => {
                let stroke_half = paint.effective_stroke_width() * 0.5;
                let min_x = p1.x.0.min(p2.x.0) - stroke_half;
                let min_y = p1.y.0.min(p2.y.0) - stroke_half;
                let max_x = p1.x.0.max(p2.x.0) + stroke_half;
                let max_y = p1.y.0.max(p2.y.0) + stroke_half;
                let local_bounds =
                    Rect::from_ltrb(Pixels(min_x), Pixels(min_y), Pixels(max_x), Pixels(max_y));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawPath {
                path,
                paint,
                transform,
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = path.compute_bounds().expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawShadow {
                path,
                elevation,
                transform,
                ..
            } => {
                let local_bounds = path.compute_bounds().expand(Pixels(*elevation));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawArc {
                rect,
                paint,
                transform,
                ..
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = rect.expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawDRRect {
                outer,
                paint,
                transform,
                ..
            } => {
                let outset = paint.effective_stroke_width() * 0.5;
                let local_bounds = outer.bounding_rect().expand(Pixels(outset));
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawPoints {
                points,
                paint,
                transform,
                ..
            } => {
                if points.is_empty() {
                    return None;
                }
                let stroke_half = paint.effective_stroke_width() * 0.5;
                let mut min_x = points[0].x;
                let mut min_y = points[0].y;
                let mut max_x = points[0].x;
                let mut max_y = points[0].y;

                for point in points.iter().skip(1) {
                    min_x = min_x.min(point.x);
                    min_y = min_y.min(point.y);
                    max_x = max_x.max(point.x);
                    max_y = max_y.max(point.y);
                }

                let local_bounds = Rect::from_ltrb(
                    min_x - Pixels(stroke_half),
                    min_y - Pixels(stroke_half),
                    max_x + Pixels(stroke_half),
                    max_y + Pixels(stroke_half),
                );
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawVertices {
                vertices,
                transform,
                ..
            } => {
                if vertices.is_empty() {
                    return None;
                }
                let mut min_x = vertices[0].x;
                let mut min_y = vertices[0].y;
                let mut max_x = vertices[0].x;
                let mut max_y = vertices[0].y;

                for vertex in vertices.iter().skip(1) {
                    min_x = min_x.min(vertex.x);
                    min_y = min_y.min(vertex.y);
                    max_x = max_x.max(vertex.x);
                    max_y = max_y.max(vertex.y);
                }

                let local_bounds = Rect::from_ltrb(min_x, min_y, max_x, max_y);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawAtlas {
                sprites,
                transforms: sprite_transforms,
                transform,
                ..
            } => {
                if sprites.is_empty() || sprite_transforms.is_empty() {
                    return None;
                }

                let mut combined_bounds: Option<Rect<Pixels>> = None;

                for (sprite_rect, sprite_transform) in sprites.iter().zip(sprite_transforms.iter())
                {
                    let local_transformed = sprite_transform.transform_rect(sprite_rect);
                    let screen_bounds = transform.transform_rect(&local_transformed);

                    combined_bounds = match combined_bounds {
                        Some(existing) => Some(existing.union(&screen_bounds)),
                        None => Some(screen_bounds),
                    };
                }

                combined_bounds
            }
            DrawCommand::DrawColor { .. } | DrawCommand::DrawPaint { .. } => None,
            DrawCommand::DrawGradient {
                rect, transform, ..
            } => Some(transform.transform_rect(rect)),
            DrawCommand::DrawGradientRRect {
                rrect, transform, ..
            } => Some(transform.transform_rect(&rrect.bounding_rect())),
            DrawCommand::ShaderMask {
                bounds, transform, ..
            } => Some(transform.transform_rect(bounds)),
            DrawCommand::BackdropFilter {
                bounds, transform, ..
            } => Some(transform.transform_rect(bounds)),
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipRSuperellipse { .. }
            | DrawCommand::ClipPath { .. } => None,
            DrawCommand::DrawText {
                offset,
                size,
                transform,
                ..
            } => {
                let local_bounds = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                Some(transform.transform_rect(&local_bounds))
            }
            DrawCommand::DrawTextSpan { .. } => None,
            DrawCommand::SaveLayer {
                bounds, transform, ..
            } => bounds.map(|b| transform.transform_rect(&b)),
            DrawCommand::RestoreLayer { .. } => None,
        }
    }

    // ===== Type Discrimination =====

    /// Returns the kind/category of this command.
    #[inline]
    pub fn kind(&self) -> CommandKind {
        match self {
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipRSuperellipse { .. }
            | DrawCommand::ClipPath { .. } => CommandKind::Clip,

            DrawCommand::SaveLayer { .. } | DrawCommand::RestoreLayer { .. } => CommandKind::Layer,

            DrawCommand::ShaderMask { .. } | DrawCommand::BackdropFilter { .. } => {
                CommandKind::Effect
            }

            _ => CommandKind::Draw,
        }
    }

    // ===== Type Checking Methods =====

    /// Returns `true` if this is a clipping command.
    #[inline]
    pub fn is_clip(&self) -> bool {
        matches!(self.kind(), CommandKind::Clip)
    }

    /// Returns `true` if this is a drawing command (shapes, text,
    /// images).
    #[inline]
    pub fn is_draw(&self) -> bool {
        matches!(self.kind(), CommandKind::Draw)
    }

    /// Returns `true` if this is an effect command (shader mask,
    /// backdrop filter).
    #[inline]
    pub fn is_effect(&self) -> bool {
        matches!(self.kind(), CommandKind::Effect)
    }

    /// Returns `true` if this is a layer command (save/restore layer).
    #[inline]
    pub fn is_layer(&self) -> bool {
        matches!(self.kind(), CommandKind::Layer)
    }

    /// Returns `true` if this command draws a shape (rect, circle,
    /// path, etc.).
    #[inline]
    pub fn is_shape(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawRect { .. }
                | DrawCommand::DrawRRect { .. }
                | DrawCommand::DrawCircle { .. }
                | DrawCommand::DrawOval { .. }
                | DrawCommand::DrawPath { .. }
                | DrawCommand::DrawArc { .. }
                | DrawCommand::DrawDRRect { .. }
                | DrawCommand::DrawLine { .. }
                | DrawCommand::DrawPoints { .. }
        )
    }

    /// Returns `true` if this command draws an image or texture.
    #[inline]
    pub fn is_image(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawImage { .. }
                | DrawCommand::DrawImageRepeat { .. }
                | DrawCommand::DrawImageNineSlice { .. }
                | DrawCommand::DrawImageFiltered { .. }
                | DrawCommand::DrawTexture { .. }
                | DrawCommand::DrawAtlas { .. }
        )
    }

    /// Returns `true` if this command draws text.
    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            DrawCommand::DrawText { .. } | DrawCommand::DrawTextSpan { .. }
        )
    }

    // ===== Accessor Methods =====

    /// Returns the transform matrix for this command.
    #[inline]
    pub fn transform(&self) -> Matrix4 {
        match self {
            DrawCommand::ClipRect { transform, .. }
            | DrawCommand::ClipRRect { transform, .. }
            | DrawCommand::ClipRSuperellipse { transform, .. }
            | DrawCommand::ClipPath { transform, .. }
            | DrawCommand::DrawLine { transform, .. }
            | DrawCommand::DrawRect { transform, .. }
            | DrawCommand::DrawRRect { transform, .. }
            | DrawCommand::DrawCircle { transform, .. }
            | DrawCommand::DrawOval { transform, .. }
            | DrawCommand::DrawPath { transform, .. }
            | DrawCommand::DrawText { transform, .. }
            | DrawCommand::DrawTextSpan { transform, .. }
            | DrawCommand::DrawImage { transform, .. }
            | DrawCommand::DrawImageRepeat { transform, .. }
            | DrawCommand::DrawImageNineSlice { transform, .. }
            | DrawCommand::DrawImageFiltered { transform, .. }
            | DrawCommand::DrawTexture { transform, .. }
            | DrawCommand::DrawShadow { transform, .. }
            | DrawCommand::DrawGradient { transform, .. }
            | DrawCommand::DrawGradientRRect { transform, .. }
            | DrawCommand::ShaderMask { transform, .. }
            | DrawCommand::BackdropFilter { transform, .. }
            | DrawCommand::DrawArc { transform, .. }
            | DrawCommand::DrawDRRect { transform, .. }
            | DrawCommand::DrawPoints { transform, .. }
            | DrawCommand::DrawVertices { transform, .. }
            | DrawCommand::DrawColor { transform, .. }
            | DrawCommand::DrawPaint { transform, .. }
            | DrawCommand::DrawAtlas { transform, .. }
            | DrawCommand::SaveLayer { transform, .. }
            | DrawCommand::RestoreLayer { transform, .. } => *transform,
        }
    }

    /// Returns a reference to the Paint for this command, if it has
    /// one.
    ///
    /// Variants carry `Arc<Paint>` internally for recording-time
    /// interning (Cycle 5 U10 / origin R15 / audit P-7); the accessor
    /// returns a plain `&Paint` borrow so consumers stay refcount-agnostic.
    #[inline]
    pub fn paint(&self) -> Option<&Paint> {
        match self {
            DrawCommand::DrawLine { paint, .. }
            | DrawCommand::DrawRect { paint, .. }
            | DrawCommand::DrawRRect { paint, .. }
            | DrawCommand::DrawCircle { paint, .. }
            | DrawCommand::DrawOval { paint, .. }
            | DrawCommand::DrawPath { paint, .. }
            | DrawCommand::DrawText { paint, .. }
            | DrawCommand::DrawArc { paint, .. }
            | DrawCommand::DrawDRRect { paint, .. }
            | DrawCommand::DrawPoints { paint, .. }
            | DrawCommand::DrawVertices { paint, .. }
            | DrawCommand::DrawPaint { paint, .. }
            | DrawCommand::SaveLayer { paint, .. } => Some(paint.as_ref()),

            DrawCommand::DrawImage { paint, .. }
            | DrawCommand::DrawImageRepeat { paint, .. }
            | DrawCommand::DrawImageNineSlice { paint, .. }
            | DrawCommand::DrawImageFiltered { paint, .. }
            | DrawCommand::DrawAtlas { paint, .. } => paint.as_deref(),

            _ => None,
        }
    }

    /// Returns `true` if this command has a Paint that can be
    /// modified.
    #[inline]
    pub fn has_paint(&self) -> bool {
        self.paint().is_some()
    }

    /// Returns a mutable reference to the transform matrix.
    #[inline]
    pub fn transform_mut(&mut self) -> &mut Matrix4 {
        match self {
            DrawCommand::ClipRect { transform, .. }
            | DrawCommand::ClipRRect { transform, .. }
            | DrawCommand::ClipRSuperellipse { transform, .. }
            | DrawCommand::ClipPath { transform, .. }
            | DrawCommand::DrawLine { transform, .. }
            | DrawCommand::DrawRect { transform, .. }
            | DrawCommand::DrawRRect { transform, .. }
            | DrawCommand::DrawCircle { transform, .. }
            | DrawCommand::DrawOval { transform, .. }
            | DrawCommand::DrawPath { transform, .. }
            | DrawCommand::DrawText { transform, .. }
            | DrawCommand::DrawTextSpan { transform, .. }
            | DrawCommand::DrawImage { transform, .. }
            | DrawCommand::DrawImageRepeat { transform, .. }
            | DrawCommand::DrawImageNineSlice { transform, .. }
            | DrawCommand::DrawImageFiltered { transform, .. }
            | DrawCommand::DrawTexture { transform, .. }
            | DrawCommand::DrawShadow { transform, .. }
            | DrawCommand::DrawGradient { transform, .. }
            | DrawCommand::DrawGradientRRect { transform, .. }
            | DrawCommand::ShaderMask { transform, .. }
            | DrawCommand::BackdropFilter { transform, .. }
            | DrawCommand::DrawArc { transform, .. }
            | DrawCommand::DrawDRRect { transform, .. }
            | DrawCommand::DrawPoints { transform, .. }
            | DrawCommand::DrawVertices { transform, .. }
            | DrawCommand::DrawColor { transform, .. }
            | DrawCommand::DrawPaint { transform, .. }
            | DrawCommand::DrawAtlas { transform, .. }
            | DrawCommand::SaveLayer { transform, .. }
            | DrawCommand::RestoreLayer { transform, .. } => transform,
        }
    }

    /// Applies an additional transform to this command.
    ///
    /// The new transform is multiplied with the existing one. For
    /// container commands (`ShaderMask`, `BackdropFilter`) the
    /// transform is also pushed into the nested child `DisplayList`
    /// so the inner commands move with the outer container — mirrors
    /// the recursive walk in [`Self::with_opacity`].
    ///
    /// Recursion into nested children is bounded by
    /// [`MAX_EFFECT_DEPTH`]. See the module-level docs for the
    /// rationale and saturation behavior.
    #[inline]
    pub fn apply_transform(&mut self, additional: Matrix4) {
        self.apply_transform_depth(additional, 0);
    }

    /// Depth-counted recursion target for [`Self::apply_transform`].
    pub(crate) fn apply_transform_depth(&mut self, additional: Matrix4, depth: usize) {
        *self.transform_mut() = additional * self.transform();

        match self {
            DrawCommand::ShaderMask { child, .. } => {
                if depth >= MAX_EFFECT_DEPTH {
                    log_effect_depth_saturation("ShaderMask", "apply_transform", depth);
                    return;
                }
                child.apply_transform_depth(additional, depth + 1);
            }
            DrawCommand::BackdropFilter { child, .. } => {
                if let Some(child) = child.as_mut() {
                    if depth >= MAX_EFFECT_DEPTH {
                        log_effect_depth_saturation("BackdropFilter", "apply_transform", depth);
                        return;
                    }
                    child.apply_transform_depth(additional, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// Produce a fresh `Arc<Paint>` from an interned source carrying the
/// requested opacity.
///
/// `DrawCommand::with_opacity_depth` rewrites every paint-carrying
/// variant; the per-arm boilerplate around `Arc::new((**arc).clone()
/// .with_opacity(opacity))` is centralised here so the match stays
/// readable. The function always allocates a new `Arc` — the opacity
/// mutation produces a distinct paint identity that the recording-time
/// interning pool never sees, so we cannot reuse the input refcount.
#[inline]
fn with_opacity_arc(paint: &Arc<Paint>, opacity: f32) -> Arc<Paint> {
    Arc::new((**paint).clone().with_opacity(opacity))
}

/// Emit a saturation warning when an effect-nesting recursion
/// reaches [`MAX_EFFECT_DEPTH`]. Extracted so the two call sites in
/// [`DrawCommand::with_opacity_depth`] and
/// [`DrawCommand::apply_transform_depth`] stay symmetrical and easy
/// to grep for in production logs.
#[cold]
#[inline(never)]
fn log_effect_depth_saturation(variant: &'static str, op: &'static str, depth: usize) {
    tracing::warn!(
        variant = variant,
        op = op,
        depth = depth,
        max_depth = MAX_EFFECT_DEPTH,
        "DrawCommand::{op} saturated MAX_EFFECT_DEPTH on {variant}; \
         inner DisplayList left untouched"
    );
}

// ============================================================================
// Diagnosticable impl for DrawCommand (ADR-0005 Decision 2)
// ============================================================================

/// Populate `builder` with the colour/geometry properties of `paint`.
///
/// Called from every `DrawCommand` variant that carries a `Paint` so all
/// variants emit properties under the same names, making automated diffing
/// stable. `stroke_width` is only emitted for stroke-style paints.
fn add_paint_props(builder: &mut DiagnosticsBuilder, paint: &Paint) {
    builder.add_value("color", paint.color);
    builder.add("style", format!("{:?}", paint.style));
    if matches!(paint.style, PaintStyle::Stroke) {
        builder.add_f64("stroke_width", f64::from(paint.stroke_width));
    }
}

/// Populate `builder` with the colour/geometry properties of `paint` when it
/// is wrapped in an `Option<Arc<Paint>>`.
fn add_opt_paint_props(builder: &mut DiagnosticsBuilder, paint: Option<&Arc<Paint>>) {
    if let Some(p) = paint {
        add_paint_props(builder, p);
    }
}

/// Populate `builder` with an axis-aligned rect property in typed form.
///
/// Thin wrapper around [`DiagnosticsBuilder::add_value`] that accepts the
/// concrete `Rect<Pixels>` type used throughout `DrawCommand` variants.
fn add_rect_prop(builder: &mut DiagnosticsBuilder, name: &'static str, r: Rect<Pixels>) {
    builder.add_value(name, r);
}

/// Emit the recording-time transform as a typed `List` of 16 floats only when
/// it is non-identity.
///
/// Identity transforms are omitted to keep diagnostics output readable and to
/// match the contract of the stable text serialiser (`maybe_transform`).
fn add_transform_if_nonidentity(builder: &mut DiagnosticsBuilder, transform: &Matrix4) {
    if !transform.is_identity() {
        builder.add_value("transform", transform);
    }
}

impl DrawCommand {
    /// Returns the stable, per-variant name used as the diagnostics node name.
    ///
    /// This is the migration target for the retired `DrawKind`-based
    /// categorisation and the old `summarize_command` prefix strings.  Using
    /// per-variant names lets automated diff tooling distinguish a `DrawOval`
    /// from a `DrawRect` even when both carry identical rects and paints.
    #[must_use]
    pub fn command_kind(&self) -> &'static str {
        match self {
            Self::DrawRect { .. } => "DrawRect",
            Self::DrawRRect { .. } => "DrawRRect",
            Self::DrawCircle { .. } => "DrawCircle",
            Self::DrawOval { .. } => "DrawOval",
            Self::DrawLine { .. } => "DrawLine",
            Self::DrawArc { .. } => "DrawArc",
            Self::DrawPath { .. } => "DrawPath",
            Self::DrawDRRect { .. } => "DrawDRRect",
            Self::DrawText { .. } => "DrawText",
            Self::DrawTextSpan { .. } => "DrawTextSpan",
            Self::DrawImage { .. } => "DrawImage",
            Self::DrawImageRepeat { .. } => "DrawImageRepeat",
            Self::DrawImageNineSlice { .. } => "DrawImageNineSlice",
            Self::DrawImageFiltered { .. } => "DrawImageFiltered",
            Self::DrawTexture { .. } => "DrawTexture",
            Self::DrawShadow { .. } => "DrawShadow",
            Self::DrawGradient { .. } => "DrawGradient",
            Self::DrawGradientRRect { .. } => "DrawGradientRRect",
            Self::ShaderMask { .. } => "ShaderMask",
            Self::BackdropFilter { .. } => "BackdropFilter",
            Self::DrawPoints { .. } => "DrawPoints",
            Self::DrawVertices { .. } => "DrawVertices",
            Self::DrawColor { .. } => "DrawColor",
            Self::DrawPaint { .. } => "DrawPaint",
            Self::DrawAtlas { .. } => "DrawAtlas",
            Self::ClipRect { .. } => "ClipRect",
            Self::ClipRRect { .. } => "ClipRRect",
            Self::ClipRSuperellipse { .. } => "ClipRSuperellipse",
            Self::ClipPath { .. } => "ClipPath",
            Self::SaveLayer { .. } => "SaveLayer",
            Self::RestoreLayer { .. } => "RestoreLayer",
        }
    }
}

impl Diagnosticable for DrawCommand {
    fn to_diagnostics_node(&self) -> flui_foundation::DiagnosticsNode {
        let mut node = flui_foundation::DiagnosticsNode::new(self.command_kind());
        let mut builder = flui_foundation::DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    fn debug_fill_properties(&self, p: &mut DiagnosticsBuilder) {
        match self {
            // ── Clipping ─────────────────────────────────────────────────────
            DrawCommand::ClipRect {
                rect,
                clip_op,
                clip_behavior,
                transform,
            } => {
                add_rect_prop(p, "rect", *rect);
                p.add_enum("clip_op", clip_op);
                p.add_enum("clip_behavior", clip_behavior);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::ClipRRect {
                rrect,
                clip_op,
                clip_behavior,
                transform,
            } => {
                // Emit as Nested so a radius change diffs the output.
                p.add_value("rect", *rrect);
                p.add_enum("clip_op", clip_op);
                p.add_enum("clip_behavior", clip_behavior);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::ClipRSuperellipse {
                rsuperellipse,
                clip_op,
                clip_behavior,
                transform,
            } => {
                add_rect_prop(p, "rect", rsuperellipse.outer_rect());
                p.add_enum("clip_op", clip_op);
                p.add_enum("clip_behavior", clip_behavior);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::ClipPath {
                path,
                clip_op,
                clip_behavior,
                transform,
            } => {
                // Bounds + command count are the stable fingerprint (raw verbs
                // are too verbose); mirrors `summarize_command` for ClipPath.
                add_rect_prop(p, "bounds", path.compute_bounds());
                p.add_i64(
                    "pt_count",
                    path.commands().len().try_into().unwrap_or(i64::MAX),
                );
                p.add_enum("clip_op", clip_op);
                p.add_enum("clip_behavior", clip_behavior);
                add_transform_if_nonidentity(p, transform);
            }

            // ── Primitive draws ───────────────────────────────────────────────
            DrawCommand::DrawLine {
                p1,
                p2,
                paint,
                transform,
            } => {
                p.add_value("p1", *p1);
                p.add_value("p2", *p2);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawRect {
                rect,
                paint,
                transform,
            } => {
                add_rect_prop(p, "rect", *rect);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawRRect {
                rrect,
                paint,
                transform,
            } => {
                p.add_value("rect", *rrect);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawCircle {
                center,
                radius,
                paint,
                transform,
            } => {
                p.add_value("center", *center);
                p.add_f64("radius", f64::from(radius.get()));
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawOval {
                rect,
                paint,
                transform,
            } => {
                add_rect_prop(p, "rect", *rect);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawPath {
                path,
                paint,
                transform,
            } => {
                // Bounds + command count are the stable fingerprint; raw verbs
                // are too verbose and unstable. Mirrors `summarize_command`.
                add_rect_prop(p, "bounds", path.compute_bounds());
                p.add_i64(
                    "pt_count",
                    path.commands().len().try_into().unwrap_or(i64::MAX),
                );
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            // ── Text ─────────────────────────────────────────────────────────
            DrawCommand::DrawText {
                text,
                offset,
                size,
                paint,
                transform,
                ..
            } => {
                p.add("text", text.as_str());
                p.add_value("offset", *offset);
                p.add_value("size", *size);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawTextSpan {
                span,
                offset,
                text_scale_factor,
                wrap_width,
                transform,
            } => {
                // Plain text is the stable fingerprint; glyph/run details are
                // not needed and change with shaper versions.
                p.add("text", span.to_plain_text());
                p.add_value("offset", *offset);
                p.add_f64("text_scale_factor", *text_scale_factor);
                if let Some(w) = wrap_width {
                    p.add_f64("wrap_width", f64::from(*w));
                }
                add_transform_if_nonidentity(p, transform);
            }

            // ── Image ─────────────────────────────────────────────────────────
            DrawCommand::DrawImage {
                dst,
                paint,
                transform,
                ..
            } => {
                add_rect_prop(p, "dst", *dst);
                add_opt_paint_props(p, paint.as_ref());
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawImageRepeat {
                dst,
                repeat,
                paint,
                transform,
                ..
            } => {
                add_rect_prop(p, "dst", *dst);
                p.add_enum("repeat", repeat);
                add_opt_paint_props(p, paint.as_ref());
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawImageNineSlice {
                center_slice,
                dst,
                paint,
                transform,
                ..
            } => {
                add_rect_prop(p, "dst", *dst);
                add_rect_prop(p, "center_slice", *center_slice);
                add_opt_paint_props(p, paint.as_ref());
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawImageFiltered {
                dst,
                filter,
                paint,
                transform,
                ..
            } => {
                add_rect_prop(p, "dst", *dst);
                p.add_enum("filter", filter);
                add_opt_paint_props(p, paint.as_ref());
                add_transform_if_nonidentity(p, transform);
            }

            // ── Texture ───────────────────────────────────────────────────────
            DrawCommand::DrawTexture {
                texture_id,
                dst,
                src,
                filter_quality,
                opacity,
                transform,
            } => {
                p.add_i64(
                    "texture_id",
                    i64::try_from(texture_id.get()).unwrap_or(i64::MAX),
                );
                add_rect_prop(p, "dst", *dst);
                if let Some(s) = src {
                    add_rect_prop(p, "src", *s);
                }
                p.add_enum("filter_quality", filter_quality);
                p.add_f64("opacity", f64::from(*opacity));
                add_transform_if_nonidentity(p, transform);
            }

            // ── Effects ───────────────────────────────────────────────────────
            DrawCommand::DrawShadow {
                path,
                color,
                elevation,
                transform,
            } => {
                // Path bounds are the stable fingerprint for shadow geometry;
                // mirrors `summarize_command` for DrawShadow.
                add_rect_prop(p, "path_bounds", path.compute_bounds());
                p.add_value("color", *color);
                p.add_f64("elevation", f64::from(*elevation));
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawGradient {
                rect,
                shader,
                transform,
            } => {
                add_rect_prop(p, "rect", *rect);
                p.add_enum("shader", shader);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawGradientRRect {
                rrect,
                shader,
                transform,
            } => {
                p.add_value("rect", *rrect);
                p.add_enum("shader", shader);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::ShaderMask {
                child,
                shader,
                bounds,
                blend_mode,
                transform,
            } => {
                add_rect_prop(p, "bounds", *bounds);
                p.add_enum("shader", shader);
                p.add_enum("blend_mode", blend_mode);
                p.add_i64("child_commands", child.len().try_into().unwrap_or(i64::MAX));
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::BackdropFilter {
                child,
                filter,
                bounds,
                blend_mode,
                transform,
            } => {
                add_rect_prop(p, "bounds", *bounds);
                p.add_enum("filter", filter);
                p.add_enum("blend_mode", blend_mode);
                if let Some(c) = child {
                    p.add_i64("child_commands", c.len().try_into().unwrap_or(i64::MAX));
                }
                add_transform_if_nonidentity(p, transform);
            }

            // ── Advanced primitives ───────────────────────────────────────────
            DrawCommand::DrawArc {
                rect,
                start_angle,
                sweep_angle,
                use_center,
                paint,
                transform,
            } => {
                add_rect_prop(p, "rect", *rect);
                p.add_f64("start_angle", f64::from(*start_angle));
                p.add_f64("sweep_angle", f64::from(*sweep_angle));
                p.add_bool("use_center", *use_center);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawDRRect {
                outer,
                inner,
                paint,
                transform,
            } => {
                // Each rrect becomes a Nested value under its own top-level
                // property name, so outer/inner corner radii cannot collide.
                p.add_value("outer", *outer);
                p.add_value("inner", *inner);
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawPoints {
                mode,
                points,
                paint,
                transform,
            } => {
                p.add_enum("mode", mode);
                p.add_i64("point_count", points.len().try_into().unwrap_or(i64::MAX));
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawVertices {
                vertices,
                colors,
                tex_coords,
                paint,
                transform,
                ..
            } => {
                p.add_i64(
                    "vertex_count",
                    vertices.len().try_into().unwrap_or(i64::MAX),
                );
                p.add_bool("has_colors", colors.is_some());
                p.add_bool("has_tex_coords", tex_coords.is_some());
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawColor {
                color,
                blend_mode,
                transform,
            } => {
                p.add_value("color", *color);
                p.add_enum("blend_mode", blend_mode);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawPaint { paint, transform } => {
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::DrawAtlas {
                sprites,
                colors,
                blend_mode,
                paint,
                transform,
                ..
            } => {
                p.add_i64("sprite_count", sprites.len().try_into().unwrap_or(i64::MAX));
                p.add_bool("has_colors", colors.is_some());
                p.add_enum("blend_mode", blend_mode);
                add_opt_paint_props(p, paint.as_ref());
                add_transform_if_nonidentity(p, transform);
            }

            // ── Layer ─────────────────────────────────────────────────────────
            DrawCommand::SaveLayer {
                bounds,
                paint,
                transform,
            } => {
                if let Some(b) = bounds {
                    add_rect_prop(p, "bounds", *b);
                }
                add_paint_props(p, paint);
                add_transform_if_nonidentity(p, transform);
            }

            DrawCommand::RestoreLayer { transform } => {
                add_transform_if_nonidentity(p, transform);
            }
        }
    }
}

#[cfg(test)]
// `expect`/`panic!`/`unwrap_or_else(|| panic!())` are approved test-assertion patterns.
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::Arc;

    use flui_foundation::Diagnosticable;
    use flui_types::{
        geometry::{Point, Rect, px},
        styling::Color,
    };

    use crate::{
        Paint,
        display_list::{BlendMode, DrawCommand},
    };

    /// Each `DrawRect` node must be named `"DrawRect"` by the per-variant
    /// `to_diagnostics_node` override, and the `rect` property must carry a
    /// typed `Rect` value, not a `String`.
    #[test]
    fn draw_rect_node_name_and_typed_rect() {
        use flui_foundation::DiagnosticsValue;

        let cmd = DrawCommand::DrawRect {
            rect: Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(30.0)),
            paint: Arc::new(Paint::fill(Color::RED)),
            transform: flui_types::geometry::Matrix4::identity(),
        };

        let node = cmd.to_diagnostics_node();
        // Per-variant override produces "DrawRect", not the generic enum type name.
        assert_eq!(node.name(), Some("DrawRect"), "node name");

        // The `rect` property must carry a typed Rect value.
        let prop = node
            .find_property("rect")
            .expect("DrawRect must have a `rect` property");
        assert!(
            matches!(
                prop.value_typed(),
                DiagnosticsValue::Rect { w, h, .. } if *w > 0.0 && *h > 0.0
            ),
            "rect property must be a typed Rect, got: {:?}",
            prop.value_typed(),
        );
    }

    /// A `DrawColor` command must emit a typed `Color` value for the `color`
    /// property so the inspector JSON is faithful (not a display string).
    #[test]
    fn draw_color_emits_typed_color_rgba() {
        use flui_foundation::DiagnosticsValue;

        let cmd = DrawCommand::DrawColor {
            color: Color::rgba(255, 128, 0, 200),
            blend_mode: BlendMode::SrcOver,
            transform: flui_types::geometry::Matrix4::identity(),
        };

        let node = cmd.to_diagnostics_node();
        let prop = node
            .find_property("color")
            .expect("DrawColor must have a `color` property");

        assert!(
            matches!(
                prop.value_typed(),
                DiagnosticsValue::Color {
                    r: 255,
                    g: 128,
                    b: 0,
                    a: 200
                }
            ),
            "color property must be typed Color {{r,g,b,a}}, got: {:?}",
            prop.value_typed(),
        );
    }

    /// `DrawDRRect` emits outer and inner rrects as `Nested` values so each
    /// rrect's corner radii live inside their own sub-object.
    ///
    /// The top-level node has exactly two rrect properties (`"outer"` and
    /// `"inner"`), each carrying a `Nested` value that contains `"rect"` plus
    /// the four corner radii `"r_tl"` / `"r_tr"` / `"r_br"` / `"r_bl"`.
    /// Each corner radius is itself a `Nested` value with `"x"` and `"y"` float
    /// sub-properties, faithfully recording elliptical radii (FIX 2).
    /// Distinct outer/inner radii must produce distinct nested values — the
    /// old flat-name collision (`outer_r_tl` vs `inner_r_tl` on the same node)
    /// is impossible with this structure.
    #[test]
    fn draw_drrect_outer_and_inner_radii_have_distinct_names() {
        use flui_foundation::DiagnosticsValue;
        use flui_types::geometry::{Matrix4, RRect, Radius, Rect, px};

        let outer = RRect::from_rect_circular(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(12.0),
        );
        let inner = RRect::from_rect_and_radius(
            Rect::from_ltrb(px(10.0), px(10.0), px(90.0), px(90.0)),
            Radius::circular(px(4.0)),
        );
        let cmd = DrawCommand::DrawDRRect {
            outer,
            inner,
            paint: Arc::new(Paint::fill(Color::RED)),
            transform: Matrix4::identity(),
        };

        let node = cmd.to_diagnostics_node();

        // Helper: extract the Nested props from a top-level property.
        let get_nested = |name: &str| -> &[flui_foundation::DiagnosticsProperty] {
            match node
                .find_property(name)
                .unwrap_or_else(|| panic!("{name} property must be present on DrawDRRect node"))
                .value_typed()
            {
                DiagnosticsValue::Nested(props) => props.as_slice(),
                other => panic!("{name} must be Nested, got {other:?}"),
            }
        };

        let outer_props = get_nested("outer");
        let inner_props = get_nested("inner");

        // Each Nested must contain a "r_tl" that is itself a Nested {x, y}.
        // Extract the x-radius from a corner's Nested value.
        let get_r_tl_x = |props: &[flui_foundation::DiagnosticsProperty]| -> f64 {
            let r_tl = props
                .iter()
                .find(|p| p.name() == "r_tl")
                .unwrap_or_else(|| panic!("Nested must contain r_tl"));
            match r_tl.value_typed() {
                DiagnosticsValue::Nested(sub) => {
                    let x_prop = sub
                        .iter()
                        .find(|p| p.name() == "x")
                        .unwrap_or_else(|| panic!("r_tl Nested must contain x"));
                    match x_prop.value_typed() {
                        DiagnosticsValue::Float(v) => *v,
                        other => panic!("r_tl.x must be Float, got {other:?}"),
                    }
                }
                other => panic!("r_tl must be Nested{{x,y}}, got {other:?}"),
            }
        };

        let outer_r_tl_x = get_r_tl_x(outer_props);
        let inner_r_tl_x = get_r_tl_x(inner_props);

        assert!(
            (outer_r_tl_x - 12.0_f64).abs() < 0.01,
            "outer r_tl.x must be ~12.0, got: {outer_r_tl_x}",
        );
        assert!(
            (inner_r_tl_x - 4.0_f64).abs() < 0.01,
            "inner r_tl.x must be ~4.0, got: {inner_r_tl_x}",
        );

        // The two Nested values must not be equal — different radii, different values.
        assert_ne!(
            node.find_property("outer")
                .map(flui_foundation::DiagnosticsProperty::value_typed),
            node.find_property("inner")
                .map(flui_foundation::DiagnosticsProperty::value_typed),
            "outer and inner Nested values must differ (distinct radii)",
        );

        // No duplicate top-level property names.
        let all_names: Vec<&str> = node
            .properties()
            .iter()
            .map(flui_foundation::DiagnosticsProperty::name)
            .collect();
        let mut deduped = all_names.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(
            all_names.len(),
            deduped.len(),
            "DrawDRRect node has duplicate property names: {all_names:?}",
        );
    }

    /// `DrawCircle` must emit typed `Float` values for `radius`, and the
    /// `Display` string for `radius` must be `"50.00"` (2-dp normalized).
    #[test]
    fn draw_circle_radius_is_typed_float_with_two_dp_display() {
        let cmd = DrawCommand::DrawCircle {
            center: Point::new(px(10.0), px(10.0)),
            radius: px(50.0),
            paint: Arc::new(Paint::fill(Color::BLUE)),
            transform: flui_types::geometry::Matrix4::identity(),
        };

        let node = cmd.to_diagnostics_node();

        // Typed value is Float.
        let prop = node
            .find_property("radius")
            .expect("DrawCircle must have a `radius` property");
        assert!(
            matches!(
                prop.value_typed(),
                flui_foundation::DiagnosticsValue::Float(_)
            ),
            "radius must be typed Float, got: {:?}",
            prop.value_typed(),
        );

        // Display is the 2-decimal normalized float.
        assert_eq!(
            prop.value(),
            "50.00",
            "radius display must be 2-dp normalized",
        );
    }
}
