//! RenderObjects organized by category

pub mod effects;
pub mod interaction;
pub mod layout;
/// Special-purpose render objects (semantics, metadata, fitted box, colored box)
pub mod special;


// Re-exports for convenience
pub use layout::*;
pub use effects::*;
pub use interaction::*;
pub use special::*;

