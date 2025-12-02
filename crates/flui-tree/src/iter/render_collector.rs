//! Render Children Collector with Arity-Aware Collection
//!
//! This module provides `RenderChildrenCollector`, a utility for collecting
//! render children and converting them to typed arity accessors. This is
//! essential for layout operations that need to collect children before
//! processing to avoid borrow conflicts.
//!
//! # Key Features
//!
//! - **Arity-aware collection** - Convert to `Single`, `Optional`, `Variable` accessors
//! - **Zero-copy when possible** - Stores references, not owned data
//! - **HRTB compatibility** - Works with universal predicates
//! - **GAT support** - Flexible iterator types
//! - **Performance optimized** - Uses const generics for stack allocation

use flui_foundation::ElementId;
use std::marker::PhantomData;

use crate::arity::{Arity, Leaf, Optional, Single, Variable};
use crate::traits::RenderTreeAccess;

// ============================================================================
// RENDER CHILDREN COLLECTOR
// ============================================================================

/// Collector for render children with arity-aware conversion.
///
/// This utility collects render children from a parent element and provides
/// methods to convert them to typed arity accessors. This is particularly
/// useful in layout operations where you need to collect children first
/// to release the borrow on the tree before performing mutable operations.
///
/// # Design Rationale
///
/// The collector solves the borrow checker conflict that arises when you
/// need both:
/// 1. Access to the tree to iterate children
/// 2. Mutable access to the tree for layout operations
///
/// By collecting children first, we release the tree borrow and enable
/// subsequent mutable operations.
///
/// # Performance
///
/// - Uses `SmallVec` with const generic inline storage
/// - Stack allocation for typical child counts (≤32)
/// - Zero-cost conversion to arity accessors
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{RenderChildrenCollector, Single, Optional, Variable};
///
/// fn layout_single<T: RenderTreeAccess>(tree: &mut T, parent: ElementId) -> Size {
///     // Collect children first to release borrow
///     let collector = RenderChildrenCollector::new(tree, parent);
///
///     // Convert to typed accessor
///     if let Some(accessor) = collector.try_into_arity::<Single>() {
///         let child = accessor.single();
///
///         // Now we can mutably access tree for layout
///         return layout_child(tree, child, constraints);
///     }
///
///     Size::ZERO
/// }
/// ```
pub struct RenderChildrenCollector<const INLINE_SIZE: usize = 32> {
    /// Collected render children with inline storage optimization
    children: smallvec::SmallVec<[ElementId; INLINE_SIZE]>,
}

impl<const INLINE_SIZE: usize> RenderChildrenCollector<INLINE_SIZE> {
    /// Creates a new collector and immediately collects render children.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to collect from
    /// * `parent` - The parent element whose render children to collect
    ///
    /// # Performance
    ///
    /// This method performs the collection immediately to release the
    /// tree borrow. Uses inline storage for up to `INLINE_SIZE` children.
    pub fn new<T: RenderTreeAccess>(tree: &T, parent: ElementId) -> Self {
        let children = tree.render_children_iter(parent).collect();
        Self { children }
    }

    /// Creates a collector with a specific capacity hint.
    ///
    /// Use this when you know the approximate number of children
    /// to optimize memory allocation.
    pub fn with_capacity<T: RenderTreeAccess>(
        tree: &T,
        parent: ElementId,
        capacity: usize,
    ) -> Self {
        let mut children = smallvec::SmallVec::with_capacity(capacity);
        children.extend(tree.render_children_iter(parent));
        Self { children }
    }

    /// Returns the number of collected children.
    #[inline]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns `true` if no children were collected.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns a slice of the collected children.
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        &self.children
    }

    /// Consumes the collector and returns the collected children.
    #[inline]
    pub fn into_vec(self) -> Vec<ElementId> {
        self.children.into_vec()
    }

    /// Consumes the collector and returns the SmallVec directly.
    #[inline]
    pub fn into_inner(self) -> smallvec::SmallVec<[ElementId; INLINE_SIZE]> {
        self.children
    }

    // ========================================================================
    // ARITY CONVERSIONS
    // ========================================================================

    /// Attempts to convert to a specific arity accessor.
    ///
    /// Returns `Some(accessor)` if the child count matches the arity,
    /// `None` otherwise.
    ///
    /// # Type Parameters
    ///
    /// * `A` - The target arity type
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(accessor) = collector.try_into_arity::<Single>() {
    ///     let child = accessor.single();
    ///     // Process single child
    /// }
    /// ```
    pub fn try_into_arity<A: Arity>(self) -> Option<CollectedChildren<A, INLINE_SIZE>> {
        if A::validate_count(self.len()) {
            Some(CollectedChildren {
                children: self.children,
                _arity: PhantomData,
            })
        } else {
            None
        }
    }

    /// Converts to a Variable (multi-child) accessor.
    ///
    /// This always succeeds since Variable arity accepts any child count.
    #[inline]
    pub fn into_variable(self) -> CollectedChildren<Variable, INLINE_SIZE> {
        CollectedChildren {
            children: self.children,
            _arity: PhantomData,
        }
    }

    /// Alias for `into_variable()` - converts to multi-child accessor.
    #[inline]
    pub fn into_multi(self) -> CollectedChildren<Variable, INLINE_SIZE> {
        self.into_variable()
    }

    /// Converts to an Optional accessor.
    ///
    /// # Panics
    ///
    /// Panics if there are more than 1 children. In debug mode, this is
    /// checked immediately. In release mode, the check is deferred
    /// until methods are called.
    #[inline]
    pub fn into_optional(self) -> CollectedChildren<Optional, INLINE_SIZE> {
        #[cfg(debug_assertions)]
        {
            assert!(
                self.len() <= 1,
                "Optional arity expects 0 or 1 children, found {}",
                self.len()
            );
        }
        CollectedChildren {
            children: self.children,
            _arity: PhantomData,
        }
    }

    /// Converts to a Single accessor.
    ///
    /// # Panics
    ///
    /// Panics if there is not exactly 1 child. In debug mode, this is
    /// checked immediately. In release mode, the check is deferred
    /// until `single()` is called.
    #[inline]
    pub fn into_single(self) -> CollectedChildren<Single, INLINE_SIZE> {
        #[cfg(debug_assertions)]
        {
            assert!(
                self.len() == 1,
                "Single arity expects exactly 1 child, found {}",
                self.len()
            );
        }
        CollectedChildren {
            children: self.children,
            _arity: PhantomData,
        }
    }

    /// Converts to a Leaf accessor (validates no children).
    ///
    /// # Panics
    ///
    /// Panics if there are any children. In debug mode, this is
    /// checked immediately. In release mode, the check is deferred
    /// until `validate_empty()` is called.
    #[inline]
    pub fn into_leaf(self) -> CollectedChildren<Leaf, INLINE_SIZE> {
        #[cfg(debug_assertions)]
        {
            assert!(
                self.is_empty(),
                "Leaf arity expects no children, found {}",
                self.len()
            );
        }
        CollectedChildren {
            children: self.children,
            _arity: PhantomData,
        }
    }

    /// Validates the child count against a specific arity.
    ///
    /// Returns `true` if the count is valid for the arity.
    pub fn validate_arity<A: Arity>(&self) -> bool {
        A::validate_count(self.len())
    }

    /// Gets the runtime arity information for the collected children.
    ///
    /// This analyzes the child count and returns the most specific
    /// arity that matches.
    pub fn runtime_arity(&self) -> crate::arity::RuntimeArity {
        match self.len() {
            0 => crate::arity::RuntimeArity::Exact(0),
            1 => crate::arity::RuntimeArity::Exact(1),
            n => crate::arity::RuntimeArity::AtLeast(n),
        }
    }

    // ========================================================================
    // HRTB-COMPATIBLE OPERATIONS
    // ========================================================================

    /// Finds the first child matching an HRTB predicate.
    ///
    /// # Arguments
    ///
    /// * `predicate` - HRTB predicate that works with any lifetime
    ///
    /// # Returns
    ///
    /// The first matching child ID, or `None` if no match found.
    pub fn find_where<P>(&self, predicate: P) -> Option<ElementId>
    where
        P: for<'a> Fn(ElementId) -> bool,
    {
        self.children.iter().copied().find(|&id| predicate(id))
    }

    /// Filters children with an HRTB predicate.
    ///
    /// # Arguments
    ///
    /// * `predicate` - HRTB predicate that works with any lifetime
    ///
    /// # Returns
    ///
    /// New collector containing only matching children.
    pub fn filter_where<P>(&self, predicate: P) -> Self
    where
        P: for<'a> Fn(ElementId) -> bool,
    {
        let children = self
            .children
            .iter()
            .copied()
            .filter(|&id| predicate(id))
            .collect();
        Self { children }
    }

    /// Counts children matching an HRTB predicate.
    pub fn count_where<P>(&self, predicate: P) -> usize
    where
        P: for<'a> Fn(ElementId) -> bool,
    {
        self.children
            .iter()
            .copied()
            .filter(|&id| predicate(id))
            .count()
    }

    /// Executes an HRTB closure for each child.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: for<'a> FnMut(ElementId),
    {
        for &id in &self.children {
            f(id);
        }
    }
}

impl<const INLINE_SIZE: usize> Default for RenderChildrenCollector<INLINE_SIZE> {
    fn default() -> Self {
        Self {
            children: smallvec::SmallVec::new(),
        }
    }
}

impl<const INLINE_SIZE: usize> std::fmt::Debug for RenderChildrenCollector<INLINE_SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderChildrenCollector")
            .field("len", &self.len())
            .field("children", &self.children)
            .finish()
    }
}

// ============================================================================
// COLLECTED CHILDREN (TYPED ACCESSOR)
// ============================================================================

/// Typed accessor for collected render children with arity guarantees.
///
/// This type provides compile-time guarantees about child count based on
/// the arity type parameter. It's created by `RenderChildrenCollector`
/// conversion methods.
///
/// # Type Parameters
///
/// * `A` - The arity type (`Single`, `Optional`, `Variable`, etc.)
/// * `INLINE_SIZE` - Const generic for inline storage size
pub struct CollectedChildren<A: Arity, const INLINE_SIZE: usize = 32> {
    children: smallvec::SmallVec<[ElementId; INLINE_SIZE]>,
    _arity: PhantomData<A>,
}

impl<A: Arity, const INLINE_SIZE: usize> CollectedChildren<A, INLINE_SIZE> {
    /// Returns the number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns `true` if there are no children.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns a slice of the children.
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        &self.children
    }

    /// Converts to a different arity type.
    ///
    /// # Safety
    ///
    /// This is unchecked in release mode. The caller must ensure
    /// the child count is valid for the target arity.
    #[inline]
    pub fn into_arity<B: Arity>(self) -> CollectedChildren<B, INLINE_SIZE> {
        CollectedChildren {
            children: self.children,
            _arity: PhantomData,
        }
    }

    /// Returns an iterator over the children.
    #[inline]
    pub fn iter(&self) -> std::iter::Copied<std::slice::Iter<'_, ElementId>> {
        self.children.iter().copied()
    }

    /// Consumes and returns the underlying collection.
    #[inline]
    pub fn into_inner(self) -> smallvec::SmallVec<[ElementId; INLINE_SIZE]> {
        self.children
    }
}

// ============================================================================
// SINGLE ARITY METHODS
// ============================================================================

impl<const INLINE_SIZE: usize> CollectedChildren<Single, INLINE_SIZE> {
    /// Returns the single child.
    ///
    /// This method is only available for `Single` arity, providing
    /// compile-time guarantee that exactly one child exists.
    ///
    /// # Panics
    ///
    /// Panics if the accessor was created with an invalid child count
    /// (not exactly 1). This can happen if the tree state changed or
    /// if `into_arity()` was used incorrectly.
    #[inline]
    pub fn single(&self) -> ElementId {
        #[cfg(debug_assertions)]
        {
            debug_assert_eq!(self.len(), 1, "Single arity must have exactly 1 child");
        }
        #[cfg(not(debug_assertions))]
        {
            if self.children.is_empty() {
                panic!("Single arity accessor: expected exactly 1 child, found 0");
            }
            if self.children.len() > 1 {
                panic!("Single arity accessor: expected exactly 1 child, found {}", self.children.len());
            }
        }
        self.children[0]
    }

    /// Returns the single child (alternative name).
    #[inline]
    pub fn child(&self) -> ElementId {
        self.single()
    }
}

// ============================================================================
// OPTIONAL ARITY METHODS
// ============================================================================

impl<const INLINE_SIZE: usize> CollectedChildren<Optional, INLINE_SIZE> {
    /// Returns the optional child.
    ///
    /// This method is only available for `Optional` arity.
    #[inline]
    pub fn get(&self) -> Option<ElementId> {
        self.children.first().copied()
    }

    /// Returns the child if present (alternative name).
    #[inline]
    pub fn child(&self) -> Option<ElementId> {
        self.get()
    }

    /// Returns `true` if there is a child.
    #[inline]
    pub fn is_some(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns `true` if there is no child.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.children.is_empty()
    }
}

// ============================================================================
// VARIABLE ARITY METHODS
// ============================================================================

impl<const INLINE_SIZE: usize> CollectedChildren<Variable, INLINE_SIZE> {
    /// Returns the first child.
    #[inline]
    pub fn first(&self) -> Option<ElementId> {
        self.children.first().copied()
    }

    /// Returns the last child.
    #[inline]
    pub fn last(&self) -> Option<ElementId> {
        self.children.last().copied()
    }

    /// Returns the nth child.
    #[inline]
    pub fn nth(&self, n: usize) -> Option<ElementId> {
        self.children.get(n).copied()
    }

    /// Checks if a specific child is present.
    #[inline]
    pub fn contains(&self, id: ElementId) -> bool {
        self.children.contains(&id)
    }

    /// Returns the position of a child.
    #[inline]
    pub fn position(&self, id: ElementId) -> Option<usize> {
        self.children.iter().position(|&child| child == id)
    }

    /// Returns all children as a Vec.
    #[inline]
    pub fn collect(&self) -> Vec<ElementId> {
        self.children.to_vec()
    }
}

// ============================================================================
// LEAF ARITY METHODS
// ============================================================================

impl<const INLINE_SIZE: usize> CollectedChildren<Leaf, INLINE_SIZE> {
    /// Validates that no children exist.
    ///
    /// This method checks that the leaf accessor was created correctly.
    ///
    /// # Panics
    ///
    /// Panics if there are any children. This can happen if the accessor
    /// was created incorrectly or if the tree state changed.
    #[inline]
    pub fn validate_empty(&self) {
        #[cfg(debug_assertions)]
        {
            debug_assert!(self.is_empty(), "Leaf arity must have no children");
        }
        #[cfg(not(debug_assertions))]
        {
            if !self.is_empty() {
                panic!("Leaf arity accessor: expected no children, found {}", self.len());
            }
        }
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl<A: Arity, const INLINE_SIZE: usize> IntoIterator for CollectedChildren<A, INLINE_SIZE> {
    type Item = ElementId;
    type IntoIter = smallvec::IntoIter<[ElementId; INLINE_SIZE]>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.children.into_iter()
    }
}

impl<A: Arity, const INLINE_SIZE: usize> IntoIterator for &CollectedChildren<A, INLINE_SIZE> {
    type Item = ElementId;
    type IntoIter = std::iter::Copied<std::slice::Iter<'_, ElementId>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<A: Arity, const INLINE_SIZE: usize> std::fmt::Debug for CollectedChildren<A, INLINE_SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectedChildren")
            .field("arity", &std::any::type_name::<A>())
            .field("len", &self.len())
            .field("children", &self.children)
            .finish()
    }
}

impl<A: Arity, const INLINE_SIZE: usize> Clone for CollectedChildren<A, INLINE_SIZE> {
    fn clone(&self) -> Self {
        Self {
            children: self.children.clone(),
            _arity: PhantomData,
        }
    }
}

// ============================================================================
// CONVENIENCE TYPE ALIASES
// ============================================================================

/// Type alias for commonly used collector size.
pub type RenderChildrenCollector32 = RenderChildrenCollector<32>;

/// Type alias for small collectors.
pub type RenderChildrenCollector16 = RenderChildrenCollector<16>;

/// Type alias for large collectors.
pub type RenderChildrenCollector64 = RenderChildrenCollector<64>;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{TreeNav, TreeRead, RenderTreeAccess};
    use flui_foundation::Slot;
    use std::any::Any;

    // Test tree implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        is_render: bool,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, is_render: bool, parent: Option<ElementId>) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            let mut node = TestNode {
                parent,
                children: Vec::new(),
                is_render,
            };
            node.parent = parent;
            self.nodes.push(Some(node));

            if let Some(parent_id) = parent {
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    p.children.push(id);
                }
            }

            id
        }
    }

    impl TreeRead for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }
    }

    impl TreeNav for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> &[ElementId] {
            self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
        }

        fn slot(&self, _id: ElementId) -> Option<Slot> {
            None
        }
    }

    impl RenderTreeAccess for TestTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            if self.get(id)?.is_render {
                Some(&() as &dyn Any)
            } else {
                None
            }
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            if self.get(id)?.is_render {
                Some(&mut () as &mut dyn Any)
            } else {
                None
            }
        }

        fn render_state(&self, _id: ElementId) -> Option<&dyn Any> {
            None
        }

        fn render_state_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    #[test]
    fn test_collector_creation() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child1 = tree.insert(true, Some(parent));
        let child2 = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        assert_eq!(collector.len(), 2);
        assert!(!collector.is_empty());

        let children = collector.as_slice();
        assert!(children.contains(&child1));
        assert!(children.contains(&child2));
    }

    #[test]
    fn test_collector_with_components() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let component = tree.insert(false, Some(parent)); // Non-render
        let child = tree.insert(true, Some(component));

        let collector = RenderChildrenCollector::new(&tree, parent);
        assert_eq!(collector.len(), 1);
        assert_eq!(collector.as_slice()[0], child);
    }

    #[test]
    fn test_into_single() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        let single = collector.into_single();

        assert_eq!(single.single(), child);
        assert_eq!(single.child(), child);
    }

    #[test]
    fn test_into_optional() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        let optional = collector.into_optional();

        assert_eq!(optional.get(), Some(child));
        assert!(optional.is_some());
        assert!(!optional.is_none());

        // Test empty optional
        let empty_parent = tree.insert(true, None);
        let empty_collector = RenderChildrenCollector::new(&tree, empty_parent);
        let empty_optional = empty_collector.into_optional();

        assert_eq!(empty_optional.get(), None);
        assert!(!empty_optional.is_some());
        assert!(empty_optional.is_none());
    }

    #[test]
    fn test_into_variable() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child1 = tree.insert(true, Some(parent));
        let child2 = tree.insert(true, Some(parent));
        let child3 = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        let variable = collector.into_variable();

        assert_eq!(variable.len(), 3);
        assert_eq!(variable.first(), Some(child1));
        assert_eq!(variable.last(), Some(child3));
        assert_eq!(variable.nth(1), Some(child2));
        assert!(variable.contains(child2));
        assert_eq!(variable.position(child2), Some(1));
    }

    #[test]
    fn test_into_leaf() {
        let mut tree = TestTree::new();
        let leaf = tree.insert(true, None);

        let collector = RenderChildrenCollector::new(&tree, leaf);
        let leaf_accessor = collector.into_leaf();

        leaf_accessor.validate_empty(); // Should not panic
        assert!(leaf_accessor.is_empty());
    }

    #[test]
    fn test_try_into_arity() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);

        // Should succeed for Single
        let single = collector.try_into_arity::<Single>();
        assert!(single.is_some());

        // Should fail for multiple children
        let _child2 = tree.insert(true, Some(parent));
        let collector2 = RenderChildrenCollector::new(&tree, parent);
        let single2 = collector2.try_into_arity::<Single>();
        assert!(single2.is_none());
    }

    #[test]
    fn test_validate_arity() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let _child = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);

        assert!(collector.validate_arity::<Single>());
        assert!(collector.validate_arity::<Optional>());
        assert!(collector.validate_arity::<Variable>());
        assert!(!collector.validate_arity::<Leaf>());
    }

    #[test]
    fn test_runtime_arity() {
        let mut tree = TestTree::new();

        // Test empty
        let empty = tree.insert(true, None);
        let collector = RenderChildrenCollector::new(&tree, empty);
        assert_eq!(collector.runtime_arity(), crate::arity::RuntimeArity::Exact(0));

        // Test single
        let parent = tree.insert(true, None);
        let _child = tree.insert(true, Some(parent));
        let collector = RenderChildrenCollector::new(&tree, parent);
        assert_eq!(collector.runtime_arity(), crate::arity::RuntimeArity::Exact(1));

        // Test multiple
        let _child2 = tree.insert(true, Some(parent));
        let collector = RenderChildrenCollector::new(&tree, parent);
        assert_eq!(collector.runtime_arity(), crate::arity::RuntimeArity::AtLeast(2));
    }

    #[test]
    fn test_hrtb_operations() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child1 = tree.insert(true, Some(parent));
        let child2 = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);

        // Test HRTB find
        let found = collector.find_where(|id| id == child2);
        assert_eq!(found, Some(child2));

        // Test HRTB filter
        let filtered = collector.filter_where(|id| id == child1);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered.as_slice()[0], child1);

        // Test HRTB count
        let count = collector.count_where(|id| id.get() > child1.get());
        assert_eq!(count, 1);

        // Test HRTB for_each
        let mut visited = Vec::new();
        collector.for_each(|id| visited.push(id));
        assert_eq!(visited.len(), 2);
    }

    #[test]
    fn test_into_iter() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let child1 = tree.insert(true, Some(parent));
        let child2 = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        let variable = collector.into_variable();

        // Test owned iterator
        let owned: Vec<_> = variable.clone().into_iter().collect();
        assert_eq!(owned, vec![child1, child2]);

        // Test reference iterator
        let borrowed: Vec<_> = (&variable).into_iter().collect();
        assert_eq!(borrowed, vec![child1, child2]);
    }

    #[test]
    fn test_const_generics() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let _child = tree.insert(true, Some(parent));

        // Test different inline sizes
        let collector16 = RenderChildrenCollector::<16>::new(&tree, parent);
        let collector64 = RenderChildrenCollector::<64>::new(&tree, parent);

        assert_eq!(collector16.len(), 1);
        assert_eq!(collector64.len(), 1);
    }

    #[test]
    fn test_debug_format() {
        let mut tree = TestTree::new();
        let parent = tree.insert(true, None);
        let _child = tree.insert(true, Some(parent));

        let collector = RenderChildrenCollector::new(&tree, parent);
        let debug_str = format!("{:?}", collector);
        assert!(debug_str.contains("RenderChildrenCollector"));
        assert!(debug_str.contains("len"));

        let variable = collector.into_variable();
        let debug_str2 = format!("{:?}", variable);
        assert!(debug_str2.contains("CollectedChildren"));
        assert!(debug_str2.contains("arity"));
    }
}
