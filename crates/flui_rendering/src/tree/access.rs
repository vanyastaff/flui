//! Render tree access traits.
//!
//! This module provides traits for accessing render-specific data
//! (`RenderObject`, `RenderState`) from tree nodes, plus ergonomic
//! extension traits for common operations.
//!
//! # Design Rationale
//!
//! These traits allow `flui-rendering` to work with abstract tree
//! interfaces without depending on `flui-element`. The concrete
//! implementation is provided by `flui-pipeline`.
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderTreeAccess (base trait)
//!     │
//!     ├── RenderTreeAccessExt (typed downcast helpers)
//!     │
//!     └── RenderTreeExt (ergonomic operations)
//!             │
//!             ├── Iterator factories (zero-allocation)
//!             ├── Convenience methods
//!             └── Batch operations
//! ```
//!
//! # Arity Integration
//!
//! `RenderChildAccessor` uses the unified arity system from [`crate::arity`].
//! The arity markers (`Leaf`, `Single`, `Optional`, `Variable`) provide
//! compile-time guarantees about child counts.

use flui_foundation::ElementId;
use flui_tree::arity::{Arity, Leaf, Optional, Single, Variable};
use flui_tree::TreeNav;
use flui_types::{Offset, Size};
use std::any::Any;

// ============================================================================
// RENDER TREE ACCESS (BASE TRAIT)
// ============================================================================

/// Access to render-specific data in tree nodes.
///
/// This trait extends [`TreeNav`] with methods to access `RenderObject`
/// and `RenderState`. It uses `dyn Any` for type erasure, allowing
/// the trait to be defined without depending on concrete render types.
///
/// # Type Erasure Strategy
///
/// `RenderObject` and `RenderState` are accessed as `dyn Any` because:
/// 1. Keeps `flui-tree` independent of `flui-rendering` types
/// 2. Implementations downcast to concrete types as needed
/// 3. Enables generic algorithms over any render tree
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::RenderTreeAccess;
/// use flui_foundation::ElementId;
///
/// fn collect_render_elements<T: RenderTreeAccess>(
///     tree: &T,
///     root: ElementId,
/// ) -> Vec<ElementId> {
///     tree.descendants(root)
///         .filter(|&id| tree.is_render_element(id))
///         .collect()
/// }
/// ```
pub trait RenderTreeAccess: TreeNav<ElementId> {
    /// Returns the `RenderObject` for an element, if it's a render element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Reference to the `RenderObject` as `dyn Any`, or `None` if the
    /// element is not a render element.
    ///
    /// # Type Safety
    ///
    /// Callers should downcast using `downcast_ref::<ConcreteType>()`.
    fn render_object(&self, id: ElementId) -> Option<&dyn Any>;

    /// Returns a mutable reference to the `RenderObject`.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Mutable reference to the `RenderObject` as `dyn Any`, or `None`
    /// if the element is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Returns the `RenderState` for an element, if it's a render element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Reference to the `RenderState` as `dyn Any`, or `None` if the
    /// element is not a render element.
    fn render_state(&self, id: ElementId) -> Option<&dyn Any>;

    /// Returns a mutable reference to the `RenderState`.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Returns `true` if the element is a render element.
    ///
    /// A render element is one that participates in layout and paint.
    /// Non-render elements (like `StatelessView`) just compose other elements.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    #[inline]
    fn is_render_element(&self, id: ElementId) -> bool {
        self.render_object(id).is_some()
    }

    /// Returns the size from `RenderState`, if available.
    ///
    /// This is a convenience method that accesses the cached size
    /// from the `RenderState`. Returns `None` if:
    /// - Element doesn't exist
    /// - Element is not a render element
    /// - Size hasn't been computed yet
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Note
    ///
    /// Default implementation returns `None`. Concrete implementations
    /// should override to extract size from `RenderState`.
    #[inline]
    fn size(&self, id: ElementId) -> Option<Size> {
        let _ = id;
        None
    }

    /// Returns the cached constraints from `RenderState`, if available.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Note
    ///
    /// Default implementation returns `None`. Concrete implementations
    /// should override to extract constraints from `RenderState`.
    #[inline]
    fn constraints(&self, id: ElementId) -> Option<&dyn Any> {
        let _ = id;
        None
    }

    /// Returns the offset from `RenderState`, if available.
    ///
    /// The offset is the position relative to the parent.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    #[inline]
    fn offset(&self, id: ElementId) -> Option<Offset> {
        let _ = id;
        None
    }
}

// ============================================================================
// TYPED ACCESS EXTENSION
// ============================================================================

/// Extension trait for typed access to render data.
///
/// This trait provides type-safe access when the caller knows the
/// concrete types. Automatically implemented for all `RenderTreeAccess`.
pub trait RenderTreeAccessExt: RenderTreeAccess {
    /// Gets the `RenderObject` with a specific type.
    ///
    /// # Type Parameters
    ///
    /// * `R` - The expected `RenderObject` type
    ///
    /// # Returns
    ///
    /// Reference to the `RenderObject` if it exists and has the correct type.
    #[inline]
    fn render_object_typed<R: 'static>(&self, id: ElementId) -> Option<&R> {
        self.render_object(id)?.downcast_ref::<R>()
    }

    /// Gets the `RenderObject` mutably with a specific type.
    #[inline]
    fn render_object_typed_mut<R: 'static>(&mut self, id: ElementId) -> Option<&mut R> {
        self.render_object_mut(id)?.downcast_mut::<R>()
    }

    /// Gets the `RenderState` with a specific type.
    #[inline]
    fn render_state_typed<S: 'static>(&self, id: ElementId) -> Option<&S> {
        self.render_state(id)?.downcast_ref::<S>()
    }

    /// Gets the `RenderState` mutably with a specific type.
    #[inline]
    fn render_state_typed_mut<S: 'static>(&mut self, id: ElementId) -> Option<&mut S> {
        self.render_state_mut(id)?.downcast_mut::<S>()
    }
}

// Blanket implementation for all RenderTreeAccess implementors
impl<T: RenderTreeAccess + ?Sized> RenderTreeAccessExt for T {}

// ============================================================================
// RENDER TREE EXTENSION (ERGONOMIC OPERATIONS)
// ============================================================================

/// Extension trait providing ergonomic render tree operations.
///
/// This trait is automatically implemented for any type that implements
/// `RenderTreeAccess`. It provides:
///
/// - **Iterator-based traversal** (zero allocation)
/// - **Convenience methods** for common patterns
/// - **Batch operations** for efficiency
///
/// # Performance
///
/// All iterator methods return lazy iterators. No allocations occur
/// until you collect or consume the iterator. For methods that return
/// `Vec`, use the `_iter` variant when you don't need random access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::RenderTreeExt;
///
/// fn layout_children<T: RenderTreeExt>(tree: &T, parent: ElementId) {
///     // Zero-allocation iteration over render children
///     for child in tree.render_children_iter(parent) {
///         // Layout each child...
///     }
/// }
/// ```
pub trait RenderTreeExt: RenderTreeAccess {
    // ========================================================================
    // ITERATOR FACTORIES (Zero-Allocation)
    // ========================================================================

    /// Returns an iterator over render children of an element.
    ///
    /// This is the primary method for layout - finding immediate render
    /// children while skipping non-render wrapper elements.
    ///
    /// # Zero-Allocation
    ///
    /// The iterator uses a small stack-allocated buffer for most trees.
    /// Only deep nesting (>8 levels of non-render wrappers) triggers allocation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for child_id in tree.render_children_iter(parent_id) {
    ///     let size = layout_child(child_id, constraints);
    /// }
    /// ```
    #[inline]
    fn render_children_iter(&self, parent: ElementId) -> super::iter::RenderChildren<'_, Self>
    where
        Self: Sized,
    {
        super::iter::RenderChildren::new(self, parent)
    }

    /// Returns an iterator over render ancestors (including self).
    ///
    /// Useful for finding the render parent or propagating dirty flags up.
    #[inline]
    fn render_ancestors_iter(&self, start: ElementId) -> super::iter::RenderAncestors<'_, Self>
    where
        Self: Sized,
    {
        super::iter::RenderAncestors::new(self, start)
    }

    /// Returns an iterator over all render descendants.
    ///
    /// Pre-order traversal that skips non-render elements but continues
    /// into their children.
    #[inline]
    fn render_descendants_iter(&self, root: ElementId) -> super::iter::RenderDescendants<'_, Self>
    where
        Self: Sized,
    {
        super::iter::RenderDescendants::new(self, root)
    }

    // ========================================================================
    // CONVENIENCE METHODS
    // ========================================================================

    /// Finds the render parent of an element.
    ///
    /// Returns the first render ancestor (excluding self), or None if
    /// there is no render parent.
    #[inline]
    fn render_parent(&self, id: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        // Use from_parent to start from parent, avoiding nth(1) issues
        // when the element itself is not a render element
        super::iter::RenderAncestors::from_parent(self, id).next()
    }

    /// Counts the render children of an element.
    ///
    /// More efficient than collecting to Vec when you only need the count.
    #[inline]
    fn render_child_count(&self, parent: ElementId) -> usize
    where
        Self: Sized,
    {
        self.render_children_iter(parent).count()
    }

    /// Returns the first render child, if any.
    #[inline]
    fn first_render_child(&self, parent: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        self.render_children_iter(parent).next()
    }

    /// Returns the single render child, or None if zero or multiple.
    ///
    /// Useful for single-child render objects like Padding, Align, etc.
    #[inline]
    fn single_render_child(&self, parent: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        let mut iter = self.render_children_iter(parent);
        let first = iter.next()?;

        // Ensure there's no second child
        if iter.next().is_some() {
            return None;
        }

        Some(first)
    }

    /// Checks if an element has any render children.
    #[inline]
    fn has_render_children(&self, parent: ElementId) -> bool
    where
        Self: Sized,
    {
        self.render_children_iter(parent).next().is_some()
    }

    /// Returns the render depth of an element.
    ///
    /// This counts only render ancestors, not all ancestors.
    #[inline]
    fn render_depth(&self, id: ElementId) -> usize
    where
        Self: Sized,
    {
        self.render_ancestors_iter(id).count().saturating_sub(1)
    }

    /// Checks if `descendant` is a render descendant of `ancestor`.
    #[inline]
    fn is_render_descendant(&self, descendant: ElementId, ancestor: ElementId) -> bool
    where
        Self: Sized,
    {
        if descendant == ancestor {
            return false;
        }
        self.render_ancestors_iter(descendant)
            .skip(1)
            .any(|id| id == ancestor)
    }

    /// Finds the lowest common render ancestor of two elements.
    fn lowest_common_render_ancestor(&self, a: ElementId, b: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        // Collect render ancestors of 'a' (small allocation, typically <10 elements)
        let ancestors_a: Vec<_> = self.render_ancestors_iter(a).collect();

        // Find first match in render ancestors of 'b'
        self.render_ancestors_iter(b)
            .find(|id| ancestors_a.contains(id))
    }

    // ========================================================================
    // BATCH OPERATIONS
    // ========================================================================

    /// Collects render children into a Vec.
    ///
    /// Use this when you need random access or multiple iterations.
    /// For single iteration, prefer `render_children_iter()`.
    #[inline]
    fn collect_render_children(&self, parent: ElementId) -> Vec<ElementId>
    where
        Self: Sized,
    {
        self.render_children_iter(parent).collect()
    }

    /// Collects render descendants into a Vec.
    #[inline]
    fn collect_render_descendants(&self, root: ElementId) -> Vec<ElementId>
    where
        Self: Sized,
    {
        self.render_descendants_iter(root).collect()
    }

    /// Applies a function to each render child.
    ///
    /// Returns early if the function returns `false`.
    #[inline]
    fn for_each_render_child<F>(&self, parent: ElementId, mut f: F) -> bool
    where
        Self: Sized,
        F: FnMut(ElementId) -> bool,
    {
        for child in self.render_children_iter(parent) {
            if !f(child) {
                return false;
            }
        }
        true
    }

    /// Maps render children with a function, collecting results.
    #[inline]
    fn map_render_children<F, R>(&self, parent: ElementId, f: F) -> Vec<R>
    where
        Self: Sized,
        F: FnMut(ElementId) -> R,
    {
        self.render_children_iter(parent).map(f).collect()
    }

    /// Filters render children that satisfy a predicate.
    #[inline]
    fn filter_render_children<F>(&self, parent: ElementId, predicate: F) -> Vec<ElementId>
    where
        Self: Sized,
        F: FnMut(&ElementId) -> bool,
    {
        self.render_children_iter(parent)
            .filter(predicate)
            .collect()
    }

    /// Returns render children sorted by a key.
    #[inline]
    fn sorted_render_children_by_key<F, K>(&self, parent: ElementId, f: F) -> Vec<ElementId>
    where
        Self: Sized,
        F: FnMut(&ElementId) -> K,
        K: Ord,
    {
        let mut children: Vec<_> = self.render_children_iter(parent).collect();
        children.sort_by_key(f);
        children
    }
}

// Blanket implementation for all RenderTreeAccess implementors
impl<T: RenderTreeAccess + ?Sized> RenderTreeExt for T {}

// ============================================================================
// RENDER CHILD ACCESSOR - TYPE STATE PATTERN
// ============================================================================

/// A typed accessor for render children with compile-time arity guarantees.
///
/// Uses the Type State pattern to provide different APIs based on expected
/// child count. This moves runtime assertions to compile-time type checking.
///
/// # Type States
///
/// Uses markers from the unified [`crate::arity`] module:
///
/// - [`Leaf`]: No children expected. Only provides validation.
/// - [`Single`]: Exactly one child expected. Provides `child()` method.
/// - [`Optional`]: Zero or one child expected. Provides `child()` returning `Option`.
/// - [`Variable`] (aliased as `Multi`): Any number of children. Provides full iterator API.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{RenderChildAccessor, Single, Optional, Variable};
///
/// // For Single arity render objects (e.g., Padding, Align):
/// let accessor: RenderChildAccessor<_, Single> = RenderChildAccessor::single(&tree, parent_id);
/// let child = accessor.child(); // Guaranteed to exist
///
/// // For Optional arity (e.g., Container):
/// let accessor: RenderChildAccessor<_, Optional> = RenderChildAccessor::optional(&tree, parent_id);
/// if let Some(child) = accessor.child() {
///     // ...
/// }
///
/// // For Variable/Multi arity (e.g., Flex, Stack):
/// let accessor: RenderChildAccessor<_, Variable> = RenderChildAccessor::multi(&tree, parent_id);
/// for child in accessor.iter() {
///     // ...
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RenderChildAccessor<'a, T: RenderTreeAccess, A: Arity = Variable> {
    tree: &'a T,
    parent: ElementId,
    _arity: std::marker::PhantomData<A>,
}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl<'a, T: RenderTreeAccess> RenderChildAccessor<'a, T, Variable> {
    /// Creates a new variable-child accessor (default).
    ///
    /// Use this for render objects with variable child count (Flex, Stack, etc.).
    #[inline]
    pub const fn new(tree: &'a T, parent: ElementId) -> Self {
        Self {
            tree,
            parent,
            _arity: std::marker::PhantomData,
        }
    }

    /// Alias for `new()` - creates a multi-child accessor.
    #[inline]
    pub const fn multi(tree: &'a T, parent: ElementId) -> Self {
        Self::new(tree, parent)
    }

    /// Alias for `new()` - creates a variable-child accessor.
    #[inline]
    pub const fn variable(tree: &'a T, parent: ElementId) -> Self {
        Self::new(tree, parent)
    }
}

impl<'a, T: RenderTreeAccess> RenderChildAccessor<'a, T, Single> {
    /// Creates a single-child accessor.
    ///
    /// Use this for render objects that always have exactly one child
    /// (Padding, Align, Transform, etc.).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if there are zero or multiple children.
    #[inline]
    pub fn single(tree: &'a T, parent: ElementId) -> Self {
        #[cfg(debug_assertions)]
        {
            let count = super::iter::RenderChildren::new(tree, parent).count();
            assert!(
                count == 1,
                "Single arity requires exactly one child, found {count}"
            );
        }
        Self {
            tree,
            parent,
            _arity: std::marker::PhantomData,
        }
    }
}

impl<'a, T: RenderTreeAccess> RenderChildAccessor<'a, T, Optional> {
    /// Creates an optional-child accessor.
    ///
    /// Use this for render objects that have zero or one child (Container, etc.).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if there are multiple children.
    #[inline]
    pub fn optional(tree: &'a T, parent: ElementId) -> Self {
        #[cfg(debug_assertions)]
        {
            let count = super::iter::RenderChildren::new(tree, parent).count();
            assert!(
                count <= 1,
                "Optional arity allows at most one child, found {count}"
            );
        }
        Self {
            tree,
            parent,
            _arity: std::marker::PhantomData,
        }
    }
}

impl<'a, T: RenderTreeAccess> RenderChildAccessor<'a, T, Leaf> {
    /// Creates a leaf accessor (no children expected).
    ///
    /// Use this for render objects that should never have children
    /// (Text, Image, etc.).
    ///
    /// # Panics
    ///
    /// Panics in debug mode if there are any children.
    #[inline]
    pub fn leaf(tree: &'a T, parent: ElementId) -> Self {
        #[cfg(debug_assertions)]
        {
            let count = super::iter::RenderChildren::new(tree, parent).count();
            assert!(count == 0, "Leaf arity expects no children, found {count}");
        }
        Self {
            tree,
            parent,
            _arity: std::marker::PhantomData,
        }
    }
}

// ============================================================================
// COMMON METHODS (all arities)
// ============================================================================

impl<'a, T: RenderTreeAccess, A: Arity> RenderChildAccessor<'a, T, A> {
    /// Returns the tree reference.
    #[inline]
    pub const fn tree(&self) -> &'a T {
        self.tree
    }

    /// Returns the parent element ID.
    #[inline]
    pub const fn parent(&self) -> ElementId {
        self.parent
    }

    /// Converts to a different arity type.
    ///
    /// # Safety
    ///
    /// This is unchecked in release mode. Use the typed constructors
    /// (`single()`, `optional()`, `leaf()`) for validation.
    #[inline]
    pub fn into_arity<B: Arity>(self) -> RenderChildAccessor<'a, T, B> {
        RenderChildAccessor {
            tree: self.tree,
            parent: self.parent,
            _arity: std::marker::PhantomData,
        }
    }
}

// ============================================================================
// SINGLE ARITY METHODS
// ============================================================================

impl<T: RenderTreeAccess> RenderChildAccessor<'_, T, Single> {
    /// Returns the single render child.
    ///
    /// This method is only available for `Single` arity accessors,
    /// providing compile-time guarantee that a child exists.
    ///
    /// # Panics
    ///
    /// Panics if the tree state has changed since the accessor was created
    /// and there is no longer exactly one render child. This can happen if
    /// the tree was modified between accessor creation and this call.
    #[inline]
    pub fn child(&self) -> ElementId {
        let mut iter = super::iter::RenderChildren::new(self.tree, self.parent);
        match iter.next() {
            Some(child) => {
                // In debug mode, verify there's exactly one child
                #[cfg(debug_assertions)]
                {
                    assert!(
                        iter.next().is_none(),
                        "Single arity accessor found multiple children - tree state changed"
                    );
                }
                child
            }
            None => {
                #[cfg(debug_assertions)]
                panic!(
                    "Single arity accessor was constructed with no children - tree state changed"
                );
                #[cfg(not(debug_assertions))]
                panic!("Single arity accessor: expected exactly one child, found none");
            }
        }
    }
}

// ============================================================================
// OPTIONAL ARITY METHODS
// ============================================================================

impl<T: RenderTreeAccess> RenderChildAccessor<'_, T, Optional> {
    /// Returns the optional render child.
    ///
    /// This method is only available for `Optional` arity accessors.
    #[inline]
    pub fn child(&self) -> Option<ElementId> {
        super::iter::RenderChildren::new(self.tree, self.parent).next()
    }

    /// Returns `true` if there is a child.
    #[inline]
    pub fn has_child(&self) -> bool {
        self.child().is_some()
    }
}

// ============================================================================
// VARIABLE (MULTI) ARITY METHODS
// ============================================================================

impl<'a, T: RenderTreeAccess> RenderChildAccessor<'a, T, Variable> {
    /// Returns an iterator over render children.
    #[inline]
    pub fn iter(&self) -> super::iter::RenderChildren<'a, T> {
        super::iter::RenderChildren::new(self.tree, self.parent)
    }

    /// Returns the count of render children.
    #[inline]
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    /// Returns `true` if there are no render children.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Returns the first render child.
    #[inline]
    pub fn first(&self) -> Option<ElementId> {
        self.iter().next()
    }

    /// Returns the last render child.
    #[inline]
    pub fn last(&self) -> Option<ElementId> {
        self.iter().last()
    }

    /// Returns the nth render child.
    #[inline]
    pub fn nth(&self, n: usize) -> Option<ElementId> {
        self.iter().nth(n)
    }

    /// Collects all render children into a Vec.
    ///
    /// Note: This allocates. For zero-allocation iteration, use [`iter()`](Self::iter).
    #[inline]
    pub fn collect(&self) -> Vec<ElementId> {
        self.iter().collect()
    }

    /// Checks if a specific element is a render child.
    #[inline]
    pub fn contains(&self, id: ElementId) -> bool {
        self.iter().any(|child| child == id)
    }

    /// Returns the index of a render child, if found.
    #[inline]
    pub fn position(&self, id: ElementId) -> Option<usize> {
        self.iter().position(|child| child == id)
    }
}

impl<'a, T: RenderTreeAccess> IntoIterator for RenderChildAccessor<'a, T, Variable> {
    type Item = ElementId;
    type IntoIter = super::iter::RenderChildren<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: RenderTreeAccess> IntoIterator for &RenderChildAccessor<'a, T, Variable> {
    type Item = ElementId;
    type IntoIter = super::iter::RenderChildren<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_tree::iter::{Ancestors, DescendantsWithDepth};

    use flui_tree::{TreeNav, TreeRead};

    // Mock render object
    #[derive(Debug)]
    struct MockRenderObject {
        width: f32,
        height: f32,
    }

    // Mock render state
    #[derive(Debug)]
    struct MockRenderState {
        size: (f32, f32),
        needs_layout: bool,
    }

    // Test node
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        render_object: Option<MockRenderObject>,
        render_state: Option<MockRenderState>,
    }

    impl TestNode {
        fn new_render(width: f32, height: f32) -> Self {
            Self {
                parent: None,
                children: Vec::new(),
                render_object: Some(MockRenderObject { width, height }),
                render_state: Some(MockRenderState {
                    size: (width, height),
                    needs_layout: false,
                }),
            }
        }

        fn new_component() -> Self {
            Self {
                parent: None,
                children: Vec::new(),
                render_object: None,
                render_state: None,
            }
        }
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, mut node: TestNode, parent: Option<ElementId>) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            node.parent = parent;
            self.nodes.push(Some(node));

            if let Some(parent_id) = parent {
                if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    parent_node.children.push(id);
                }
            }

            id
        }

        fn insert_render(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(TestNode::new_render(100.0, 50.0), parent)
        }

        fn insert_component(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(TestNode::new_component(), parent)
        }
    }

    impl TreeRead<ElementId> for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            (0..self.nodes.len()).filter_map(|i| {
                if self.nodes[i].is_some() {
                    Some(ElementId::new(i + 1))
                } else {
                    None
                }
            })
        }
    }

    impl TreeNav<ElementId> for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            self.get(id)
                .map(|node| node.children.iter().copied())
                .into_iter()
                .flatten()
        }

        fn ancestors(&self, start: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent = self.parent(id);
            parent
                .into_iter()
                .flat_map(move |p| self.children(p).filter(move |&c| c != id))
        }
    }

    impl RenderTreeAccess for TestTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            self.get(id)?.render_object.as_ref().map(|r| r as &dyn Any)
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            self.nodes
                .get_mut(id.get() as usize - 1)?
                .as_mut()?
                .render_object
                .as_mut()
                .map(|r| r as &mut dyn Any)
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            self.get(id)?.render_state.as_ref().map(|s| s as &dyn Any)
        }

        fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            self.nodes
                .get_mut(id.get() as usize - 1)?
                .as_mut()?
                .render_state
                .as_mut()
                .map(|s| s as &mut dyn Any)
        }

        fn size(&self, id: ElementId) -> Option<Size> {
            self.get(id)?
                .render_state
                .as_ref()
                .map(|s| Size::new(s.size.0, s.size.1))
        }
    }

    // ========================================================================
    // BASE TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_is_render_element() {
        let mut tree = TestTree::new();
        let render = tree.insert_render(None);
        let component = tree.insert_component(None);

        assert!(tree.is_render_element(render));
        assert!(!tree.is_render_element(component));
    }

    #[test]
    fn test_render_object_typed() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode::new_render(100.0, 50.0), None);

        let obj = tree.render_object_typed::<MockRenderObject>(id).unwrap();
        assert_eq!(obj.width, 100.0);
        assert_eq!(obj.height, 50.0);
    }

    #[test]
    fn test_render_state_typed() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode::new_render(100.0, 50.0), None);

        let state = tree.render_state_typed::<MockRenderState>(id).unwrap();
        assert_eq!(state.size, (100.0, 50.0));
        assert!(!state.needs_layout);
    }

    // ========================================================================
    // EXTENSION TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_render_parent() {
        let mut tree = TestTree::new();

        // Build tree: render1 -> component -> render2
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        // render2's render parent should be render1 (skipping component)
        assert_eq!(tree.render_parent(render2), Some(render1));

        // component's render parent should be render1
        assert_eq!(tree.render_parent(component), Some(render1));

        // render1 has no render parent
        assert_eq!(tree.render_parent(render1), None);
    }

    #[test]
    fn test_render_children_iter() {
        let mut tree = TestTree::new();

        // Build tree: render1 -> [component -> [render2, render3], render4]
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(component));
        let render4 = tree.insert_render(Some(render1));

        let children: Vec<_> = tree.render_children_iter(render1).collect();
        assert_eq!(children.len(), 3);
        assert!(children.contains(&render2));
        assert!(children.contains(&render3));
        assert!(children.contains(&render4));
    }

    #[test]
    fn test_single_render_child() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let component = tree.insert_component(Some(parent));
        let child = tree.insert_render(Some(component));

        assert_eq!(tree.single_render_child(parent), Some(child));
    }

    #[test]
    fn test_single_render_child_multiple() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let _child1 = tree.insert_render(Some(parent));
        let _child2 = tree.insert_render(Some(parent));

        // Multiple children -> None
        assert_eq!(tree.single_render_child(parent), None);
    }

    #[test]
    fn test_render_depth() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render2));

        assert_eq!(tree.render_depth(render1), 0);
        assert_eq!(tree.render_depth(render2), 1);
        assert_eq!(tree.render_depth(render3), 2);
    }

    #[test]
    fn test_is_render_descendant() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render2));

        assert!(tree.is_render_descendant(render3, render1));
        assert!(tree.is_render_descendant(render2, render1));
        assert!(!tree.is_render_descendant(render1, render3));
        assert!(!tree.is_render_descendant(render1, render1));
    }

    #[test]
    fn test_lowest_common_render_ancestor() {
        let mut tree = TestTree::new();

        let root = tree.insert_render(None);
        let left = tree.insert_render(Some(root));
        let right = tree.insert_render(Some(root));
        let left_child = tree.insert_render(Some(left));

        assert_eq!(
            tree.lowest_common_render_ancestor(left_child, right),
            Some(root)
        );
        assert_eq!(tree.lowest_common_render_ancestor(left, right), Some(root));
        assert_eq!(
            tree.lowest_common_render_ancestor(left_child, left),
            Some(left)
        );
    }

    // ========================================================================
    // RENDER CHILD ACCESSOR TESTS
    // ========================================================================

    #[test]
    fn test_render_child_accessor_multi() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let component = tree.insert_component(Some(parent));
        let child = tree.insert_render(Some(component));

        let accessor = RenderChildAccessor::new(&tree, parent);
        assert_eq!(accessor.count(), 1);
        assert!(!accessor.is_empty());
        assert_eq!(accessor.first(), Some(child));
        assert!(accessor.contains(child));
        assert_eq!(accessor.position(child), Some(0));
    }

    #[test]
    fn test_render_child_accessor_single() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let component = tree.insert_component(Some(parent));
        let child = tree.insert_render(Some(component));

        // Single arity accessor
        let accessor = RenderChildAccessor::single(&tree, parent);
        assert_eq!(accessor.child(), child);
    }

    #[test]
    fn test_render_child_accessor_optional() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let component = tree.insert_component(Some(parent));
        let child = tree.insert_render(Some(component));

        // Optional arity accessor with child
        let accessor = RenderChildAccessor::optional(&tree, parent);
        assert_eq!(accessor.child(), Some(child));
        assert!(accessor.has_child());

        // Optional arity accessor without child
        let empty_parent = tree.insert_render(None);
        let empty_accessor = RenderChildAccessor::optional(&tree, empty_parent);
        assert_eq!(empty_accessor.child(), None);
        assert!(!empty_accessor.has_child());
    }

    #[test]
    fn test_render_child_accessor_leaf() {
        let mut tree = TestTree::new();

        let leaf = tree.insert_render(None);

        // Leaf arity accessor
        let accessor = RenderChildAccessor::leaf(&tree, leaf);
        assert_eq!(accessor.parent(), leaf);
    }

    #[test]
    fn test_render_child_accessor_multiple() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child1 = tree.insert_render(Some(parent));
        let child2 = tree.insert_render(Some(parent));
        let child3 = tree.insert_render(Some(parent));

        let accessor = RenderChildAccessor::new(&tree, parent);
        assert_eq!(accessor.count(), 3);
        assert_eq!(accessor.first(), Some(child1));
        assert_eq!(accessor.last(), Some(child3));
        assert_eq!(accessor.nth(1), Some(child2));

        let collected: Vec<_> = accessor.collect();
        assert_eq!(collected, vec![child1, child2, child3]);
    }

    #[test]
    fn test_render_child_accessor_into_iter() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child1 = tree.insert_render(Some(parent));
        let child2 = tree.insert_render(Some(parent));

        let accessor = RenderChildAccessor::new(&tree, parent);

        // Test IntoIterator for owned accessor
        let collected: Vec<_> = accessor.into_iter().collect();
        assert_eq!(collected, vec![child1, child2]);

        // Test IntoIterator for reference
        let accessor2 = RenderChildAccessor::new(&tree, parent);
        let collected2: Vec<_> = (&accessor2).into_iter().collect();
        assert_eq!(collected2, vec![child1, child2]);
    }

    #[test]
    fn test_render_child_accessor_into_arity() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child = tree.insert_render(Some(parent));

        // Start with Multi, convert to Single
        let multi = RenderChildAccessor::new(&tree, parent);
        let single: RenderChildAccessor<'_, _, Single> = multi.into_arity();
        assert_eq!(single.child(), child);
    }

    // ========================================================================
    // BATCH OPERATION TESTS
    // ========================================================================

    #[test]
    fn test_for_each_render_child() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let _child1 = tree.insert_render(Some(parent));
        let _child2 = tree.insert_render(Some(parent));

        let mut count = 0;
        let completed = tree.for_each_render_child(parent, |_| {
            count += 1;
            true
        });

        assert!(completed);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_for_each_render_child_early_exit() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let _child1 = tree.insert_render(Some(parent));
        let _child2 = tree.insert_render(Some(parent));
        let _child3 = tree.insert_render(Some(parent));

        let mut count = 0;
        let completed = tree.for_each_render_child(parent, |_| {
            count += 1;
            count < 2 // Stop after 2
        });

        assert!(!completed);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_map_render_children() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child1 = tree.insert_render(Some(parent));
        let child2 = tree.insert_render(Some(parent));

        let ids: Vec<usize> = tree.map_render_children(parent, |id| id.get());
        assert_eq!(ids, vec![child1.get(), child2.get()]);
    }

    #[test]
    fn test_filter_render_children() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child1 = tree.insert_render(Some(parent));
        let child2 = tree.insert_render(Some(parent));
        let _child3 = tree.insert_render(Some(parent));

        let filtered = tree.filter_render_children(parent, |&id| id == child1 || id == child2);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&child1));
        assert!(filtered.contains(&child2));
    }

    #[test]
    fn test_sorted_render_children_by_key() {
        let mut tree = TestTree::new();

        let parent = tree.insert_render(None);
        let child3 = tree.insert_render(Some(parent)); // id = 2
        let child1 = tree.insert_render(Some(parent)); // id = 3
        let child2 = tree.insert_render(Some(parent)); // id = 4

        // Sort by id descending
        let sorted = tree.sorted_render_children_by_key(parent, |&id| std::cmp::Reverse(id.get()));
        assert_eq!(sorted, vec![child2, child1, child3]);
    }
}
