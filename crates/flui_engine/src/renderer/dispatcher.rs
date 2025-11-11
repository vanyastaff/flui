//! Command dispatcher - Visitor pattern implementation
//!
//! This module implements the visitor dispatch logic for DrawCommands.
//! It follows the **Visitor Pattern** to separate data (DrawCommand) from
//! execution logic (CommandRenderer implementations).
//!
//! # Visitor Pattern Architecture
//!
//! ```text
//! DrawCommand (Visitable)
//!     ↓
//! dispatch_command() (Accept)
//!     ↓
//! CommandRenderer (Visitor)
//!     ↓
//! WgpuRenderer / DebugRenderer (Concrete Visitors)
//! ```
//!
//! # Benefits
//!
//! - **Open/Closed Principle**: Add new renderers without modifying DrawCommand
//! - **Single Responsibility**: DrawCommand stores data, renderers handle execution
//! - **Dependency Inversion**: High-level code depends on CommandRenderer abstraction
//! - **Testability**: Easy to swap renderers for testing
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_painting::{DisplayList, DrawCommand};
//! use flui_engine::renderer::{CommandRenderer, dispatch_command};
//!
//! fn render_display_list(display_list: &DisplayList, renderer: &mut dyn CommandRenderer) {
//!     for command in display_list.commands() {
//!         dispatch_command(command, renderer);
//!     }
//! }
//! ```

use super::command_renderer::CommandRenderer;
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
/// # Visitor Pattern Implementation
///
/// Traditional visitor pattern uses an `accept` method on visitable objects:
///
/// ```rust,ignore
/// impl DrawCommand {
///     fn accept(&self, visitor: &mut dyn CommandRenderer) {
///         // dispatch logic here
///     }
/// }
/// ```
///
/// However, we use a free function instead to avoid circular dependencies
/// (flui_painting would need to import flui_engine). This is a valid
/// variant of the visitor pattern called "external visitor" or "functional visitor".
///
/// # Performance
///
/// The match statement compiles to a jump table, making dispatch O(1).
/// No dynamic allocation or vtable lookups beyond the initial trait object call.
#[inline]
pub fn dispatch_command(command: &DrawCommand, renderer: &mut dyn CommandRenderer) {
    match command {
        // === Drawing Commands ===
        DrawCommand::DrawRect { rect, paint, transform } => {
            renderer.render_rect(*rect, paint, transform);
        }
        DrawCommand::DrawRRect { rrect, paint, transform } => {
            renderer.render_rrect(*rrect, paint, transform);
        }
        DrawCommand::DrawCircle { center, radius, paint, transform } => {
            renderer.render_circle(*center, *radius, paint, transform);
        }
        DrawCommand::DrawLine { p1, p2, paint, transform } => {
            renderer.render_line(*p1, *p2, paint, transform);
        }
        DrawCommand::DrawOval { rect, paint, transform } => {
            renderer.render_oval(*rect, paint, transform);
        }
        DrawCommand::DrawPath { path, paint, transform } => {
            renderer.render_path(path, paint, transform);
        }
        DrawCommand::DrawText { text, offset, style, paint, transform } => {
            renderer.render_text(text, *offset, style, paint, transform);
        }
        DrawCommand::DrawImage { image, dst, paint, transform } => {
            renderer.render_image(image, *dst, paint.as_ref(), transform);
        }
        DrawCommand::DrawShadow { path, color, elevation, transform } => {
            renderer.render_shadow(path, *color, *elevation, transform);
        }
        DrawCommand::DrawArc { rect, start_angle, sweep_angle, use_center, paint, transform } => {
            renderer.render_arc(*rect, *start_angle, *sweep_angle, *use_center, paint, transform);
        }
        DrawCommand::DrawDRRect { outer, inner, paint, transform } => {
            renderer.render_drrect(*outer, *inner, paint, transform);
        }
        DrawCommand::DrawPoints { mode, points, paint, transform } => {
            renderer.render_points(*mode, points, paint, transform);
        }
        DrawCommand::DrawVertices { vertices, colors, tex_coords, indices, paint, transform } => {
            renderer.render_vertices(
                vertices,
                colors.as_deref(),
                tex_coords.as_deref(),
                indices,
                paint,
                transform,
            );
        }
        DrawCommand::DrawColor { color, blend_mode, transform } => {
            renderer.render_color(*color, *blend_mode, transform);
        }
        DrawCommand::DrawAtlas { image, sprites, transforms, colors, blend_mode, paint, transform } => {
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
    }
}

/// Batch dispatch for multiple commands
///
/// This is a convenience function for rendering entire display lists.
/// It's more efficient than calling dispatch_command in a loop due to
/// better optimization opportunities for the compiler.
///
/// # Example
///
/// ```rust,ignore
/// use flui_painting::DisplayList;
/// use flui_engine::renderer::{WgpuRenderer, dispatch_commands};
///
/// let display_list = DisplayList::new();
/// let mut renderer = WgpuRenderer::new(painter);
///
/// dispatch_commands(display_list.commands(), &mut renderer);
/// ```
#[inline]
pub fn dispatch_commands<'a, I>(commands: I, renderer: &mut dyn CommandRenderer)
where
    I: IntoIterator<Item = &'a DrawCommand>,
{
    for command in commands {
        dispatch_command(command, renderer);
    }
}
