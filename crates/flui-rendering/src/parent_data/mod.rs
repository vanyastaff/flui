//! The data a parent stores on each child: its offset and whatever its
//! layout reads per child (flex factor, stack insets, sliver slot, table
//! cell).
//!
//! ```text
//! ParentData (trait)
//! ├── BoxParentData                     offset only (Wrap, ListBody, single-child)
//! ├── FlexParentData                    Row, Column
//! ├── StackParentData                   Stack, Positioned
//! ├── MultiChildLayoutParentData        CustomMultiChildLayout
//! ├── TableCellParentData               Table
//! ├── SliverParentData                  slivers in a viewport
//! ├── SliverPhysicalParentData
//! ├── SliverCrossAxisParentData
//! └── SliverMultiBoxAdaptorParentData   lazy lists and grids
//! ```
//!
//! Siblings are the parent's ordered `children` list in the render tree, so
//! no parent data links its neighbours. Every type is `Hash + Eq` so a layout
//! cache can key on it.

mod base;
mod box_parent_data;
mod box_variants;
mod sliver_cross_axis;
mod sliver_parent_data;
mod sliver_variants;
mod table_text;

pub use base::ParentData;
pub use box_parent_data::BoxParentData;
pub use box_variants::{
    FlexFit, FlexParentData, HorizontalInsets, MultiChildLayoutParentData, StackParentData,
};
pub use sliver_cross_axis::{SliverCrossAxisAllocation, SliverCrossAxisParentData};
pub use sliver_parent_data::SliverParentData;
pub use sliver_variants::{SliverMultiBoxAdaptorParentData, SliverPhysicalParentData, SliverSlot};
pub use table_text::{TableCellParentData, TableCellVerticalAlignment};
