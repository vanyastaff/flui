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

use flui_types::geometry::{Matrix4, Pixels, Rect, Size};

use super::command::{CommandKind, DrawCommand};
use crate::display_list::Paint;

impl DrawCommand {
    /// Apply opacity to the Paint in this command.
    ///
    /// Creates a new `DrawCommand` with the Paint's opacity multiplied
    /// by the given value. Used by `DisplayList::to_opacity()` to
    /// implement opacity effects.
    #[must_use = "with_opacity returns a new DrawCommand and does not modify the original"]
    pub fn with_opacity(&self, opacity: f32) -> Self {
        match self {
            // Passthrough: Commands without opacity (clips, gradients, etc.)
            Self::ClipRect { .. }
            | Self::ClipRRect { .. }
            | Self::ClipPath { .. }
            | Self::DrawTextSpan { .. }
            | Self::DrawGradient { .. }
            | Self::DrawGradientRRect { .. }
            | Self::RestoreLayer { .. } => self.clone(),

            // Paint commands: Apply opacity to paint field
            Self::DrawRect {
                rect,
                paint,
                transform,
            } => Self::DrawRect {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawRRect {
                rrect,
                paint,
                transform,
            } => Self::DrawRRect {
                rrect: *rrect,
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawOval {
                rect,
                paint,
                transform,
            } => Self::DrawOval {
                rect: *rect,
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::DrawPath {
                path,
                paint,
                transform,
            } => Self::DrawPath {
                path: path.clone(),
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
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
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },
            Self::SaveLayer {
                bounds,
                paint,
                transform,
            } => Self::SaveLayer {
                bounds: *bounds,
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },

            // Optional paint commands: Map over Option<Paint>
            Self::DrawImage {
                image,
                dst,
                paint,
                transform,
            } => Self::DrawImage {
                image: image.clone(),
                dst: *dst,
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
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
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
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
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
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
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
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
                paint: paint.as_ref().map(|p| p.clone().with_opacity(opacity)),
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
                paint: paint.clone().with_opacity(opacity),
                transform: *transform,
            },

            // Child commands: Recursively apply opacity to DisplayList
            Self::ShaderMask {
                child,
                shader,
                bounds,
                blend_mode,
                transform,
            } => Self::ShaderMask {
                child: Box::new(child.to_opacity(opacity)),
                shader: shader.clone(),
                bounds: *bounds,
                blend_mode: *blend_mode,
                transform: *transform,
            },
            Self::BackdropFilter {
                child,
                filter,
                bounds,
                blend_mode,
                transform,
            } => Self::BackdropFilter {
                child: child.as_ref().map(|c| Box::new(c.to_opacity(opacity))),
                filter: filter.clone(),
                bounds: *bounds,
                blend_mode: *blend_mode,
                transform: *transform,
            },

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
            | DrawCommand::SaveLayer { paint, .. } => Some(paint),

            DrawCommand::DrawImage { paint, .. }
            | DrawCommand::DrawImageRepeat { paint, .. }
            | DrawCommand::DrawImageNineSlice { paint, .. }
            | DrawCommand::DrawImageFiltered { paint, .. }
            | DrawCommand::DrawAtlas { paint, .. } => paint.as_ref(),

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
    /// The new transform is multiplied with the existing one.
    #[inline]
    pub fn apply_transform(&mut self, additional: Matrix4) {
        *self.transform_mut() = additional * self.transform();
    }
}
