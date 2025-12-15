//! Effects render objects for visual transformations.
//!
//! This module provides render objects that apply visual effects to their children:
//!
//! - **Opacity**: Applies alpha transparency
//! - **Transform**: Applies matrix transformations
//! - **Clipping**: Clips child to various shapes (rect, rrect, oval, path)
//! - **Decoration**: Paints backgrounds and borders
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::objects::r#box::effects::*;
//!
//! let opacity = RenderOpacity::new(0.5);
//! let transform = RenderTransform::new(Matrix4::from_rotation_z(0.5));
//! ```

mod clip_oval;
mod clip_path;
mod clip_rect;
mod clip_rrect;
mod decorated_box;
pub mod fitted_box;
mod fractional_translation;
mod opacity;
mod rotated_box;
mod transform;

pub use clip_oval::*;
pub use clip_path::*;
pub use clip_rect::*;
pub use clip_rrect::*;
pub use decorated_box::*;
pub use fitted_box::*;
pub use fractional_translation::*;
pub use opacity::*;
pub use rotated_box::*;
pub use transform::*;

// Note: RenderBackdropFilter is complex and requires shader support,
// so it's deferred to a later implementation phase.
