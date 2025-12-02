//! Advanced arity system with GAT integration and rendering-specific extensions.
//!
//! This module provides a comprehensive arity system that leverages the unified
//! arity abstractions from `flui-tree` while adding rendering-specific extensions
//! for maximum ergonomics and performance.
//!
//! # Design Philosophy
//!
//! - **Unified system**: Single source of truth from `flui-tree`
//! - **Zero-cost abstractions**: GAT and const generics for compile-time optimization
//! - **Type safety**: Compile-time child count validation
//! - **Ergonomics**: Convenient methods for common rendering operations
//! - **Performance**: Optimized accessors and batch operations
//!
//! # Arity Types
//!
//! | Arity | Child Count | Use Cases | Examples |
//! |-------|-------------|-----------|----------|
//! | [`Leaf`] | 0 | Terminal elements | Text, Image, Spacer |
//! | [`Optional`] | 0-1 | Conditional content | Container, SizedBox |
//! | [`Single`] | 1 | Wrappers/decorators | Padding, Transform, Align |
//! | [`Variable`] | 0+ | Dynamic layouts | Flex, Stack, Column, Row |
//! | [`Exact<N>`] | N | Fixed layouts | Grid cells, Tab pairs |
//! | [`AtLeast<N>`] | N+ | Minimum requirements | TabBar, MenuBar |
//! | [`Range<MIN, MAX>`] | MIN-MAX | Bounded layouts | Carousel, PageView |
//!
//! # GAT Integration
//!
//! The arity system leverages Generic Associated Types for flexible, zero-cost
//! abstractions:
//!
//! ```rust,ignore
//! trait ChildrenAccess<'a, T> {
//!     type Iter: Iterator<Item = &'a T> + 'a;
//!
//!     fn iter(&self) -> Self::Iter;
//!     fn as_slice(&self) -> &'a [T];
//!     fn len(&self) -> usize;
//!
//!     // HRTB-based operations
//!     fn find_where<F>(&self, predicate: F) -> Option<&'a T>
//!     where F: for<'b> Fn(&'b T) -> bool;
//! }
//! ```
//!
//! # Performance Features
//!
//! - **Const generic optimization**: Batch operations with compile-time sizing
//! - **Stack allocation**: Small collections use stack storage
//! - **Cache-friendly access**: Contiguous memory layout for children
//! - **Branch prediction**: Likely/unlikely hints for common paths
//!
//! # Usage Examples
//!
//! ## Basic Usage
//!
//! ```rust,ignore
//! use flui_rendering::core::{Single, Variable, RenderChildrenExt};
//!
//! // Single child access
//! let children = [ElementId::new(42)];
//! let accessor = Single::from_slice(&children);
//! let child_id = accessor.single_child_id();
//!
//! // Variable children access
//! let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
//! let accessor = Variable::from_slice(&children);
//!
//! // Iterate efficiently
//! for child_id in accessor.element_ids() {
//!     println!("Child: {}", child_id);
//! }
//!
//! // Use HRTB predicates
//! let first_large = accessor.find_where(|id| id.get() > 2);
//! ```
//!
//! ## Advanced Operations
//!
//! ```rust,ignore
//! use flui_rendering::core::{Variable, RenderChildrenExt, VariableChildExt};
//!
//! let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
//! let accessor = Variable::from_slice(&children);
//!
//! // Batch operations with const generics
//! let batch_result = accessor.process_batch::<2>(|batch| {
//!     // Process up to 2 elements at a time
//!     batch.iter().map(|id| id.get()).sum::<usize>()
//! });
//!
//! // Conditional operations
//! let filtered_count = accessor.count_where(|id| id.get() % 2 == 0);
//!
//! // Spatial operations for rendering
//! let bounds = accessor.compute_bounds(|id| get_element_bounds(*id));
//! ```

use flui_foundation::ElementId;

// ============================================================================
// CORE RE-EXPORTS FROM FLUI-TREE
// ============================================================================

// Core arity trait and markers
pub use flui_tree::arity::{
    AccessPattern,
    // Core trait with GAT support
    Arity,

    // Arity markers with const generic support
    AtLeast,
    Exact,
    Leaf,
    // Advanced features
    Optional,
    Range,
    RuntimeArity,
    Single,
    Variable,
};

// GAT-based accessor types
pub use flui_tree::arity::{
    // Core accessor trait
    ChildrenAccess,

    // Concrete accessor implementations
    FixedChildren,
    NoChildren,
    OptionalChild,
    SliceChildren,
};

// ============================================================================
// RENDERING-SPECIFIC EXTENSIONS
// ============================================================================

/// Core extension trait for children accessors with ElementId-specific operations.
///
/// This trait provides ergonomic methods for working with `ElementId` children
/// in rendering operations, leveraging HRTB for maximum flexibility.
pub trait RenderChildrenExt<'a>: ChildrenAccess<'a, ElementId> {
    /// Returns an iterator over ElementIds by value.
    ///
    /// This is optimized for the common case of copying ElementIds.
    #[inline]
    fn element_ids(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.iter().copied()
    }

    /// Returns the first ElementId, if any.
    #[inline]
    fn first_id(&self) -> Option<ElementId> {
        self.as_slice().first().copied()
    }

    /// Returns the last ElementId, if any.
    #[inline]
    fn last_id(&self) -> Option<ElementId> {
        self.as_slice().last().copied()
    }

    /// Returns ElementId at the given index, if valid.
    #[inline]
    fn id_at(&self, index: usize) -> Option<ElementId> {
        self.as_slice().get(index).copied()
    }

    /// Checks if the accessor contains the given ElementId.
    #[inline]
    fn contains_id(&self, id: ElementId) -> bool {
        self.as_slice().contains(&id)
    }

    /// Finds the index of the given ElementId.
    #[inline]
    fn index_of(&self, id: ElementId) -> Option<usize> {
        self.as_slice().iter().position(|&x| x == id)
    }

    /// Finds the first ElementId matching the predicate using HRTB.
    fn find_id_where<F>(&self, predicate: F) -> Option<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.as_slice().iter().find(|&id| predicate(id)).copied()
    }

    /// Counts ElementIds matching the predicate.
    fn count_where<F>(&self, predicate: F) -> usize
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.as_slice().iter().filter(|&id| predicate(id)).count()
    }

    /// Collects ElementIds matching the predicate.
    fn collect_where<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.as_slice()
            .iter()
            .filter(|&id| predicate(id))
            .copied()
            .collect()
    }

    /// Processes elements in batches for performance optimization.
    ///
    /// Uses const generics for compile-time optimization of batch size.
    fn process_batch<const BATCH_SIZE: usize, F, R>(&self, mut processor: F) -> Vec<R>
    where
        F: FnMut(&[ElementId]) -> R,
    {
        let children = self.as_slice();
        let mut results = Vec::with_capacity((children.len() + BATCH_SIZE - 1) / BATCH_SIZE);

        for chunk in children.chunks(BATCH_SIZE) {
            results.push(processor(chunk));
        }

        results
    }

    /// Checks if any ElementId matches the predicate.
    fn any_where<F>(&self, predicate: F) -> bool
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.as_slice().iter().any(|id| predicate(id))
    }

    /// Checks if all ElementIds match the predicate.
    fn all_where<F>(&self, predicate: F) -> bool
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.as_slice().iter().all(|id| predicate(id))
    }

    /// Partitions ElementIds based on the predicate.
    fn partition_where<F>(&self, predicate: F) -> (Vec<ElementId>, Vec<ElementId>)
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        let mut matching = Vec::new();
        let mut non_matching = Vec::new();

        for &id in self.as_slice() {
            if predicate(&id) {
                matching.push(id);
            } else {
                non_matching.push(id);
            }
        }

        (matching, non_matching)
    }
}

// Blanket implementation for all accessors over ElementId
impl<'a, A> RenderChildrenExt<'a> for A where A: ChildrenAccess<'a, ElementId> {}

// ============================================================================
// ARITY-SPECIFIC EXTENSIONS
// ============================================================================

/// Extension trait for OptionalChild with ElementId convenience methods.
pub trait OptionalChildExt<'a> {
    /// Gets the optional ElementId by value, if present.
    fn child_id(&self) -> Option<ElementId>;

    /// Unwraps the ElementId or panics with a descriptive message.
    fn unwrap_child_id(&self) -> ElementId;

    /// Gets the ElementId or returns the default value.
    fn child_id_or(&self, default: ElementId) -> ElementId;

    /// Gets the ElementId or computes it from the closure.
    fn child_id_or_else<F>(&self, f: F) -> ElementId
    where
        F: FnOnce() -> ElementId;
}

impl<'a> OptionalChildExt<'a> for OptionalChild<'a, ElementId> {
    #[inline]
    fn child_id(&self) -> Option<ElementId> {
        self.get().copied()
    }

    #[inline]
    fn unwrap_child_id(&self) -> ElementId {
        self.child_id().expect("OptionalChild was None")
    }

    #[inline]
    fn child_id_or(&self, default: ElementId) -> ElementId {
        self.child_id().unwrap_or(default)
    }

    #[inline]
    fn child_id_or_else<F>(&self, f: F) -> ElementId
    where
        F: FnOnce() -> ElementId,
    {
        self.child_id().unwrap_or_else(f)
    }
}

/// Extension trait for FixedChildren<1> (Single) with ElementId convenience methods.
pub trait SingleChildExt<'a> {
    /// Gets the single ElementId by value.
    fn single_child_id(&self) -> ElementId;

    /// Gets a reference to the single ElementId.
    fn single_child_ref(&self) -> &ElementId;
}

impl<'a> SingleChildExt<'a> for FixedChildren<'a, ElementId, 1> {
    #[inline]
    fn single_child_id(&self) -> ElementId {
        *self.single()
    }

    #[inline]
    fn single_child_ref(&self) -> &ElementId {
        self.single()
    }
}

/// Extension trait for Variable arity with performance optimizations.
pub trait VariableChildExt<'a> {
    /// Performs a parallel-friendly operation on children.
    ///
    /// This method can be optimized by implementations to use parallel
    /// processing for large numbers of children.
    fn parallel_map<F, R>(&self, f: F) -> Vec<R>
    where
        F: Fn(ElementId) -> R + Send + Sync,
        R: Send;

    /// Finds multiple ElementIds matching the predicate efficiently.
    fn find_all_where<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool;

    /// Computes aggregate statistics over the children.
    fn compute_stats<F, T>(&self, extractor: F) -> ChildrenStats<T>
    where
        F: Fn(ElementId) -> T,
        T: Clone + PartialOrd + std::fmt::Debug;
}

impl<'a> VariableChildExt<'a> for SliceChildren<'a, ElementId> {
    fn parallel_map<F, R>(&self, f: F) -> Vec<R>
    where
        F: Fn(ElementId) -> R + Send + Sync,
        R: Send,
    {
        // For small collections, sequential is faster due to overhead
        if self.len() < 100 {
            self.element_ids().map(f).collect()
        } else {
            // For larger collections, could use rayon for parallel processing
            // For now, use sequential implementation
            self.element_ids().map(f).collect()
        }
    }

    fn find_all_where<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.collect_where(predicate)
    }

    fn compute_stats<F, T>(&self, extractor: F) -> ChildrenStats<T>
    where
        F: Fn(ElementId) -> T,
        T: Clone + PartialOrd + std::fmt::Debug,
    {
        let values: Vec<T> = self.element_ids().map(extractor).collect();

        if values.is_empty() {
            return ChildrenStats::empty();
        }

        let min = values.iter().min().unwrap().clone();
        let max = values.iter().max().unwrap().clone();
        let count = values.len();

        ChildrenStats {
            count,
            min: Some(min),
            max: Some(max),
            values,
        }
    }
}

// ============================================================================
// UTILITY TYPES
// ============================================================================

/// Statistics computed over a collection of children.
#[derive(Debug, Clone)]
pub struct ChildrenStats<T> {
    /// Number of children.
    pub count: usize,
    /// Minimum value, if any.
    pub min: Option<T>,
    /// Maximum value, if any.
    pub max: Option<T>,
    /// All values for further processing.
    pub values: Vec<T>,
}

impl<T> ChildrenStats<T> {
    /// Creates empty statistics.
    pub fn empty() -> Self {
        Self {
            count: 0,
            min: None,
            max: None,
            values: Vec::new(),
        }
    }

    /// Checks if there are no children.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ============================================================================
// CONVENIENCE TYPE ALIASES
// ============================================================================

/// Type alias for children accessor with Leaf arity.
pub type LeafChildren<'a> = NoChildren<ElementId>;

/// Type alias for children accessor with Optional arity.
pub type OptionalChildren<'a> = OptionalChild<'a, ElementId>;

/// Type alias for children accessor with Single arity.
pub type SingleChildren<'a> = FixedChildren<'a, ElementId, 1>;

/// Type alias for children accessor with Variable arity.
pub type VariableChildren<'a> = SliceChildren<'a, ElementId>;

/// Type alias for children accessor with Exact<N> arity.
pub type ExactChildren<'a, const N: usize> = FixedChildren<'a, ElementId, N>;

/// Type alias for children accessor with AtLeast<N> arity.
pub type AtLeastChildren<'a> = SliceChildren<'a, ElementId>;

/// Type alias for children accessor with Range<MIN, MAX> arity.
pub type RangeChildren<'a> = SliceChildren<'a, ElementId>;

// ============================================================================
// PERFORMANCE OPTIMIZATIONS
// ============================================================================

/// Marker trait for arities that can benefit from stack allocation.
pub trait StackOptimized: Arity {
    /// Maximum number of children that should use stack allocation.
    const STACK_THRESHOLD: usize = 8;

    /// Whether this arity typically uses small numbers of children.
    const PREFERS_STACK: bool = false;
}

impl StackOptimized for Leaf {
    const PREFERS_STACK: bool = true;
}

impl StackOptimized for Optional {
    const PREFERS_STACK: bool = true;
}

// Single is an alias for Exact<1>, so it uses the Exact<N> implementation

impl<const N: usize> StackOptimized for Exact<N> {
    const STACK_THRESHOLD: usize = N;
    const PREFERS_STACK: bool = N <= 8;
}

/// Marker trait for arities that can benefit from parallel processing.
pub trait ParallelOptimized: Arity {
    /// Minimum number of children before parallel processing is beneficial.
    const PARALLEL_THRESHOLD: usize = 100;

    /// Whether this arity can benefit from parallel operations.
    const SUPPORTS_PARALLEL: bool = false;
}

impl ParallelOptimized for Variable {
    const SUPPORTS_PARALLEL: bool = true;
}

impl<const N: usize> ParallelOptimized for AtLeast<N> {
    const SUPPORTS_PARALLEL: bool = true;
}

// ============================================================================
// DEBUG AND INTROSPECTION
// ============================================================================

/// Debugging information about an arity configuration.
#[derive(Debug, Clone)]
pub struct ArityInfo {
    /// Human-readable name of the arity.
    pub name: &'static str,
    /// Runtime arity information.
    pub runtime: RuntimeArity,
    /// Performance characteristics.
    pub performance: PerformanceProfile,
}

/// Performance profile for an arity type.
#[derive(Debug, Clone)]
pub struct PerformanceProfile {
    /// Prefers stack allocation.
    pub stack_optimized: bool,
    /// Supports parallel processing.
    pub parallel_optimized: bool,
    /// Typical access pattern.
    pub access_pattern: AccessPattern,
}

/// Extension trait for getting arity information.
pub trait ArityInfoExt: Arity {
    /// Gets debugging information about this arity.
    fn arity_info() -> ArityInfo;
}

impl ArityInfoExt for Leaf {
    fn arity_info() -> ArityInfo {
        ArityInfo {
            name: "Leaf",
            runtime: RuntimeArity::Exact(0),
            performance: PerformanceProfile {
                stack_optimized: true,
                parallel_optimized: false,
                access_pattern: AccessPattern::Never,
            },
        }
    }
}

impl ArityInfoExt for Single {
    fn arity_info() -> ArityInfo {
        ArityInfo {
            name: "Single",
            runtime: RuntimeArity::Exact(1),
            performance: PerformanceProfile {
                stack_optimized: true,
                parallel_optimized: false,
                access_pattern: AccessPattern::Sequential,
            },
        }
    }
}

impl ArityInfoExt for Variable {
    fn arity_info() -> ArityInfo {
        ArityInfo {
            name: "Variable",
            runtime: RuntimeArity::Variable,
            performance: PerformanceProfile {
                stack_optimized: false,
                parallel_optimized: true,
                access_pattern: AccessPattern::Sequential,
            },
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_children_ext_basic() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);

        assert_eq!(accessor.first_id(), Some(ElementId::new(1)));
        assert_eq!(accessor.last_id(), Some(ElementId::new(3)));
        assert_eq!(accessor.id_at(1), Some(ElementId::new(2)));
        assert!(accessor.contains_id(ElementId::new(2)));
        assert!(!accessor.contains_id(ElementId::new(99)));
        assert_eq!(accessor.index_of(ElementId::new(2)), Some(1));
    }

    #[test]
    fn test_render_children_ext_predicates() {
        let children = [
            ElementId::new(1),
            ElementId::new(2),
            ElementId::new(3),
            ElementId::new(4),
        ];
        let accessor = Variable::from_slice(&children);

        // Test HRTB predicates
        let first_even = accessor.find_id_where(|id| id.get() % 2 == 0);
        assert_eq!(first_even, Some(ElementId::new(2)));

        let count_odd = accessor.count_where(|id| id.get() % 2 == 1);
        assert_eq!(count_odd, 2);

        let odd_ids = accessor.collect_where(|id| id.get() % 2 == 1);
        assert_eq!(odd_ids, vec![ElementId::new(1), ElementId::new(3)]);

        assert!(accessor.any_where(|id| id.get() > 3));
        assert!(!accessor.all_where(|id| id.get() > 3));
    }

    #[test]
    fn test_optional_child_ext() {
        let children = [ElementId::new(42)];
        let accessor = Optional::from_slice(&children);

        assert_eq!(accessor.child_id(), Some(ElementId::new(42)));
        assert_eq!(accessor.unwrap_child_id(), ElementId::new(42));
        assert_eq!(accessor.child_id_or(ElementId::new(1)), ElementId::new(42));

        let empty: [ElementId; 0] = [];
        let empty_accessor = Optional::from_slice(&empty);
        assert_eq!(empty_accessor.child_id(), None);
        assert_eq!(
            empty_accessor.child_id_or(ElementId::new(1)),
            ElementId::new(1)
        );
    }

    #[test]
    fn test_single_child_ext() {
        let children = [ElementId::new(42)];
        let accessor = Single::from_slice(&children);

        assert_eq!(accessor.single_child_id(), ElementId::new(42));
        assert_eq!(*accessor.single_child_ref(), ElementId::new(42));
    }

    #[test]
    fn test_batch_processing() {
        let children = [
            ElementId::new(1),
            ElementId::new(2),
            ElementId::new(3),
            ElementId::new(4),
            ElementId::new(5),
        ];
        let accessor = Variable::from_slice(&children);

        let batch_results = accessor
            .process_batch::<2, _, _>(|batch| batch.iter().map(|id| id.get()).sum::<usize>());

        // Should have 3 batches: [1,2], [3,4], [5]
        assert_eq!(batch_results.len(), 3);
        assert_eq!(batch_results[0], 3); // 1 + 2
        assert_eq!(batch_results[1], 7); // 3 + 4
        assert_eq!(batch_results[2], 5); // 5
    }

    #[test]
    fn test_variable_child_ext() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);

        let mapped = accessor.parallel_map(|id| id.get() * 2);
        assert_eq!(mapped, vec![2, 4, 6]);

        let all_where = accessor.find_all_where(|id| id.get() > 1);
        assert_eq!(all_where, vec![ElementId::new(2), ElementId::new(3)]);

        let stats = accessor.compute_stats(|id| id.get());
        assert_eq!(stats.count, 3);
        assert_eq!(stats.min, Some(1));
        assert_eq!(stats.max, Some(3));
    }

    #[test]
    fn test_partition() {
        let children = [
            ElementId::new(1),
            ElementId::new(2),
            ElementId::new(3),
            ElementId::new(4),
        ];
        let accessor = Variable::from_slice(&children);

        let (even, odd) = accessor.partition_where(|id| id.get() % 2 == 0);
        assert_eq!(even, vec![ElementId::new(2), ElementId::new(4)]);
        assert_eq!(odd, vec![ElementId::new(1), ElementId::new(3)]);
    }

    #[test]
    fn test_arity_info() {
        let leaf_info = Leaf::arity_info();
        assert_eq!(leaf_info.name, "Leaf");
        assert_eq!(leaf_info.runtime, RuntimeArity::Exact(0));
        assert!(leaf_info.performance.stack_optimized);

        let single_info = Single::arity_info();
        assert_eq!(single_info.name, "Single");
        assert_eq!(single_info.runtime, RuntimeArity::Exact(1));

        let variable_info = Variable::arity_info();
        assert_eq!(variable_info.name, "Variable");
        assert_eq!(variable_info.runtime, RuntimeArity::Variable);
        assert!(variable_info.performance.parallel_optimized);
    }

    #[test]
    fn test_children_stats() {
        let stats = ChildrenStats::<i32>::empty();
        assert!(stats.is_empty());
        assert_eq!(stats.count, 0);
        assert_eq!(stats.min, None);
        assert_eq!(stats.max, None);

        let values = vec![1, 3, 2, 5, 4];
        let stats = ChildrenStats {
            count: values.len(),
            min: values.iter().min().copied(),
            max: values.iter().max().copied(),
            values: values.clone(),
        };

        assert!(!stats.is_empty());
        assert_eq!(stats.count, 5);
        assert_eq!(stats.min, Some(1));
        assert_eq!(stats.max, Some(5));
    }

    #[test]
    fn test_stack_optimization_markers() {
        assert!(Leaf::PREFERS_STACK);
        assert!(Optional::PREFERS_STACK);
        assert!(Single::PREFERS_STACK);
        assert_eq!(Leaf::STACK_THRESHOLD, 8);

        assert!(Exact::<4>::PREFERS_STACK);
        assert!(!Exact::<16>::PREFERS_STACK);
    }

    #[test]
    fn test_parallel_optimization_markers() {
        assert!(!Leaf::SUPPORTS_PARALLEL);
        assert!(!Single::SUPPORTS_PARALLEL);
        assert!(Variable::SUPPORTS_PARALLEL);

        assert_eq!(Variable::PARALLEL_THRESHOLD, 100);
    }

    #[test]
    fn test_type_aliases() {
        let _: LeafChildren = NoChildren::new();

        let children = [ElementId::new(1)];
        let _: OptionalChildren = Optional::from_slice(&children);
        let _: SingleChildren = Single::from_slice(&children);

        let children = [ElementId::new(1), ElementId::new(2)];
        let _: VariableChildren = Variable::from_slice(&children);
        let _: ExactChildren<2> = Exact::<2>::from_slice(&children);
    }
}
