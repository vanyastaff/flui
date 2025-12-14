//! Layout render objects for multi-child arrangements.
//!
//! This module provides render objects that arrange multiple children:
//!
//! - **Flex**: Linear arrangement (Row/Column)
//! - **Stack**: Z-axis stacking with positioned children
//! - **Wrap**: Wrapping flow layout
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::objects::r#box::layout::*;
//!
//! let flex = RenderFlex::row();
//! let stack = RenderStack::new();
//! ```

mod flex;
mod stack;
mod wrap;

pub use flex::*;
pub use stack::*;
pub use wrap::*;

// TODO: Additional layout objects
// mod flow;
// mod list_body;
// mod table;
// mod custom_layout;
// mod intrinsic_width;
// mod intrinsic_height;
// mod limited_box;
// mod overflow_box variants;
