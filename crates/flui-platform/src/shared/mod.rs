//! Shared platform infrastructure
//!
//! Components shared between platform implementations to reduce code
//! duplication.

pub mod gestures;
mod handlers;
pub mod keys;
pub mod keys_macos;
pub mod scroll;

pub use handlers::{PlatformHandlers, WindowCallbacks};
