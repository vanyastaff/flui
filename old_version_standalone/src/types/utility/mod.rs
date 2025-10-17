//! Utility types.
//!
//! This module contains miscellaneous utility types:
//! - [`TargetPlatform`]: Platform detection (Android, iOS, Web, etc.)
//! - [`PlatformBrightness`]: Light or dark mode
//!
//! Note: [`Duration`] is in [`crate::types::core`] as it is a core primitive.

pub mod platform;

// Re-export types for convenience
pub use platform::{PlatformBrightness, TargetPlatform};



