//! Core rendering types and traits for FLUI with advanced flui-tree integration.
//!
//! This module provides the foundational rendering architecture that leverages the
//! unified arity system and advanced type features from `flui-tree` for maximum
//! performance, type safety, and ergonomics.
//!
//! # Architecture Overview
//!
//! ```text
//! flui-tree (unified abstractions)
//!     │
//!     ├── Arity system (GAT + HRTB + const generics)
//!     ├── Tree traits (TreeNav, TreeRead, DirtyTracking)
//!     └── RenderTreeAccess (type-erased render data)
//!     │
//!     ▼
//! flui_rendering::core (this module)
//!     │
//!     ├── RenderObject (base trait for all render objects)
//!     ├── RenderBox<A> (box protocol with arity A)
//!     ├── RenderSliver<A> (sliver protocol with arity A)
//!     ├── Contexts (GAT-based layout/paint/hit-test contexts)
//!     ├── TreeOps (dyn-compatible tree operations)
//!     └── Wrappers (ergonomic proxy and utility wrappers)
//! ```
//!
//! # Design Principles
//!
//! 1. **Zero-cost abstractions** - GAT and const generics for compile-time optimization
//! 2. **Type safety** - Arity system prevents child count errors at compile time
//! 3. **Performance** - Atomic dirty tracking and efficient tree operations
//! 4. **Ergonomics** - Context-based API with convenient methods
//! 5. **Flexibility** - Support for both GAT-based and dyn-compatible operations
//!
//! # Key Types
//!
//! ## Core Traits
//!
//! - [`RenderObject`] - Base trait for all render objects with type erasure support
//! - [`RenderBox<A>`] - Box protocol render objects with arity validation
//! - [`RenderSliver<A>`] - Sliver protocol render objects with arity validation
//!
//! ## Contexts (GAT-based)
//!
//! - [`LayoutContext<A, P>`] - Layout operations with typed children access
//! - [`PaintContext<A, P>`] - Paint operations with canvas management
//! - [`HitTestContext<A, P>`] - Hit testing with efficient algorithms
//!
//! ## Tree Operations (dyn-compatible)
//!
//! - [`LayoutTree`] - Layout operations for type erasure scenarios
//! - [`PaintTree`] - Paint operations for type erasure scenarios
//! - [`RenderTreeOps`] - Combined render tree operations
//!
//! ## Arity System
//!
//! Re-exports from `flui-tree` with rendering-specific extensions:
//!
//! - [`Leaf`] - 0 children (Text, Image, Spacer)
//! - [`Optional`] - 0-1 children (Container, SizedBox)
//! - [`Single`] - 1 child (Padding, Transform, Align)
//! - [`Variable`] - Any number (Flex, Stack, Column)
//! - [`Exact<N>`] - Exactly N children (custom layouts)
//!
//! # Usage Examples
//!
//! ## Simple Render Object
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, LayoutContext, PaintContext, Leaf, BoxProtocol};
//!
//! #[derive(Debug)]
//! struct RenderColoredBox {
//!     color: Color,
//!     size: Size,
//! }
//!
//! impl RenderBox<Leaf> for RenderColoredBox {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
//!         // Leaf elements have no children to layout
//!         let size = ctx.constraints.constrain(self.size);
//!         self.size = size;
//!         size
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Leaf, BoxProtocol>) {
//!         let paint = Paint::new().with_color(self.color);
//!         ctx.canvas_mut().draw_rect(Rect::from_size(self.size), &paint);
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//!
//! impl RenderObject for RenderColoredBox {
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//! }
//! ```
//!
//! ## Container with Single Child
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, LayoutContext, PaintContext, Single, BoxProtocol};
//!
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
//!         let inner_constraints = ctx.constraints.deflate(&self.padding);
//!         let child_id = ctx.single_child();
//!         let child_size = ctx.layout_child(child_id, inner_constraints)?;
//!
//!         Size::new(
//!             child_size.width + self.padding.horizontal(),
//!             child_size.height + self.padding.vertical(),
//!         )
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Single, BoxProtocol>) {
//!         let child_id = ctx.single_child();
//!         let offset = Offset::new(self.padding.left, self.padding.top);
//!         ctx.paint_child(child_id, ctx.offset + offset)?;
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! ## Multi-child Layout
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, LayoutContext, PaintContext, Variable, BoxProtocol};
//!
//! #[derive(Debug)]
//! struct RenderFlex {
//!     direction: Axis,
//!     main_axis_alignment: MainAxisAlignment,
//! }
//!
//! impl RenderBox<Variable> for RenderFlex {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
//!         let mut total_main_size = 0.0;
//!         let mut max_cross_size = 0.0;
//!
//!         // Layout all children
//!         for child_id in ctx.children() {
//!             let child_size = ctx.layout_child(child_id, ctx.constraints)?;
//!             let (main_size, cross_size) = match self.direction {
//!                 Axis::Horizontal => (child_size.width, child_size.height),
//!                 Axis::Vertical => (child_size.height, child_size.width),
//!             };
//!             total_main_size += main_size;
//!             max_cross_size = max_cross_size.max(cross_size);
//!         }
//!
//!         match self.direction {
//!             Axis::Horizontal => Size::new(total_main_size, max_cross_size),
//!             Axis::Vertical => Size::new(max_cross_size, total_main_size),
//!         }
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Variable, BoxProtocol>) {
//!         let mut main_offset = 0.0;
//!
//!         for child_id in ctx.children() {
//!             let offset = match self.direction {
//!                 Axis::Horizontal => Offset::new(main_offset, 0.0),
//!                 Axis::Vertical => Offset::new(0.0, main_offset),
//!             };
//!             ctx.paint_child(child_id, ctx.offset + offset)?;
//!
//!             if let Some(child_size) = ctx.get_child_size(child_id) {
//!                 main_offset += match self.direction {
//!                     Axis::Horizontal => child_size.width,
//!                     Axis::Vertical => child_size.height,
//!                 };
//!             }
//!         }
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! # Performance Characteristics
//!
//! - **Layout**: O(n) where n is number of children (parallelizable)
//! - **Paint**: O(n) with layer composition optimization
//! - **Hit Testing**: O(log n) with spatial indexing for large trees
//! - **Memory**: Zero-cost arity abstractions, atomic dirty flags
//!
//! # Thread Safety
//!
//! All render objects must be `Send + Sync`. The dirty tracking system uses
//! atomic operations for thread-safe state management.

// ============================================================================
// CORE MODULES
// ============================================================================

// Arity system re-exports and rendering extensions
mod arity;

// Rendering contexts with GAT integration
mod contexts;

// Geometry and constraint types
mod geometry;

// Protocol definitions (Box, Sliver)
mod protocol;

// Parent data system for per-child layout metadata
mod parent_data;

// Base render object trait
mod render_object;

// Atomic render flags for lock-free state management
mod render_flags;

// Per-render state storage with atomic flags
mod render_state;

// Box protocol render trait
mod render_box;

// Sliver protocol render trait
mod render_sliver;

// Full render tree traits (combines LayoutTree + PaintTree + HitTestTree)
// Note: tree_ops.rs was removed - all tree traits are now in render_tree.rs
mod render_tree;

// Proxy traits for pass-through render objects
mod render_proxy;

// Utility wrappers and proxies
mod wrappers;

// ============================================================================
// ARITY SYSTEM RE-EXPORTS
// ============================================================================

pub use arity::{
    // Access pattern hints
    AccessPattern,
    // Core arity trait
    Arity,
    // Arity markers with const generic support
    AtLeast,
    // GAT-based accessor trait
    ChildrenAccess,
    Exact,
    // Concrete accessor types
    ExactChildren,
    // Fixed children accessor
    FixedChildren,
    Leaf,
    LeafChildren,
    // No children accessor
    NoChildren,
    Optional,
    // Optional child accessor
    OptionalChild,
    OptionalChildren,
    // Range arity for bounded children
    Range,
    // Runtime arity for dynamic dispatch
    RuntimeArity,
    Single,
    SingleChildren,
    // Slice children accessor
    SliceChildren,
    Variable,
    VariableChildren,
};

// ============================================================================
// PROTOCOL SYSTEM
// ============================================================================

pub use protocol::{
    BoxProtocol,
    // Protocol-specific constraint types
    LayoutProtocol, // Type alias for ProtocolId
    // Protocol trait and implementations
    Protocol,
    SliverProtocol,
};

// ============================================================================
// GEOMETRY AND CONSTRAINTS
// ============================================================================

pub use geometry::{
    BoxConstraints,
    // Unified constraint types
    Constraints,
    // Geometry helpers
    Geometry,
};

// Re-export flui_types for convenience
pub use flui_types::prelude::{CrossAxisAlignment, MainAxisAlignment};
pub use flui_types::{Axis, EdgeInsets, Offset, Rect, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// CORE RENDER TRAITS
// ============================================================================

pub use render_box::{RenderBox, RenderBoxExt};
pub use render_object::RenderObject;
pub use render_sliver::{RenderSliver, RenderSliverExt};

// Re-export old names for backward compatibility
// These are just re-exports, not deprecated aliases, to ensure impl blocks work
pub use render_sliver::RenderSliver as SliverRender;
pub use render_sliver::RenderSliverExt as SliverRenderExt;

// ============================================================================
// PARENT DATA SYSTEM
// ============================================================================

pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

// ============================================================================
// RENDER FLAGS AND STATE
// ============================================================================

pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_state::RenderState;

// ============================================================================
// CONTEXTS (GAT-based)
// ============================================================================

pub use contexts::{
    BoxHitTestContext,
    BoxLayoutContext,
    BoxPaintContext,
    // Hit test contexts
    HitTestContext,
    // Layout contexts
    LayoutContext,
    // Paint contexts
    PaintContext,
    SliverHitTestContext,
    SliverLayoutContext,
    SliverPaintContext,
};

// Type aliases for backward compatibility (shorter names)
/// Alias for `BoxLayoutContext` (backward compatibility)
pub type BoxLayoutCtx<'a, A> = BoxLayoutContext<'a, A>;

/// Alias for `BoxPaintContext` (backward compatibility)
pub type BoxPaintCtx<'a, A> = BoxPaintContext<'a, A>;

// ============================================================================
// TREE OPERATIONS (dyn-compatible)
// ============================================================================

pub use render_tree::{
    // Utility functions
    hit_test_subtree,
    layout_batch,
    layout_subtree,
    paint_batch,
    paint_subtree,
    // Combined traits
    FullRenderTree,
    // Phase-specific traits
    HitTestTree,
    // Extension traits
    HitTestTreeExt,
    LayoutTree,
    LayoutTreeExt,
    PaintTree,
    PaintTreeExt,
    RenderTreeOps,
};

// ============================================================================
// PROXY TRAITS
// ============================================================================

pub use render_proxy::{SimpleBoxProxy, SimpleSliverProxy};

// Re-export old names for backward compatibility
pub use render_proxy::SimpleBoxProxy as RenderBoxProxy;
pub use render_proxy::SimpleSliverProxy as RenderSliverProxy;

// ============================================================================
// WRAPPERS AND UTILITIES
// ============================================================================

pub use wrappers::{
    BoxRenderWrapper,
    // Utility traits
    ProxyRender,
    // Proxy wrappers for single-child render objects
    RenderProxy,
    // Generic wrappers for common patterns
    RenderWrapper,
    SingleChildProxy,
    SliverRenderWrapper,
    WrapperRender,
};

// ============================================================================
// FLUI-TREE INTEGRATION
// ============================================================================

// Re-export commonly used flui-tree types for convenience
pub use flui_tree::{
    // Utility functions
    collect_render_children,
    count_render_children,
    find_render_ancestor,
    render_depth,
    AtomicDirtyFlags,
    // Dirty tracking
    DirtyTracking,
    DirtyTrackingExt,
    RenderAncestors,
    // Iterators
    RenderChildren,
    RenderDescendants,
    // Render tree access
    RenderTreeAccess,
    RenderTreeAccessExt,
    // Tree navigation
    TreeNav,
    TreeRead,
    TreeWrite,
};

// ============================================================================
// FOUNDATION RE-EXPORTS
// ============================================================================

pub use flui_foundation::ElementId;
pub use flui_interaction::{HitTestBehavior, HitTestResult, HitTestable};
pub use flui_painting::{Canvas, Paint};

// ============================================================================
// ERROR HANDLING
// ============================================================================

// Re-export error types and result alias
pub use crate::error::{RenderError, Result as RenderResult};

// ============================================================================
// PRELUDE FOR COMMON USAGE
// ============================================================================

/// The rendering prelude - commonly used types and traits.
///
/// ```rust
/// use flui_rendering::core::prelude::*;
/// ```
pub mod prelude {
    // Core traits
    pub use super::{RenderBox, RenderObject, RenderSliver};

    // Arity system
    pub use super::{Arity, AtLeast, Exact, Leaf, Optional, Single, Variable};

    // Protocols
    pub use super::{BoxProtocol, Protocol, SliverProtocol};

    // Contexts
    pub use super::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
    pub use super::{HitTestContext, LayoutContext, PaintContext};

    // Geometry
    pub use super::{BoxConstraints, Offset, Rect, Size};

    // Tree operations
    pub use super::{HitTestTree, LayoutTree, PaintTree, RenderTreeOps};

    // Foundation types
    pub use super::{Canvas, ElementId, HitTestResult, Paint};

    // Error handling
    pub use super::{RenderError, RenderResult};
}

// ============================================================================
// FEATURE FLAGS AND INFORMATION
// ============================================================================

/// Returns information about enabled rendering features.
pub fn rendering_features() -> &'static str {
    concat!(
        "GAT contexts, ",
        "unified arity system, ",
        "atomic dirty tracking, ",
        "zero-cost abstractions, ",
        "const generic optimization"
    )
}

/// Returns performance characteristics of the rendering system.
pub fn performance_info() -> &'static str {
    concat!(
        "Zero-cost arity validation, ",
        "GAT-based contexts with compile-time optimization, ",
        "atomic dirty flags for thread-safe updates, ",
        "efficient tree traversal with spatial indexing"
    )
}

/// Returns the version of the core rendering system.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// DOCUMENTATION TESTS
// ============================================================================

#[cfg(doctest)]
mod doctests {
    //! This module exists solely to run doctests in the module-level documentation.
    //! The actual tests are embedded in the doc comments above.
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_detection() {
        let features = rendering_features();
        assert!(features.contains("GAT contexts"));
        assert!(features.contains("unified arity"));
        assert!(features.contains("zero-cost"));

        let perf_info = performance_info();
        assert!(perf_info.contains("Zero-cost"));
        assert!(perf_info.contains("atomic"));
    }

    #[test]
    fn test_version_info() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_prelude_exports() {
        use prelude::*;

        // Test that key types are available
        let _: Option<ElementId> = None;
        let _: Size = Size::new(100.0, 100.0);
        let _: BoxConstraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    }

    #[test]
    fn test_arity_system_integration() {
        // Test arity validation at compile time
        assert!(Leaf::validate_count(0));
        assert!(!Leaf::validate_count(1));

        assert!(Single::validate_count(1));
        assert!(!Single::validate_count(0));
        assert!(!Single::validate_count(2));

        assert!(Variable::validate_count(0));
        assert!(Variable::validate_count(100));
    }

    #[test]
    fn test_flui_tree_integration() {
        // Test that flui-tree re-exports work
        let flags = AtomicDirtyFlags::new();
        assert!(!flags.needs_layout());
        assert!(!flags.needs_paint());
    }

    /// Comprehensive test demonstrating the new three-tree architecture
    #[test]
    fn test_new_architecture_integration() {
        use std::any::Any;

        // Test GAT-based context creation
        let element_id = ElementId::new(42);
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Test arity accessors work with ElementId slices
        let leaf_children: [ElementId; 0] = [];
        let leaf_accessor = Leaf::from_slice(&leaf_children);
        assert_eq!(leaf_accessor.len(), 0);

        let single_children = [element_id];
        let single_accessor = Single::from_slice(&single_children);
        assert_eq!(single_accessor.len(), 1);
        assert_eq!(single_accessor.single_child(), Some(element_id));

        let variable_children = [element_id, ElementId::new(43), ElementId::new(44)];
        let variable_accessor = Variable::from_slice(&variable_children);
        assert_eq!(variable_accessor.len(), 3);

        // Test render children extensions
        let count = variable_accessor.count_matching(|id| id.get() > 42);
        assert_eq!(count, 2); // IDs 43 and 44

        // Test geometry and protocol system
        let box_protocol = BoxProtocol;
        let _sliver_protocol = SliverProtocol;

        // Test constraints operations
        let deflated = constraints.deflate_all(10.0);
        assert_eq!(deflated.max_width, 80.0); // 100 - 20
        assert_eq!(deflated.max_height, 80.0); // 100 - 20

        // Test that we can create mock tree operations
        struct MockTree;

        impl LayoutTree for MockTree {
            fn perform_layout(
                &mut self,
                _id: ElementId,
                _constraints: BoxConstraints,
            ) -> RenderResult<Size> {
                Ok(Size::new(50.0, 25.0))
            }

            fn perform_sliver_layout(
                &mut self,
                _id: ElementId,
                _constraints: flui_types::SliverConstraints,
            ) -> RenderResult<flui_types::SliverGeometry> {
                Ok(flui_types::SliverGeometry::default())
            }

            fn set_offset(&mut self, _id: ElementId, _offset: Offset) {}
            fn get_offset(&self, _id: ElementId) -> Option<Offset> {
                Some(Offset::ZERO)
            }
            fn mark_needs_layout(&mut self, _id: ElementId) {}
            fn needs_layout(&self, _id: ElementId) -> bool {
                false
            }
            fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
                None
            }
            fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
                None
            }
        }

        impl PaintTree for MockTree {
            fn perform_paint(
                &mut self,
                _id: ElementId,
                _offset: Offset,
            ) -> RenderResult<flui_painting::Canvas> {
                Ok(flui_painting::Canvas::new())
            }
            fn mark_needs_paint(&mut self, _id: ElementId) {}
            fn needs_paint(&self, _id: ElementId) -> bool {
                false
            }
            fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
                None
            }
            fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
                None
            }
        }

        impl HitTestTree for MockTree {
            fn hit_test(
                &self,
                _id: ElementId,
                _position: Offset,
                _result: &mut flui_interaction::HitTestResult,
            ) -> bool {
                true
            }
            fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
                None
            }
        }

        let mut mock_tree = MockTree;

        // Test context creation (compile-time verification)
        let layout_ctx = LayoutContext::new(&mut mock_tree, element_id, constraints, leaf_accessor);
        // Note: element_id field is private, but context creation works

        // Test that we can create different protocol contexts
        let _box_layout_ctx: BoxLayoutContext<'_, Leaf> = BoxLayoutContext::new(
            &mut mock_tree,
            element_id,
            constraints,
            Leaf::from_slice(&[]),
        );
        let _sliver_layout_ctx: SliverLayoutContext<'_, Variable> = SliverLayoutContext::new(
            &mut mock_tree,
            element_id,
            flui_types::SliverConstraints::default(),
            Variable::from_slice(&variable_children),
        );

        // Test wrapper/proxy system
        #[derive(Debug)]
        struct TestRenderBox;

        impl RenderObject for TestRenderBox {
            fn debug_name(&self) -> &'static str {
                "TestRenderBox"
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        impl RenderBox<Leaf> for TestRenderBox {
            fn layout(&mut self, _ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
                Ok(Size::new(42.0, 24.0))
            }

            fn paint(&self, _ctx: &mut BoxPaintContext<'_, Leaf>) {}

            fn as_render_object(&self) -> &dyn RenderObject {
                self
            }
        }

        let test_box = TestRenderBox;
        let wrapped = BoxRenderWrapper::new(Box::new(test_box));
        assert_eq!(wrapped.debug_name(), "BoxRenderWrapper");
    }
}
