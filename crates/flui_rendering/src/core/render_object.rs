//! Core render object trait with advanced introspection and lifecycle management.
//!
//! This module provides the foundational `RenderObject` trait that serves as the
//! base for all render objects in the FLUI rendering system. It emphasizes type
//! safety, performance, and comprehensive introspection capabilities.
//!
//! # Design Philosophy
//!
//! - **Protocol agnostic**: Works with any layout protocol (Box, Sliver, Custom)
//! - **Type erasure friendly**: Supports downcasting and introspection
//! - **Performance aware**: Provides hints for optimization
//! - **Debug friendly**: Rich debugging and profiling support
//! - **Thread safe**: All operations are Send + Sync compatible
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderObject (base trait)
//!     │
//!     ├── RenderBox<A> (box protocol)
//!     ├── SliverRender<A> (sliver protocol)
//!     └── CustomRender<A, P> (custom protocols)
//! ```
//!
//! # Key Features
//!
//! ## Type Safety and Introspection
//!
//! - **Safe downcasting**: Type-safe conversion to concrete types
//! - **Runtime type information**: Debug names and type introspection
//! - **Property inspection**: Access to render object properties
//!
//! ## Performance Optimization
//!
//! - **Layout complexity hints**: Help the scheduler optimize layout passes
//! - **Paint complexity hints**: Enable paint layer optimizations
//! - **Cache behavior**: Hints for caching strategies
//! - **Memory footprint**: Size estimates for memory management
//!
//! ## Lifecycle Management
//!
//! - **Creation notifications**: Setup and initialization hooks
//! - **Update notifications**: React to property changes
//! - **Disposal cleanup**: Resource cleanup and deallocation
//! - **Debug lifecycle tracking**: Monitor object creation/destruction
//!
//! # Usage Examples
//!
//! ## Basic Implementation
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderObject, ComplexityHint, CacheBehavior};
//!
//! #[derive(Debug)]
//! struct RenderColoredBox {
//!     color: Color,
//!     size: Size,
//! }
//!
//! impl RenderObject for RenderColoredBox {
//!     fn as_any(&self) -> &dyn Any {
//!         self
//!     }
//!
//!     fn as_any_mut(&mut self) -> &mut dyn Any {
//!         self
//!     }
//!
//!     fn debug_name(&self) -> &'static str {
//!         "RenderColoredBox"
//!     }
//!
//!     fn layout_complexity(&self) -> ComplexityHint {
//!         ComplexityHint::Constant // Simple, no children
//!     }
//!
//!     fn paint_complexity(&self) -> ComplexityHint {
//!         ComplexityHint::Constant // Single rectangle draw
//!     }
//!
//!     fn cache_behavior(&self) -> CacheBehavior {
//!         CacheBehavior::Static // Color rarely changes
//!     }
//! }
//! ```
//!
//! ## Advanced Implementation with Lifecycle
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderObject, LifecyclePhase, UpdateHint};
//!
//! #[derive(Debug)]
//! struct RenderExpensiveWidget {
//!     texture: Option<GpuTexture>,
//!     needs_texture_update: bool,
//! }
//!
//! impl RenderObject for RenderExpensiveWidget {
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//!
//!     fn debug_name(&self) -> &'static str {
//!         "RenderExpensiveWidget"
//!     }
//!
//!     fn lifecycle_event(&mut self, phase: LifecyclePhase) {
//!         match phase {
//!             LifecyclePhase::Created => {
//!                 tracing::info!("Creating expensive widget");
//!                 // Initialize GPU resources
//!             }
//!             LifecyclePhase::WillDispose => {
//!                 tracing::info!("Disposing expensive widget");
//!                 if let Some(texture) = self.texture.take() {
//!                     texture.dispose();
//!                 }
//!             }
//!             _ => {}
//!         }
//!     }
//!
//!     fn update_hint(&self) -> UpdateHint {
//!         if self.needs_texture_update {
//!             UpdateHint::ExpensiveLayout
//!         } else {
//!             UpdateHint::CheapPaint
//!         }
//!     }
//!
//!     fn memory_footprint(&self) -> usize {
//!         std::mem::size_of::<Self>() +
//!         self.texture.as_ref().map(|t| t.size_in_bytes()).unwrap_or(0)
//!     }
//! }
//! ```
//!
//! # Thread Safety
//!
//! All render objects must implement `Send + Sync`. This enables:
//! - Parallel layout computation
//! - Background resource loading
//! - Multi-threaded paint operations
//! - Cross-thread debugging and profiling

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};

use flui_foundation::ElementId;
use flui_types::{Offset, Rect, Size};

// ============================================================================
// CORE RENDER OBJECT TRAIT
// ============================================================================

/// Base trait for all render objects in the FLUI rendering system.
///
/// This trait provides the foundational capabilities that all render objects
/// must implement, regardless of their specific layout protocol or complexity.
/// It emphasizes type safety, performance optimization, and comprehensive
/// debugging support.
///
/// # Requirements
///
/// All implementors must be:
/// - `Send + Sync` for thread safety
/// - `Debug` for debugging support
/// - `'static` for type erasure compatibility
///
/// # Thread Safety
///
/// The trait is designed to be thread-safe. All methods should be safe to call
/// from multiple threads, though individual render objects may use interior
/// mutability for performance-critical paths.
pub trait RenderObject: Send + Sync + fmt::Debug + 'static {
    // ========================================================================
    // TYPE ERASURE AND INTROSPECTION
    // ========================================================================

    /// Returns a reference to this render object as `&dyn Any`.
    ///
    /// This enables type-safe downcasting to concrete render object types
    /// for specialized operations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(colored_box) = render_object.as_any().downcast_ref::<RenderColoredBox>() {
    ///     println!("Color: {:?}", colored_box.color);
    /// }
    /// ```
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to this render object as `&mut dyn Any`.
    ///
    /// This enables type-safe mutable downcasting for property updates
    /// and specialized mutations.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Returns a human-readable debug name for this render object.
    ///
    /// This should be a static string that identifies the type of render object
    /// for debugging, profiling, and development tools.
    ///
    /// # Default Implementation
    ///
    /// Returns the Rust type name, which may be mangled. Override for
    /// cleaner debug output.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns detailed type information about this render object.
    ///
    /// This includes protocol support, arity information, and other
    /// metadata useful for tooling and introspection.
    fn type_info(&self) -> RenderTypeInfo {
        RenderTypeInfo {
            name: self.debug_name(),
            type_id: std::any::TypeId::of::<Self>(),
            supports_box_protocol: false,
            supports_sliver_protocol: false,
            arity_info: None,
            category: RenderObjectCategory::Custom,
        }
    }

    // ========================================================================
    // PERFORMANCE HINTS
    // ========================================================================

    /// Returns the computational complexity of layout operations.
    ///
    /// This hint helps the layout scheduler optimize layout passes by
    /// prioritizing simple layouts and parallelizing complex ones.
    ///
    /// # Default Implementation
    ///
    /// Returns `ComplexityHint::Linear`, which is a safe default for
    /// most render objects that process their children sequentially.
    fn layout_complexity(&self) -> ComplexityHint {
        ComplexityHint::Linear
    }

    /// Returns the computational complexity of paint operations.
    ///
    /// This hint helps the paint scheduler optimize paint passes and
    /// layer composition strategies.
    ///
    /// # Default Implementation
    ///
    /// Returns `ComplexityHint::Linear`, which is appropriate for most
    /// render objects that paint themselves and their children.
    fn paint_complexity(&self) -> ComplexityHint {
        ComplexityHint::Linear
    }

    /// Returns the cache behavior characteristics of this render object.
    ///
    /// This helps the rendering system make intelligent caching decisions
    /// and optimize memory usage.
    ///
    /// # Default Implementation
    ///
    /// Returns `CacheBehavior::Dynamic`, which disables aggressive caching
    /// but is safe for objects with frequently changing properties.
    fn cache_behavior(&self) -> CacheBehavior {
        CacheBehavior::Dynamic
    }

    /// Returns the estimated memory footprint of this render object.
    ///
    /// This includes the object itself plus any owned resources like
    /// textures, meshes, or large data structures.
    ///
    /// # Default Implementation
    ///
    /// Returns the size of the type itself. Override to include additional
    /// resources for accurate memory profiling.
    fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    /// Returns performance optimization hints for this render object.
    ///
    /// These hints help the rendering system choose appropriate optimization
    /// strategies based on the object's characteristics.
    fn optimization_hints(&self) -> OptimizationHints {
        OptimizationHints {
            prefers_gpu_acceleration: false,
            benefits_from_caching: self.cache_behavior() != CacheBehavior::Never,
            supports_parallel_layout: false,
            supports_incremental_paint: false,
            memory_intensive: self.memory_footprint() > 1024 * 1024, // 1MB threshold
        }
    }

    // ========================================================================
    // LIFECYCLE MANAGEMENT
    // ========================================================================

    /// Called when the render object lifecycle changes.
    ///
    /// This provides hooks for initialization, cleanup, and other lifecycle
    /// events. The default implementation does nothing, but render objects
    /// can override to perform setup/teardown operations.
    ///
    /// # Arguments
    ///
    /// * `phase` - The lifecycle phase being entered
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn lifecycle_event(&mut self, phase: LifecyclePhase) {
    ///     match phase {
    ///         LifecyclePhase::Created => {
    ///             self.initialize_resources();
    ///         }
    ///         LifecyclePhase::WillDispose => {
    ///             self.cleanup_resources();
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    fn lifecycle_event(&mut self, _phase: LifecyclePhase) {
        // Default implementation does nothing
    }

    /// Returns hints about what type of updates this render object needs.
    ///
    /// This helps the rendering system prioritize updates and choose
    /// appropriate update strategies.
    ///
    /// # Default Implementation
    ///
    /// Returns `UpdateHint::Unknown`, which causes conservative update
    /// behavior. Override for better performance.
    fn update_hint(&self) -> UpdateHint {
        UpdateHint::Unknown
    }

    /// Called when properties of this render object have changed.
    ///
    /// This notification allows render objects to update internal state,
    /// invalidate caches, or perform other update-related operations.
    ///
    /// # Arguments
    ///
    /// * `hint` - Hint about what type of update occurred
    fn properties_updated(&mut self, _hint: UpdateHint) {
        // Default implementation does nothing
    }

    // ========================================================================
    // DEBUGGING AND PROFILING
    // ========================================================================

    /// Returns debugging information about this render object.
    ///
    /// This provides rich debugging data that can be used by development
    /// tools, profilers, and debugging utilities.
    fn debug_info(&self) -> RenderDebugInfo {
        RenderDebugInfo {
            type_name: self.debug_name(),
            memory_usage: self.memory_footprint(),
            layout_complexity: self.layout_complexity(),
            paint_complexity: self.paint_complexity(),
            cache_behavior: self.cache_behavior(),
            optimization_hints: self.optimization_hints(),
            custom_properties: self.debug_properties(),
        }
    }

    /// Returns custom debug properties specific to this render object type.
    ///
    /// Override this method to provide render-object-specific debugging
    /// information that will be displayed in development tools.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn debug_properties(&self) -> Vec<(&'static str, String)> {
    ///     vec![
    ///         ("color", format!("{:?}", self.color)),
    ///         ("corner_radius", format!("{:.2}", self.corner_radius)),
    ///         ("has_shadow", format!("{}", self.shadow.is_some())),
    ///     ]
    /// }
    /// ```
    fn debug_properties(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Returns a unique identifier for this render object instance.
    ///
    /// This is used for debugging, profiling, and tracking render object
    /// lifecycles. The default implementation uses the object's memory address.
    fn instance_id(&self) -> u64 {
        self as *const _ as u64
    }

    // ========================================================================
    // OPTIONAL PROTOCOL-SPECIFIC HOOKS
    // ========================================================================

    /// Returns the intrinsic size of this render object, if it has one.
    ///
    /// An intrinsic size is a natural size that the render object prefers
    /// when not constrained by layout. This is protocol-agnostic and can
    /// be used by any layout algorithm.
    ///
    /// # Returns
    ///
    /// `Some(size)` if this render object has a preferred intrinsic size,
    /// `None` if it should be sized by its parent's constraints.
    fn intrinsic_size(&self) -> Option<Size> {
        None
    }

    /// Returns the bounding box of this render object in its local coordinates.
    ///
    /// This is used for hit testing, clipping, and other spatial operations.
    /// The default implementation returns a zero-sized rectangle at the origin.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Checks if this render object handles pointer events.
    ///
    /// This is used by hit testing algorithms to determine whether to
    /// include this object in hit test results.
    ///
    /// # Default Implementation
    ///
    /// Returns `false`, meaning the object is transparent to pointer events.
    /// Override to `true` for interactive elements.
    fn handles_pointer_events(&self) -> bool {
        false
    }
}

// ============================================================================
// SUPPORTING TYPES AND ENUMS
// ============================================================================

/// Computational complexity hint for performance optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComplexityHint {
    /// Operation completes in constant time O(1).
    Constant,
    /// Operation complexity scales logarithmically O(log n).
    Logarithmic,
    /// Operation complexity scales linearly O(n).
    Linear,
    /// Operation complexity scales quadratically O(n²) or worse.
    Quadratic,
    /// Operation involves expensive computations (GPU operations, I/O, etc.).
    Expensive,
    /// Complexity is unknown or variable.
    Unknown,
}

/// Cache behavior characteristics for memory optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheBehavior {
    /// Object properties never change after creation - aggressive caching beneficial.
    Static,
    /// Object properties change infrequently - moderate caching beneficial.
    SemiStatic,
    /// Object properties change frequently - limited caching beneficial.
    Dynamic,
    /// Object should never be cached (streaming content, animations, etc.).
    Never,
}

/// Lifecycle phases that render objects can respond to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecyclePhase {
    /// Render object has been created and initialized.
    Created,
    /// Render object has been attached to the render tree.
    Attached,
    /// Render object properties are about to be updated.
    WillUpdate,
    /// Render object properties have been updated.
    DidUpdate,
    /// Render object is about to be detached from the render tree.
    WillDetach,
    /// Render object has been detached from the render tree.
    Detached,
    /// Render object is about to be disposed and should clean up resources.
    WillDispose,
}

/// Hints about what type of updates a render object needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateHint {
    /// Only visual properties changed - repaint sufficient.
    CheapPaint,
    /// Layout properties changed - relayout required.
    ExpensiveLayout,
    /// Structural changes - full rebuild may be needed.
    Structural,
    /// Update type is unknown - assume worst case.
    Unknown,
}

/// Category classification for render objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderObjectCategory {
    /// Leaf elements with no children (Text, Image, etc.).
    Leaf,
    /// Container elements that layout children (Flex, Stack, etc.).
    Container,
    /// Decorator elements that wrap a single child (Padding, Transform, etc.).
    Decorator,
    /// Special-purpose elements (Viewport, Scrollable, etc.).
    Special,
    /// Custom render objects with domain-specific behavior.
    Custom,
}

/// Comprehensive type information about a render object.
#[derive(Debug, Clone)]
pub struct RenderTypeInfo {
    /// Human-readable type name.
    pub name: &'static str,
    /// Rust type identifier for exact type matching.
    pub type_id: std::any::TypeId,
    /// Whether this render object supports the box layout protocol.
    pub supports_box_protocol: bool,
    /// Whether this render object supports the sliver layout protocol.
    pub supports_sliver_protocol: bool,
    /// Arity information if applicable.
    pub arity_info: Option<String>,
    /// General category classification.
    pub category: RenderObjectCategory,
}

/// Performance optimization hints for the rendering system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimizationHints {
    /// Whether this render object benefits from GPU acceleration.
    pub prefers_gpu_acceleration: bool,
    /// Whether caching would benefit this render object.
    pub benefits_from_caching: bool,
    /// Whether layout can be computed in parallel with siblings.
    pub supports_parallel_layout: bool,
    /// Whether paint operations can be done incrementally.
    pub supports_incremental_paint: bool,
    /// Whether this render object uses significant memory.
    pub memory_intensive: bool,
}

/// Comprehensive debugging information about a render object.
#[derive(Debug, Clone)]
pub struct RenderDebugInfo {
    /// Type name for identification.
    pub type_name: &'static str,
    /// Estimated memory usage in bytes.
    pub memory_usage: usize,
    /// Layout computational complexity.
    pub layout_complexity: ComplexityHint,
    /// Paint computational complexity.
    pub paint_complexity: ComplexityHint,
    /// Cache behavior characteristics.
    pub cache_behavior: CacheBehavior,
    /// Performance optimization hints.
    pub optimization_hints: OptimizationHints,
    /// Custom properties specific to the render object type.
    pub custom_properties: Vec<(&'static str, String)>,
}

// ============================================================================
// UTILITY TRAITS AND IMPLEMENTATIONS
// ============================================================================

/// Extension trait for working with boxed render objects.
pub trait RenderObjectExt {
    /// Attempts to downcast to a specific render object type.
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T>;

    /// Attempts to mutably downcast to a specific render object type.
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T>;

    /// Checks if this render object is of a specific type.
    fn is_type<T: RenderObject>(&self) -> bool;

    /// Gets a summary string for debugging.
    fn debug_summary(&self) -> String;
}

impl RenderObjectExt for dyn RenderObject {
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    fn is_type<T: RenderObject>(&self) -> bool {
        self.as_any().is::<T>()
    }

    fn debug_summary(&self) -> String {
        let info = self.debug_info();
        format!(
            "{} (mem: {} bytes, layout: {:?}, paint: {:?})",
            info.type_name, info.memory_usage, info.layout_complexity, info.paint_complexity
        )
    }
}

/// Helper for creating render object type information with protocol support.
pub struct RenderTypeInfoBuilder {
    info: RenderTypeInfo,
}

impl RenderTypeInfoBuilder {
    /// Creates a new type info builder for the given type.
    pub fn new<T: RenderObject>() -> Self {
        Self {
            info: RenderTypeInfo {
                name: std::any::type_name::<T>(),
                type_id: std::any::TypeId::of::<T>(),
                supports_box_protocol: false,
                supports_sliver_protocol: false,
                arity_info: None,
                category: RenderObjectCategory::Custom,
            },
        }
    }

    /// Sets the human-readable name.
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.info.name = name;
        self
    }

    /// Marks as supporting the box protocol.
    pub fn with_box_protocol(mut self) -> Self {
        self.info.supports_box_protocol = true;
        self
    }

    /// Marks as supporting the sliver protocol.
    pub fn with_sliver_protocol(mut self) -> Self {
        self.info.supports_sliver_protocol = true;
        self
    }

    /// Sets the arity information.
    pub fn with_arity_info(mut self, arity: String) -> Self {
        self.info.arity_info = Some(arity);
        self
    }

    /// Sets the category.
    pub fn with_category(mut self, category: RenderObjectCategory) -> Self {
        self.info.category = category;
        self
    }

    /// Builds the final type information.
    pub fn build(self) -> RenderTypeInfo {
        self.info
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct MockRenderObject {
        name: &'static str,
        complexity: ComplexityHint,
        memory_size: usize,
    }

    impl RenderObject for MockRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn debug_name(&self) -> &'static str {
            self.name
        }

        fn layout_complexity(&self) -> ComplexityHint {
            self.complexity
        }

        fn memory_footprint(&self) -> usize {
            self.memory_size
        }

        fn debug_properties(&self) -> Vec<(&'static str, String)> {
            vec![
                ("complexity", format!("{:?}", self.complexity)),
                ("memory", format!("{} bytes", self.memory_size)),
            ]
        }
    }

    #[test]
    fn test_render_object_basic() {
        let obj = MockRenderObject {
            name: "MockRender",
            complexity: ComplexityHint::Constant,
            memory_size: 1024,
        };

        assert_eq!(obj.debug_name(), "MockRender");
        assert_eq!(obj.layout_complexity(), ComplexityHint::Constant);
        assert_eq!(obj.memory_footprint(), 1024);
    }

    #[test]
    fn test_downcast() {
        let mut obj = MockRenderObject {
            name: "TestObject",
            complexity: ComplexityHint::Linear,
            memory_size: 512,
        };

        let render_obj: &mut dyn RenderObject = &mut obj;

        // Test successful downcast
        assert!(render_obj.is_type::<MockRenderObject>());
        let downcasted = render_obj.downcast_ref::<MockRenderObject>().unwrap();
        assert_eq!(downcasted.name, "TestObject");

        // Test failed downcast
        #[derive(Debug)]
        struct OtherRenderObject;
        impl RenderObject for OtherRenderObject {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        assert!(!render_obj.is_type::<OtherRenderObject>());
        assert!(render_obj.downcast_ref::<OtherRenderObject>().is_none());
    }

    #[test]
    fn test_debug_info() {
        let obj = MockRenderObject {
            name: "DebugTest",
            complexity: ComplexityHint::Quadratic,
            memory_size: 2048,
        };

        let debug_info = obj.debug_info();
        assert_eq!(debug_info.type_name, "DebugTest");
        assert_eq!(debug_info.memory_usage, 2048);
        assert_eq!(debug_info.layout_complexity, ComplexityHint::Quadratic);
        assert_eq!(debug_info.custom_properties.len(), 2);
    }

    #[test]
    fn test_debug_summary() {
        let mut obj = MockRenderObject {
            name: "SummaryTest",
            complexity: ComplexityHint::Expensive,
            memory_size: 4096,
        };

        let render_obj: &mut dyn RenderObject = &mut obj;
        let summary = render_obj.debug_summary();

        assert!(summary.contains("SummaryTest"));
        assert!(summary.contains("4096 bytes"));
        assert!(summary.contains("Expensive"));
    }

    #[test]
    fn test_type_info_builder() {
        let info = RenderTypeInfoBuilder::new::<MockRenderObject>()
            .with_name("CustomMock")
            .with_box_protocol()
            .with_category(RenderObjectCategory::Container)
            .with_arity_info("Variable".to_string())
            .build();

        assert_eq!(info.name, "CustomMock");
        assert!(info.supports_box_protocol);
        assert!(!info.supports_sliver_protocol);
        assert_eq!(info.category, RenderObjectCategory::Container);
        assert_eq!(info.arity_info.as_ref().unwrap(), "Variable");
    }

    #[test]
    fn test_optimization_hints() {
        let obj = MockRenderObject {
            name: "OptimTest",
            complexity: ComplexityHint::Constant,
            memory_size: 2 * 1024 * 1024, // 2MB - should be memory intensive
        };

        let hints = obj.optimization_hints();
        assert!(hints.memory_intensive); // Above 1MB threshold
        assert!(hints.benefits_from_caching); // Default cache behavior is Dynamic
    }

    #[test]
    fn test_lifecycle_phases() {
        let mut obj = MockRenderObject {
            name: "LifecycleTest",
            complexity: ComplexityHint::Linear,
            memory_size: 100,
        };

        // Test that default lifecycle_event doesn't panic
        obj.lifecycle_event(LifecyclePhase::Created);
        obj.lifecycle_event(LifecyclePhase::WillDispose);
    }
}
