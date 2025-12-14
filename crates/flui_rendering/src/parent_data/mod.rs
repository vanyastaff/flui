//! Parent data types for storing child metadata.
//!
//! Parent data is metadata that a parent render object stores on each
//! of its children. This typically includes positioning information
//! (like offsets) and layout parameters (like flex factors).
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `ParentData` class and its subclasses:
//! - `ParentData` → [`ParentData`] trait
//! - `BoxParentData` → [`BoxParentData`]
//! - `SliverLogicalParentData` → [`SliverParentData`]
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::parent_data::{ParentData, BoxParentData};
//!
//! let mut data = BoxParentData::default();
//! data.offset = Offset::new(10.0, 20.0);
//! ```

mod base;
mod box_parent_data;
mod sliver_parent_data;

pub use base::ParentData;
pub use box_parent_data::*;
pub use sliver_parent_data::*;
