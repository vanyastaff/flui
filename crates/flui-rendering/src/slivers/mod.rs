//! Sliver infrastructure: data structures and algorithms for lazy
//! scrollable content.
//!
//! This module provides the building blocks for efficient lazy
//! lists, grids, and other scrollable content:
//!
//! - [`FenwickExtents`]: O(log n) offset↔index mapping for variable-size items

pub mod fenwick;

pub use fenwick::FenwickExtents;
