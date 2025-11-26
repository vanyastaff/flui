//! Platform Providers
//!
//! This module contains platform-related providers that expose
//! window and device information to the widget tree.

pub mod media_query;

// Re-export main types
pub use media_query::{MediaQueryData, MediaQueryProvider};
