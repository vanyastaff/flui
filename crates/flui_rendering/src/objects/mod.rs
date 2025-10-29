//! RenderObjects organized by category

pub mod effects;
pub mod interaction;
pub mod layout;
pub mod special;
pub mod text;
pub use effects::*;
pub use interaction::*;
/// Special-purpose render objects (semantics, metadata, fitted box, colored box)
// Re-exports for convenience
pub use layout::*;
pub use special::*;
pub use text::*;
