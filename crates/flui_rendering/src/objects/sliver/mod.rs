//! Sliver protocol render objects (scrollable content layout).
//!
//! Sliver render objects use `SliverConstraints` and `SliverGeometry` for layout,
//! providing efficient rendering of scrollable content.
//!
//! # Categories
//!
//! - [`basic`]: Simple sliver modifications and adapters
//! - [`layout`]: Multi-child sliver layouts (List, Grid, etc.)
//! - [`effects`]: Visual effects for slivers

pub mod basic;
pub mod effects;
pub mod layout;
