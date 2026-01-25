//! Complete ParentData hierarchy - 15+ types matching Flutter's architecture.
//!
//! Comprehensive parent data system supporting all major layout protocols:
//! - Box layouts (Flex, Stack, Flow, Table, etc)
//! - Sliver layouts (List, Grid, Tree, etc)
//! - Text layouts (Rich text with inline spans)
//!
//! # Hierarchy Overview
//!
//! ```text
//! ParentData (trait)
//! │
//! ├── BoxParentData
//! │   └── TableCellParentData
//! │   └── ContainerBoxParentData
//! │       ├── FlexParentData
//! │       ├── StackParentData
//! │       ├── WrapParentData
//! │       ├── FlowParentData
//! │       ├── ListBodyParentData
//! │       ├── ListWheelParentData
//! │       └── MultiChildLayoutParentData
//! │
//! ├── SliverLogicalParentData
//! │   ├── SliverMultiBoxAdaptorParentData
//! │   │   ├── SliverGridParentData
//! │   │   └── TreeSliverNodeParentData
//! │   └── SliverLogicalContainerParentData
//! │
//! ├── SliverPhysicalParentData
//! │   └── SliverPhysicalContainerParentData
//! │
//! └── TextParentData
//! ```
//!
//! # Features
//!
//! All types include:
//! - **Hash + Eq** - For caching layout results
//! - **Builder pattern** - Fluent construction
//! - **Utility methods** - Common operations
//! - **Type safety** - Downcasting support
//! - **Comprehensive tests** - Full coverage
//!
//! # Usage
//!
//! ```ignore
//! use flui_rendering::parent_data::prelude::*;
//!
//! // Flex layout (Row/Column)
//! let flex_data = FlexParentData::flexible(2)
//!     .with_fit(FlexFit::Tight);
//!
//! // Stack layout
//! let stack_data = StackParentData::new()
//!     .with_top(10.0)
//!     .with_left(20.0);
//!
//! // Sliver grid
//! let grid_data = SliverGridParentData::new(5, 100.0)
//!     .with_layout_offset(500.0);
//!
//! // Table cell
//! let cell_data = TableCellParentData::zero()
//!     .at_cell(2, 3)
//!     .with_alignment(TableCellVerticalAlignment::Middle);
//! ```

mod base;
mod box_parent_data;
mod sliver_parent_data;

// Mixins
mod container_mixin;
mod keep_alive_mixin;

// Variants
mod box_variants;
mod sliver_variants;
mod table_text;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Base
pub use base::ParentData;

// Core types
pub use box_parent_data::BoxParentData;
pub use sliver_parent_data::SliverParentData;

// Mixins
pub use container_mixin::ContainerParentDataMixin;
pub use keep_alive_mixin::KeepAliveParentDataMixin;

// Box variants
pub use box_variants::{
    ContainerBoxParentData, FlexFit, FlexParentData, FlowParentData, ListBodyParentData,
    ListWheelParentData, MultiChildLayoutParentData, StackParentData, WrapParentData,
};

// Sliver variants
pub use sliver_variants::{
    SliverGridParentData, SliverLogicalContainerParentData, SliverLogicalParentData,
    SliverMultiBoxAdaptorParentData, SliverPhysicalContainerParentData, SliverPhysicalParentData,
    TreeSliverNodeParentData,
};

// Table and text
pub use table_text::{TableCellParentData, TableCellVerticalAlignment, TextParentData, TextRange};

// ============================================================================
// TYPE COUNTS
// ============================================================================

/// Total number of parent data types in hierarchy.
pub const PARENT_DATA_TYPE_COUNT: usize = 18;

/// Core parent data types (non-container).
pub const CORE_TYPES: usize = 4;

/// Container parent data types (with sibling pointers).
pub const CONTAINER_TYPES: usize = 14;

// ============================================================================
// PRELUDE
// ============================================================================

/// Convenient imports for parent data system.
///
/// ```ignore
/// use flui_rendering::parent_data::prelude::*;
/// ```
pub mod prelude {
    // Base
    pub use super::ParentData;

    // Core types
    pub use super::{BoxParentData, SliverParentData};

    // Mixins
    pub use super::{ContainerParentDataMixin, KeepAliveParentDataMixin};

    // Box variants
    pub use super::{
        ContainerBoxParentData, FlexFit, FlexParentData, FlowParentData, ListBodyParentData,
        ListWheelParentData, MultiChildLayoutParentData, StackParentData, WrapParentData,
    };

    // Sliver variants
    pub use super::{
        SliverGridParentData, SliverLogicalContainerParentData, SliverLogicalParentData,
        SliverMultiBoxAdaptorParentData, SliverPhysicalContainerParentData,
        SliverPhysicalParentData, TreeSliverNodeParentData,
    };

    // Table and text
    pub use super::{TableCellParentData, TableCellVerticalAlignment, TextParentData, TextRange};
}

// ============================================================================
// TYPE CATEGORIZATION
// ============================================================================

/// Box protocol parent data types.
pub mod r#box {
    pub use super::{
        BoxParentData, ContainerBoxParentData, FlexFit, FlexParentData, FlowParentData,
        ListBodyParentData, ListWheelParentData, MultiChildLayoutParentData, StackParentData,
        TableCellParentData, TableCellVerticalAlignment, WrapParentData,
    };
}

/// Sliver protocol parent data types.
pub mod sliver {
    pub use super::{
        SliverGridParentData, SliverLogicalContainerParentData, SliverLogicalParentData,
        SliverMultiBoxAdaptorParentData, SliverParentData, SliverPhysicalContainerParentData,
        SliverPhysicalParentData, TreeSliverNodeParentData,
    };
}

/// Text protocol parent data types.
pub mod text {
    pub use super::{TextParentData, TextRange};
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Get total number of parent data types.
pub const fn type_count() -> usize {
    PARENT_DATA_TYPE_COUNT
}

/// Check if type supports container operations (sibling pointers).
pub fn is_container_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "ContainerBoxParentData"
            | "FlexParentData"
            | "StackParentData"
            | "WrapParentData"
            | "FlowParentData"
            | "ListBodyParentData"
            | "ListWheelParentData"
            | "MultiChildLayoutParentData"
            | "SliverLogicalContainerParentData"
            | "SliverPhysicalContainerParentData"
            | "TextParentData"
    )
}

// ============================================================================
// DOCUMENTATION HELPERS
// ============================================================================

/// Get description of parent data type usage.
pub fn type_usage(type_name: &str) -> &'static str {
    match type_name {
        "BoxParentData" => "Basic 2D positioning (offset only)",
        "ContainerBoxParentData" => "Container base with sibling pointers",
        "FlexParentData" => "Flex layouts (Row/Column) with flex factors",
        "StackParentData" => "Absolute positioning with top/right/bottom/left",
        "WrapParentData" => "Wrapping layouts (horizontal/vertical)",
        "FlowParentData" => "Custom flow layouts with transforms",
        "ListBodyParentData" => "List body layouts",
        "ListWheelParentData" => "3D carousel/wheel layouts with index",
        "MultiChildLayoutParentData" => "Custom multi-child layouts with IDs",
        "TableCellParentData" => "Table cells with row/column position",
        "TextParentData" => "Rich text with inline spans",
        "SliverLogicalParentData" => "Sliver logical positioning",
        "SliverMultiBoxAdaptorParentData" => "Sliver lists with keep-alive",
        "SliverGridParentData" => "Sliver grids with cross-axis offset",
        "TreeSliverNodeParentData" => "Tree views with depth tracking",
        "SliverLogicalContainerParentData" => "Sliver container (logical)",
        "SliverPhysicalParentData" => "Sliver physical paint offset",
        "SliverPhysicalContainerParentData" => "Sliver container (physical)",
        _ => "Unknown type",
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_count() {
        assert_eq!(type_count(), 18);
    }

    #[test]
    fn test_is_container_type() {
        assert!(is_container_type("ContainerBoxParentData"));
        assert!(is_container_type("FlexParentData"));
        assert!(is_container_type("TextParentData"));
        assert!(!is_container_type("BoxParentData"));
        assert!(!is_container_type("SliverLogicalParentData"));
    }

    #[test]
    fn test_type_usage() {
        let usage = type_usage("FlexParentData");
        assert!(usage.contains("flex"));
    }

    #[test]
    fn test_all_types_importable() {
        // Ensure all types can be imported
        let _box = BoxParentData::default();
        let _sliver = SliverParentData::default();
        let _flex = FlexParentData::default();
        let _stack = StackParentData::default();
        let _grid = SliverGridParentData::default();
        let _table = TableCellParentData::default();
        let _text = TextParentData::default();
    }

    #[test]
    fn test_prelude() {
        use prelude::*;

        // Should be able to use all types from prelude
        let _ = BoxParentData::default();
        let _ = FlexParentData::default();
        let _ = SliverGridParentData::default();
    }
}
