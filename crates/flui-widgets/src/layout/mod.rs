//! Multi-child layout widgets.
//!
//! This module contains widgets for laying out multiple children:
//! - Row: Horizontal flex layout
//! - Column: Vertical flex layout
//! - Flex: Base flex layout widget

// Active widgets (using new RenderBox architecture)
pub mod flex;

// Re-exports
pub use flex::{Column, CrossAxisAlignment, Flex, MainAxisAlignment, Row};

// ============================================================================
// DISABLED: Widgets below use old flui_core/flui_objects architecture
// They will be migrated when their RenderObjects are implemented
// ============================================================================

// pub mod baseline;
// pub mod column;  // Replaced by flex::Column
// pub mod expanded;
// pub mod flexible;
// pub mod fractionally_sized_box;
// pub mod indexed_stack;
// pub mod intrinsic_height;
// pub mod intrinsic_width;
// pub mod list_body;
// pub mod overflow_box;
// pub mod positioned;
// pub mod positioned_directional;
// pub mod rotated_box;
// pub mod row;  // Replaced by flex::Row
// pub mod scaffold;
// pub mod scroll_controller;
// pub mod single_child_scroll_view;
// pub mod sized_overflow_box;
// pub mod spacer;
// pub mod stack;
// pub mod wrap;
