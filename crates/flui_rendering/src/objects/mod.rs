//! RenderObjects organized by category

/// Basic render objects (primitives, shapes)
pub mod basic;
pub mod debug;
pub mod effects;
pub mod interaction;
pub mod layout;
pub mod media;
pub mod sliver;
pub mod special;
pub mod text;

/// Debug render objects and utilities
/// Effect render objects (opacity, transforms, clips)
/// Interaction render objects (pointer listeners, gesture detection)
/// Layout render objects (flex, padding, align, etc.)
/// Scrollable viewport render object
/// Viewport render object for clipping and scrolling
/// Special render objects (custom paint, etc.)
/// Text rendering objects


pub use effects::*;
pub use interaction::*;
pub use layout::*;
pub use media::*;
pub use sliver::*;
pub use special::*;
pub use text::*;

// Debug objects
pub use debug::{RenderErrorBox, RenderPlaceholder};




