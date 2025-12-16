//! Child helpers for View composition.
//!
//! This module provides ergonomic wrappers for working with child Views:
//! - [`Child`] - Single optional child
//! - [`Children`] - Multiple children

mod child;
mod children;

pub use child::Child;
pub use children::Children;
