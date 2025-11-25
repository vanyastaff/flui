//! Visitor patterns for tree-based pipeline operations.
//!
//! These traits define patterns for layout, paint, and hit-test operations
//! that traverse the element tree. They are designed to work with any tree
//! that implements `flui_tree::RenderTreeAccess`.
//!
//! # Design Philosophy
//!
//! - **Abstract patterns**: Generic traits that define how operations flow through the tree
//! - **Callback-based API**: Operations are performed via closures, avoiding type dependencies
//! - **Visitor patterns**: For walking the tree during different pipeline phases
//!
//! # Re-exported from flui-tree
//!
//! These traits and functions are re-exported from `flui-tree` for convenience:
//! - `LayoutVisitable`, `LayoutVisitableExt`
//! - `PaintVisitable`, `PaintVisitableExt`
//! - `HitTestVisitable`, `HitTestVisitableExt`
//! - `TreeVisitor`, `SimpleTreeVisitor`, `TreeOperation`
//! - `layout_with_callback`, `paint_with_callback`, `hit_test_with_callback`

// Re-export visitor traits from flui-tree
pub use flui_tree::{
    // Callback-based operations
    hit_test_with_callback,
    layout_with_callback,
    paint_with_callback,
    // Hit test visitor
    HitTestVisitable,
    HitTestVisitableExt,
    // Layout visitor
    LayoutVisitable,
    LayoutVisitableExt,
    // Paint visitor
    PaintVisitable,
    PaintVisitableExt,
    // Generic visitors (renamed in flui-tree to avoid conflict with visitor module)
    PipelineSimpleVisitor as SimpleTreeVisitor,
    PipelineTreeVisitor as PipelineVisitor,
    TreeOperation,
};

// Re-export tree access traits that are commonly needed with visitors
pub use flui_tree::{DirtyTracking, DirtyTrackingExt, RenderTreeAccess, TreeNav, TreeRead};
