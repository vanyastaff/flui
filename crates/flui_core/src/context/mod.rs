//! Build context for accessing the element tree
//!
//! This module provides Context, which is passed to widget build methods
//! to provide access to the element tree and framework services.

mod children;
mod core;
mod inherited;
mod iterators;
mod lifecycle;
mod navigation;
mod render;


// Re-export main types
pub use core::{Context, BuildContext};
pub use iterators::Ancestors;

// All impl blocks are in their respective modules


