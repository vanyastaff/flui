//! Text rendering subsystem
//!
//! Manages font loading, glyph rasterization, and text layout
//! for GPU-accelerated text rendering.

pub mod cache;
pub mod system;

pub use cache::TextCacheKey;
pub use system::TextSystem;
