//! Text rendering utilities shared across all backends
//!
//! This module provides backend-agnostic text rendering capabilities,
//! particularly for complex transformations that require vector-based rendering.

pub mod vector;

pub use vector::{TextVertex, VectorTextError, VectorTextRenderer};
