//! `DrawCommand` dispatch.
//!
//! The match that routes each `flui_painting::DrawOp` variant to its
//! [`CommandRenderer`](crate::command_renderer::CommandRenderer) method — the
//! two are read together, since the enum and the trait are the two halves of
//! one contract.
//!
//! ```text
//! DrawCommand { transform, op } (flui_painting)
//!     │
//!     ▼
//! dispatch_command() ─────► CommandRenderer::render_*(…, transform)
//! ```

use flui_painting::{DrawCommand, DrawOp};

use crate::command_renderer::CommandRenderer;

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
pub(crate) fn dispatch_command<R: CommandRenderer + ?Sized>(
    command: &DrawCommand,
    renderer: &mut R,
) {
    let transform = &command.transform;
    match &command.op {
        // === Drawing Commands ===
        DrawOp::Rect { rect, paint } => {
            renderer.render_rect(*rect, paint, transform);
        }
        DrawOp::RRect { rrect, paint } => {
            renderer.render_rrect(*rrect, paint, transform);
        }
        DrawOp::Circle {
            center,
            radius,
            paint,
        } => {
            renderer.render_circle(*center, radius.0, paint, transform);
        }
        DrawOp::Line { p1, p2, paint } => {
            renderer.render_line(*p1, *p2, paint, transform);
        }
        DrawOp::Oval { rect, paint } => {
            renderer.render_oval(*rect, paint, transform);
        }
        DrawOp::Path { path, paint } => {
            renderer.render_path(path, paint, transform);
        }
        DrawOp::Paragraph {
            layout,
            offset,
            color,
        } => {
            renderer.render_paragraph(layout, *offset, *color, transform);
        }
        DrawOp::Image { image, dst, paint } => {
            renderer.render_image(image, *dst, paint.as_deref(), transform);
        }
        DrawOp::Texture {
            texture_id,
            dst,
            src,
            filter_quality,
            opacity,
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
        DrawOp::Shadow {
            path,
            color,
            elevation,
        } => {
            renderer.render_shadow(path, *color, *elevation, transform);
        }
        DrawOp::Arc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
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
        DrawOp::DRRect {
            outer,
            inner,
            paint,
        } => {
            renderer.render_drrect(*outer, *inner, paint, transform);
        }
        DrawOp::Points {
            mode,
            points,
            paint,
        } => {
            renderer.render_points(*mode, points, paint, transform);
        }
        DrawOp::Vertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
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
        DrawOp::Color { color, blend_mode } => {
            renderer.render_color(*color, *blend_mode, transform);
        }
        DrawOp::Paint { paint } => {
            renderer.render_paint(paint, transform);
        }
        DrawOp::Atlas {
            image,
            sprites,
            transforms,
            colors,
            blend_mode,
            paint,
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

        // === Clipping Commands ===
        DrawOp::ClipRect {
            rect,
            clip_op,
            clip_behavior,
        } => {
            renderer.clip_rect(*rect, *clip_op, *clip_behavior, transform);
        }
        DrawOp::ClipRRect {
            rrect,
            clip_op,
            clip_behavior,
        } => {
            renderer.clip_rrect(*rrect, *clip_op, *clip_behavior, transform);
        }
        DrawOp::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            clip_behavior,
        } => {
            renderer.clip_rsuperellipse(*rsuperellipse, *clip_op, *clip_behavior, transform);
        }
        DrawOp::ClipPath {
            path,
            clip_op,
            clip_behavior,
        } => {
            renderer.clip_path(path, *clip_op, *clip_behavior, transform);
        }
        // === Image Extensions ===
        DrawOp::ImageRepeat {
            image,
            dst,
            repeat,
            paint,
        } => {
            renderer.render_image_repeat(image, *dst, *repeat, paint.as_deref(), transform);
        }
        DrawOp::ImageNineSlice {
            image,
            center_slice,
            dst,
            paint,
        } => {
            renderer.render_image_nine_slice(
                image,
                *center_slice,
                *dst,
                paint.as_deref(),
                transform,
            );
        }
        DrawOp::ImageFiltered {
            image,
            dst,
            filter,
            paint,
        } => {
            renderer.render_image_filtered(image, *dst, *filter, paint.as_deref(), transform);
        }

        // === Layer Commands ===
        DrawOp::SaveLayer { bounds, paint } => {
            renderer.save_layer(*bounds, paint, transform);
        }
        DrawOp::RestoreLayer => {
            renderer.restore_layer(transform);
        }
        DrawOp::Save => {
            renderer.save_state();
        }
        DrawOp::Restore => {
            renderer.restore_state();
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
pub(crate) fn dispatch_commands<'a, I, R>(commands: I, renderer: &mut R)
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
    //! Regression guard: dispatching an `Arc<Paint>`-carrying
    //! `DrawCommand` reaches the backend identically to the earlier
    //! by-value-`Paint` shape. `DebugBackend` only counts commands —
    //! that is enough to prove the dispatch arm executed (rather
    //! than falling through the `_` catch-all) and that no panic was
    //! introduced by the deref shape.
    //!
    //! No GPU is required; this runs on every CI worker.
    use flui_painting::{Canvas, Paint};
    use flui_types::{
        geometry::{Rect, px},
        styling::Color,
    };

    use super::dispatch_commands;
    use crate::debug::DebugBackend;

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

        let mut backend = DebugBackend::new();
        dispatch_commands(dl.commands(), &mut backend);

        // Two `render_rect` arms must have fired — proves dispatch
        // worked on the new `Arc<Paint>` field shape end-to-end.
        assert_eq!(backend.command_count(), 2);
    }
}
