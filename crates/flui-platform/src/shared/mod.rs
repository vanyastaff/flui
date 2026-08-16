//! Shared platform infrastructure
//!
//! Components shared between platform implementations to reduce code
//! duplication.

mod handlers;
pub mod scroll;

pub use handlers::{PlatformHandlers, WindowCallbacks};
