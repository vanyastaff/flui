//! Visual effects widgets
//!
//! This module contains widgets that apply visual effects to their children:
//! - Opacity: Transparency effects
//! - Transform: Matrix transformations
//! - ClipRRect: Rounded rectangle clipping
//! - And more...

pub mod clip_oval;
pub mod clip_rect;
pub mod clip_rrect;
pub mod offstage;
pub mod opacity;
pub mod transform;




// Re-exports
pub use clip_oval::ClipOval;
pub use clip_rect::ClipRect;
pub use clip_rrect::ClipRRect;
pub use offstage::Offstage;
pub use opacity::Opacity;
pub use transform::Transform;






