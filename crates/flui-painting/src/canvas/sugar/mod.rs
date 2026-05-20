//! Canvas syntactic sugar: caller-side ergonomic wrappers over the
//! primary `draw_*` API.
//!
//! Mythos chain U4 originally extracted these from the 3,305-LOC
//! `canvas.rs` god module into a single 674-LOC `sugar.rs`. The
//! code-review fixup pass 2 split that file further into six
//! concern-based submodules (~50–360 LOC each):
//!
//! - [`batch`]       -- `draw_rects` / `draw_circles` / `draw_lines` /
//!   `draw_rrects` / `draw_paths`: loop wrappers over the primary
//!   single-shape methods.
//! - [`conditional`] -- `draw_rect_if` / `draw_circle_if` / `draw_if`
//!   / `draw_unless` / `draw_if_some`: avoid verbose `if` statements.
//! - [`grid`]        -- `draw_grid` / `repeat_x` / `repeat_y` /
//!   `repeat_radial`: recurrent layouts built on `with_translate`.
//! - [`debug`]       -- `debug_rect` / `debug_point` / `debug_axes` /
//!   `debug_grid`: diagnostic drawing helpers.
//! - [`shapes`]      -- `draw_rounded_rect` / `draw_rounded_rect_corners`
//!   / `draw_pill` / `draw_ring`: common compound shapes assembled
//!   from primary primitives.
//! - [`chain`]       -- ~30 fluent methods returning `&mut Self` plus
//!   the closure combinators (`also`, `when`, `when_else`).
//!
//! None of these methods emit `DrawCommand` variants directly; they
//! all delegate to the primary methods in [`super::drawing`],
//! [`super::transform`], [`super::clipping`], [`super::state`], or
//! [`super::scoped`].

pub mod batch;
pub mod chain;
pub mod conditional;
pub mod debug;
pub mod grid;
pub mod shapes;
