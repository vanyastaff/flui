//! RenderCommand dispatch
//!
//! This module provides the dispatch functions that route DrawCommands to the
//! appropriate CommandRenderer methods. It follows the Visitor pattern to
//! separate command data from execution logic.
//!
//! # Architecture
//!
//! ```text
//! DrawCommand (flui_painting)
//!     │
//!     ▼
//! dispatch_command() ─────► CommandRenderer.render_*()
//!                                 │
//!                                 ▼
//!                           Backend (wgpu, skia, etc.)
//! ```

use crate::traits::CommandRenderer;
use flui_painting::DrawCommand;

/// Dispatch a single DrawCommand to the appropriate CommandRenderer method
///
/// This is the core visitor dispatch function. It performs type-safe
/// double-dispatch: the command type determines which renderer method is called.
///
/// # Arguments
///
/// * `command` - The drawing command to execute
/// * `renderer` - The renderer that will execute the command
///
/// # Performance
///
/// The match statement compiles to a jump table, making dispatch O(1).
/// Uses static dispatch via generics for zero-overhead renderer calls.
#[inline]
pub fn dispatch_command<R: CommandRenderer + ?Sized>(command: &DrawCommand, renderer: &mut R) {
    match command {
        // === Drawing Commands ===
        DrawCommand::DrawRect {
            rect,
            paint,
            transform,
        } => {
            renderer.render_rect(*rect, paint, transform);
        }
        DrawCommand::DrawRRect {
            rrect,
            paint,
            transform,
        } => {
            renderer.render_rrect(*rrect, paint, transform);
        }
        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            transform,
        } => {
            renderer.render_circle(*center, radius.0, paint, transform);
        }
        DrawCommand::DrawLine {
            p1,
            p2,
            paint,
            transform,
        } => {
            renderer.render_line(*p1, *p2, paint, transform);
        }
        DrawCommand::DrawOval {
            rect,
            paint,
            transform,
        } => {
            renderer.render_oval(*rect, paint, transform);
        }
        DrawCommand::DrawPath {
            path,
            paint,
            transform,
        } => {
            renderer.render_path(path, paint, transform);
        }
        DrawCommand::DrawText {
            text,
            offset,
            size: _,
            style,
            paint,
            transform,
        } => {
            renderer.render_text(text, *offset, style, paint, transform);
        }
        DrawCommand::DrawTextSpan {
            span,
            offset,
            text_scale_factor,
            transform,
        } => {
            renderer.render_text_span(span, *offset, *text_scale_factor, transform);
        }
        DrawCommand::DrawImage {
            image,
            dst,
            paint,
            transform,
        } => {
            renderer.render_image(image, *dst, paint.as_ref(), transform);
        }
        DrawCommand::DrawTexture {
            texture_id,
            dst,
            src,
            filter_quality,
            opacity,
            transform,
        } => {
            renderer.render_texture(
                *texture_id,
                *dst,
                *src,
                *filter_quality,
                *opacity,
                transform,
            );
        }
        DrawCommand::DrawShadow {
            path,
            color,
            elevation,
            transform,
        } => {
            renderer.render_shadow(path, *color, *elevation, transform);
        }
        DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
            transform,
        } => {
            renderer.render_arc(
                *rect,
                *start_angle,
                *sweep_angle,
                *use_center,
                paint,
                transform,
            );
        }
        DrawCommand::DrawDRRect {
            outer,
            inner,
            paint,
            transform,
        } => {
            renderer.render_drrect(*outer, *inner, paint, transform);
        }
        DrawCommand::DrawPoints {
            mode,
            points,
            paint,
            transform,
        } => {
            renderer.render_points(*mode, points, paint, transform);
        }
        DrawCommand::DrawVertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
            transform,
        } => {
            renderer.render_vertices(
                vertices,
                colors.as_deref(),
                tex_coords.as_deref(),
                indices,
                paint,
                transform,
            );
        }
        DrawCommand::DrawColor {
            color,
            blend_mode,
            transform,
        } => {
            renderer.render_color(*color, *blend_mode, transform);
        }
        DrawCommand::DrawAtlas {
            image,
            sprites,
            transforms,
            colors,
            blend_mode,
            paint,
            transform,
        } => {
            renderer.render_atlas(
                image,
                sprites,
                transforms,
                colors.as_deref(),
                *blend_mode,
                paint.as_ref(),
                transform,
            );
        }

        // === Gradient Commands ===
        DrawCommand::DrawGradient {
            rect,
            shader,
            transform,
        } => {
            renderer.render_gradient(*rect, shader, transform);
        }
        DrawCommand::DrawGradientRRect {
            rrect,
            shader,
            transform,
        } => {
            renderer.render_gradient_rrect(*rrect, shader, transform);
        }

        // === Effects ===
        DrawCommand::ShaderMask {
            child,
            shader,
            bounds,
            blend_mode,
            transform,
        } => {
            renderer.render_shader_mask(child, shader, *bounds, *blend_mode, transform);
        }

        // === Clipping Commands ===
        DrawCommand::ClipRect { rect, transform } => {
            renderer.clip_rect(*rect, transform);
        }
        DrawCommand::ClipRRect { rrect, transform } => {
            renderer.clip_rrect(*rrect, transform);
        }
        DrawCommand::ClipPath { path, transform } => {
            renderer.clip_path(path, transform);
        }
        DrawCommand::BackdropFilter {
            child,
            filter,
            bounds,
            blend_mode,
            transform,
        } => {
            renderer.render_backdrop_filter(
                child.as_ref().map(|c| c.as_ref()),
                filter,
                *bounds,
                *blend_mode,
                transform,
            );
        }

        // === Image Extensions ===
        DrawCommand::DrawImageRepeat {
            image,
            dst,
            repeat,
            paint,
            transform,
        } => {
            renderer.render_image_repeat(image, *dst, *repeat, paint.as_ref(), transform);
        }
        DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
            transform,
        } => {
            renderer.render_image_nine_slice(image, *center_slice, *dst, paint.as_ref(), transform);
        }
        DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint,
            transform,
        } => {
            renderer.render_image_filtered(image, *dst, *filter, paint.as_ref(), transform);
        }

        // === Layer Commands ===
        DrawCommand::SaveLayer {
            bounds,
            paint,
            transform,
        } => {
            renderer.save_layer(*bounds, paint, transform);
        }
        DrawCommand::RestoreLayer { transform } => {
            renderer.restore_layer(transform);
        }
    }
}

/// Batch dispatch for multiple commands
///
/// This is a convenience function for rendering entire display lists.
/// It's more efficient than calling dispatch_command in a loop due to
/// better optimization opportunities for the compiler.
///
/// Uses static dispatch via generics for zero-overhead renderer calls.
#[inline]
pub fn dispatch_commands<'a, I, R>(commands: I, renderer: &mut R)
where
    I: IntoIterator<Item = &'a DrawCommand>,
    R: CommandRenderer + ?Sized,
{
    for command in commands {
        dispatch_command(command, renderer);
    }
}
