//! RenderObjects organized by category

/// Basic render objects (primitives, shapes)
pub mod basic;
/// Debug render objects and utilities
pub mod debug;
/// Effect render objects (opacity, transforms, clips)
pub mod effects;
/// Interaction render objects (pointer listeners, gesture detection)
pub mod interaction;
/// Layout render objects (flex, padding, align, etc.)
pub mod layout;
/// Scrollable viewport render object
pub mod render_scroll_view;
/// Viewport render object for clipping and scrolling
pub mod render_viewport;
/// Special render objects (custom paint, etc.)
pub mod special;
/// Text rendering objects
pub mod text;

pub use effects::*;
pub use interaction::*;
pub use layout::*;
pub use render_scroll_view::RenderScrollView;
pub use render_viewport::RenderViewport;
pub use special::*;
pub use text::*;

// Debug objects - currently TODO, will be re-enabled after Canvas API migration
// #[cfg(debug_assertions)]
// pub use debug::*;
