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

use flui_painting::DrawCommand;

use crate::traits::CommandRenderer;

/// Dispatch a single DrawCommand to the appropriate CommandRenderer method
///
/// This is the core visitor dispatch function. It performs type-safe
/// double-dispatch: the command type determines which renderer method is
/// called.
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
            wrap_width,
            transform,
        } => {
            renderer.render_text_span(span, *offset, *text_scale_factor, *wrap_width, transform);
        }
        DrawCommand::DrawImage {
            image,
            dst,
            paint,
            transform,
        } => {
            renderer.render_image(image, *dst, paint.as_deref(), transform);
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
        DrawCommand::DrawPaint { paint, transform } => {
            renderer.render_paint(paint, transform);
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
                paint.as_deref(),
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
        DrawCommand::ClipRect {
            rect,
            clip_op,
            clip_behavior,
            transform,
        } => {
            renderer.clip_rect(*rect, *clip_op, *clip_behavior, transform);
        }
        DrawCommand::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
            transform,
        } => {
            renderer.clip_rrect(*rrect, *clip_op, *clip_behavior, transform);
        }
        DrawCommand::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
            transform,
        } => {
            renderer.clip_rsuperellipse(*rsuperellipse, *clip_op, *clip_behavior, transform);
        }
        DrawCommand::ClipPath {
            path,
            clip_op,
            clip_behavior,
            transform,
        } => {
            renderer.clip_path(path, *clip_op, *clip_behavior, transform);
        }
        DrawCommand::BackdropFilter {
            child,
            filter,
            bounds,
            blend_mode,
            transform,
        } => {
            renderer.render_backdrop_filter(
                child.as_ref().map(std::convert::AsRef::as_ref),
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
            renderer.render_image_repeat(image, *dst, *repeat, paint.as_deref(), transform);
        }
        DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
            transform,
        } => {
            renderer.render_image_nine_slice(
                image,
                *center_slice,
                *dst,
                paint.as_deref(),
                transform,
            );
        }
        DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint,
            transform,
        } => {
            renderer.render_image_filtered(image, *dst, *filter, paint.as_deref(), transform);
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
        // `DrawCommand` is `#[non_exhaustive]` so downstream crates
        // (this one) must handle the open-set shape. When a new
        // variant lands in flui-painting before flui-engine grows the
        // matching `render_*` method, fall through with a warn rather
        // than crashing the frame.
        _ => {
            tracing::warn!(
                ?command,
                "dispatch_command: unhandled DrawCommand variant; flui-engine needs an update"
            );
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

#[cfg(test)]
mod tests {
    //! Cycle 5 U10 regression: dispatching an `Arc<Paint>`-carrying
    //! `DrawCommand` reaches the backend identically to the pre-U10
    //! by-value-`Paint` shape. `DebugBackend` only counts commands —
    //! that is enough to prove the dispatch arm executed (rather
    //! than falling through the `_` catch-all) and that no panic was
    //! introduced by the deref shape.
    //!
    //! No GPU is required; this runs on every CI worker.
    use flui_painting::{Canvas, DisplayListCore, Paint};
    use flui_types::{
        geometry::{Rect, px},
        styling::Color,
    };

    use super::dispatch_commands;
    use crate::wgpu::DebugBackend;

    #[test]
    fn dispatch_handles_interned_paint() {
        let mut canvas = Canvas::new();
        let paint = Paint::fill(Color::RED);
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
            &paint,
        );
        canvas.draw_rect(
            Rect::from_ltrb(px(20.0), px(20.0), px(30.0), px(30.0)),
            &paint,
        );
        let dl = canvas.finish();

        let mut backend =
            DebugBackend::new(Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)));
        dispatch_commands(dl.commands(), &mut backend);

        // Two `render_rect` arms must have fired — proves dispatch
        // worked on the new `Arc<Paint>` field shape end-to-end.
        assert_eq!(backend.command_count(), 2);
    }
}
