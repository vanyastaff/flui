//! Visitor composition for combining multiple visitors.
//!
//! This module provides utilities for composing multiple visitors
//! into a single traversal pass, improving efficiency.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{CountVisitor, MaxDepthVisitor, ComposedVisitor};
//!
//! let count = CountVisitor::new();
//! let depth = MaxDepthVisitor::new();
//!
//! // Combine visitors - both run in single traversal
//! let composed = ComposedVisitor::new(count, depth);
//! visit_depth_first(&tree, root, &mut composed);
//!
//! let (count_visitor, depth_visitor) = composed.into_parts();
//! println!("Count: {}, Max depth: {}", count_visitor.count, depth_visitor.max_depth);
//! ```

use super::{sealed, TreeVisitor, VisitorResult};
use crate::TreeNav;
use flui_foundation::TreeId;
use std::marker::PhantomData;

// ============================================================================
// COMPOSED VISITOR
// ============================================================================

/// A visitor that combines two visitors into one.
///
/// Both visitors are called for each node, allowing multiple
/// operations in a single tree traversal.
pub struct ComposedVisitor<A, B> {
    /// First visitor.
    pub first: A,
    /// Second visitor.
    pub second: B,
}

impl<A, B> ComposedVisitor<A, B> {
    /// Create a new composed visitor.
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    /// Decompose into individual visitors.
    pub fn into_parts(self) -> (A, B) {
        (self.first, self.second)
    }

    /// Get references to both visitors.
    pub fn parts(&self) -> (&A, &B) {
        (&self.first, &self.second)
    }

    /// Get mutable references to both visitors.
    pub fn parts_mut(&mut self) -> (&mut A, &mut B) {
        (&mut self.first, &mut self.second)
    }
}

impl<A, B> sealed::Sealed for ComposedVisitor<A, B> {}

impl<T, A, B> TreeVisitor<T> for ComposedVisitor<A, B>
where
    T: TreeNav,
    A: TreeVisitor<T>,
    B: TreeVisitor<T>,
{
    fn visit(&mut self, id: T::Id, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);

        // Combine results - most restrictive wins
        combine_results(result_a, result_b)
    }

    fn pre_children(&mut self, id: T::Id, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
    }

    fn post_children(&mut self, id: T::Id, depth: usize) {
        self.first.post_children(id, depth);
        self.second.post_children(id, depth);
    }
}

/// Combine two visitor results, taking the most restrictive.
fn combine_results(a: VisitorResult, b: VisitorResult) -> VisitorResult {
    // Priority order (most to least restrictive):
    // Stop > SkipSiblings > SkipChildren > Continue variants
    match (a, b) {
        (VisitorResult::Stop, _) | (_, VisitorResult::Stop) => VisitorResult::Stop,
        (VisitorResult::SkipSiblings, _) | (_, VisitorResult::SkipSiblings) => {
            VisitorResult::SkipSiblings
        }
        (VisitorResult::SkipChildren, _) | (_, VisitorResult::SkipChildren) => {
            VisitorResult::SkipChildren
        }
        (VisitorResult::ContinueDepthFirst, _) | (_, VisitorResult::ContinueDepthFirst) => {
            VisitorResult::ContinueDepthFirst
        }
        (VisitorResult::ContinueBreadthFirst, _) | (_, VisitorResult::ContinueBreadthFirst) => {
            VisitorResult::ContinueBreadthFirst
        }
        _ => VisitorResult::Continue,
    }
}

// ============================================================================
// TRIPLE COMPOSED VISITOR
// ============================================================================

/// A visitor that combines three visitors into one.
pub struct TripleComposedVisitor<A, B, C> {
    /// First visitor.
    pub first: A,
    /// Second visitor.
    pub second: B,
    /// Third visitor.
    pub third: C,
}

impl<A, B, C> TripleComposedVisitor<A, B, C> {
    /// Create a new triple composed visitor.
    pub fn new(first: A, second: B, third: C) -> Self {
        Self {
            first,
            second,
            third,
        }
    }

    /// Decompose into individual visitors.
    pub fn into_parts(self) -> (A, B, C) {
        (self.first, self.second, self.third)
    }
}

impl<A, B, C> sealed::Sealed for TripleComposedVisitor<A, B, C> {}

impl<T, A, B, C> TreeVisitor<T> for TripleComposedVisitor<A, B, C>
where
    T: TreeNav,
    A: TreeVisitor<T>,
    B: TreeVisitor<T>,
    C: TreeVisitor<T>,
{
    fn visit(&mut self, id: T::Id, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);
        let result_c = self.third.visit(id, depth);

        combine_results(combine_results(result_a, result_b), result_c)
    }

    fn pre_children(&mut self, id: T::Id, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
        self.third.pre_children(id, depth);
    }

    fn post_children(&mut self, id: T::Id, depth: usize) {
        self.first.post_children(id, depth);
        self.second.post_children(id, depth);
        self.third.post_children(id, depth);
    }
}

// ============================================================================
// VISITOR VEC
// ============================================================================

/// A dynamic collection of boxed visitors.
///
/// Useful when the number of visitors isn't known at compile time.
/// This type is generic over the ID type for the tree.
pub struct VisitorVec<Id: TreeId> {
    visitors: Vec<Box<dyn DynVisitor<Id>>>,
}

/// Object-safe visitor trait for dynamic dispatch.
///
/// Generic over ID type to support any tree.
pub trait DynVisitor<Id: TreeId>: Send + Sync {
    /// Visit a node.
    fn visit_dyn(&mut self, id: Id, depth: usize) -> VisitorResult;

    /// Pre-children hook.
    fn pre_children_dyn(&mut self, id: Id, depth: usize);

    /// Post-children hook.
    fn post_children_dyn(&mut self, id: Id, depth: usize);
}

// Note: We cannot provide a blanket impl of DynVisitor for TreeVisitor
// because the tree type T is not constrained. Users who need dynamic
// dispatch should implement DynVisitor manually for their visitor types.

impl<Id: TreeId> VisitorVec<Id> {
    /// Create an empty visitor collection.
    pub fn new() -> Self {
        Self {
            visitors: Vec::new(),
        }
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            visitors: Vec::with_capacity(capacity),
        }
    }

    /// Add a visitor to the collection.
    pub fn push(&mut self, visitor: Box<dyn DynVisitor<Id>>) {
        self.visitors.push(visitor);
    }

    /// Get the number of visitors.
    pub fn len(&self) -> usize {
        self.visitors.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.visitors.is_empty()
    }
}

impl<Id: TreeId> Default for VisitorVec<Id> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id: TreeId> sealed::Sealed for VisitorVec<Id> {}

impl<T: TreeNav> TreeVisitor<T> for VisitorVec<T::Id> {
    fn visit(&mut self, id: T::Id, depth: usize) -> VisitorResult {
        let mut result = VisitorResult::Continue;

        for visitor in &mut self.visitors {
            let visitor_result = visitor.visit_dyn(id, depth);
            result = combine_results(result, visitor_result);

            // Early exit on Stop
            if result == VisitorResult::Stop {
                break;
            }
        }

        result
    }

    fn pre_children(&mut self, id: T::Id, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.pre_children_dyn(id, depth);
        }
    }

    fn post_children(&mut self, id: T::Id, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.post_children_dyn(id, depth);
        }
    }
}

// ============================================================================
// CONDITIONAL VISITOR
// ============================================================================

/// A visitor that only visits nodes matching a predicate.
pub struct ConditionalVisitor<V, P, Id> {
    visitor: V,
    predicate: P,
    _marker: PhantomData<Id>,
}

impl<V, P, Id> ConditionalVisitor<V, P, Id> {
    /// Create a new conditional visitor.
    pub fn new(visitor: V, predicate: P) -> Self
    where
        Id: TreeId,
        P: FnMut(Id, usize) -> bool,
    {
        Self {
            visitor,
            predicate,
            _marker: PhantomData,
        }
    }

    /// Get the inner visitor.
    pub fn into_inner(self) -> V {
        self.visitor
    }

    /// Get reference to inner visitor.
    pub fn inner(&self) -> &V {
        &self.visitor
    }

    /// Get mutable reference to inner visitor.
    pub fn inner_mut(&mut self) -> &mut V {
        &mut self.visitor
    }
}

impl<V, P, Id> sealed::Sealed for ConditionalVisitor<V, P, Id> {}

impl<T, V, P> TreeVisitor<T> for ConditionalVisitor<V, P, T::Id>
where
    T: TreeNav,
    V: TreeVisitor<T>,
    P: FnMut(T::Id, usize) -> bool,
{
    fn visit(&mut self, id: T::Id, depth: usize) -> VisitorResult {
        if (self.predicate)(id, depth) {
            self.visitor.visit(id, depth)
        } else {
            VisitorResult::Continue
        }
    }

    fn pre_children(&mut self, id: T::Id, depth: usize) {
        self.visitor.pre_children(id, depth);
    }

    fn post_children(&mut self, id: T::Id, depth: usize) {
        self.visitor.post_children(id, depth);
    }
}

// ============================================================================
// MAPPED VISITOR
// ============================================================================

/// A visitor that transforms node IDs before visiting.
pub struct MappedVisitor<V, F, Id> {
    visitor: V,
    mapper: F,
    _marker: PhantomData<Id>,
}

impl<V, F, Id> MappedVisitor<V, F, Id> {
    /// Create a new mapped visitor.
    pub fn new(visitor: V, mapper: F) -> Self
    where
        Id: TreeId,
        F: FnMut(Id, usize) -> Id,
    {
        Self {
            visitor,
            mapper,
            _marker: PhantomData,
        }
    }

    /// Get the inner visitor.
    pub fn into_inner(self) -> V {
        self.visitor
    }
}

impl<V, F, Id> sealed::Sealed for MappedVisitor<V, F, Id> {}

impl<T, V, F> TreeVisitor<T> for MappedVisitor<V, F, T::Id>
where
    T: TreeNav,
    V: TreeVisitor<T>,
    F: FnMut(T::Id, usize) -> T::Id,
{
    fn visit(&mut self, id: T::Id, depth: usize) -> VisitorResult {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.visit(mapped_id, depth)
    }

    fn pre_children(&mut self, id: T::Id, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.pre_children(mapped_id, depth);
    }

    fn post_children(&mut self, id: T::Id, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.post_children(mapped_id, depth);
    }
}

// ============================================================================
// EXTENSION TRAIT FOR COMPOSITION
// ============================================================================

/// Extension trait for composing visitors.
pub trait VisitorExt<T: TreeNav>: TreeVisitor<T> + Sized {
    /// Compose with another visitor.
    fn and_then<V: TreeVisitor<T>>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Add a third visitor to composition.
    fn and_also<V: TreeVisitor<T>>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Only visit nodes matching predicate.
    fn filter<P>(self, predicate: P) -> ConditionalVisitor<Self, P, T::Id>
    where
        P: FnMut(T::Id, usize) -> bool,
    {
        ConditionalVisitor::new(self, predicate)
    }

    /// Transform IDs before visiting.
    fn map_ids<F>(self, mapper: F) -> MappedVisitor<Self, F, T::Id>
    where
        F: FnMut(T::Id, usize) -> T::Id,
    {
        MappedVisitor::new(self, mapper)
    }
}

// Blanket implementation
impl<T: TreeNav, V: TreeVisitor<T> + Sized> VisitorExt<T> for V {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visitor::{CollectVisitor, CountVisitor, MaxDepthVisitor};
    use crate::{TreeNav, TreeRead};
    use flui_foundation::ElementId;

    // Test tree implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    }

    struct TestTree {
        nodes: std::collections::HashMap<ElementId, TestNode>,
    }

    impl crate::traits::sealed::TreeReadSealed for TestTree {}
    impl crate::traits::sealed::TreeNavSealed for TestTree {}

    impl TestTree {
        fn new() -> Self {
            Self {
                nodes: std::collections::HashMap::new(),
            }
        }

        fn insert(&mut self, id: ElementId, parent: Option<ElementId>) {
            let node = TestNode {
                parent,
                children: Vec::new(),
            };

            if let Some(parent_id) = parent {
                if let Some(parent_node) = self.nodes.get_mut(&parent_id) {
                    parent_node.children.push(id);
                }
            }

            self.nodes.insert(id, node);
        }
    }

    impl TreeRead for TestTree {
        type Id = ElementId;
        type Node = TestNode;
        type NodeIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> Self::NodeIter<'_> {
            Box::new(self.nodes.keys().copied())
        }
    }

    impl TreeNav for TestTree {
        type ChildrenIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
        type AncestorsIter<'a> = crate::iter::Ancestors<'a, Self>;
        type DescendantsIter<'a> = crate::iter::DescendantsWithDepth<'a, Self>;
        type SiblingsIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
            if let Some(node) = self.get(id) {
                Box::new(node.children.iter().copied())
            } else {
                Box::new(std::iter::empty())
            }
        }

        fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
            crate::iter::Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
            crate::iter::DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
            if let Some(parent_id) = self.parent(id) {
                Box::new(
                    self.children(parent_id)
                        .filter(move |&child_id| child_id != id),
                )
            } else {
                Box::new(std::iter::empty())
            }
        }
    }

    #[test]
    fn test_combine_results() {
        assert_eq!(
            combine_results(VisitorResult::Continue, VisitorResult::Continue),
            VisitorResult::Continue
        );

        assert_eq!(
            combine_results(VisitorResult::Continue, VisitorResult::Stop),
            VisitorResult::Stop
        );

        assert_eq!(
            combine_results(VisitorResult::SkipChildren, VisitorResult::Continue),
            VisitorResult::SkipChildren
        );

        assert_eq!(
            combine_results(VisitorResult::SkipChildren, VisitorResult::Stop),
            VisitorResult::Stop
        );
    }

    #[test]
    fn test_composed_visitor() {
        let mut tree = TestTree::new();
        tree.insert(ElementId::new(1), None);

        let count = CountVisitor::new();
        let depth = MaxDepthVisitor::new();

        let mut composed = ComposedVisitor::new(count, depth);

        // Test visit method
        let result = <ComposedVisitor<_, _> as TreeVisitor<TestTree>>::visit(
            &mut composed,
            ElementId::new(1),
            0,
        );
        assert_eq!(result, VisitorResult::Continue);

        let (count, depth) = composed.into_parts();
        assert_eq!(count.count, 1);
        assert_eq!(depth.max_depth, 0);
    }

    #[test]
    fn test_visitor_vec() {
        let vec: VisitorVec<ElementId> = VisitorVec::new();
        assert!(vec.is_empty());
        assert_eq!(vec.len(), 0);
    }

    #[test]
    fn test_visitor_ext() {
        let mut tree = TestTree::new();
        tree.insert(ElementId::new(1), None);

        let count = CountVisitor::new();
        let depth = MaxDepthVisitor::new();

        // Test composition via extension trait - need to specify tree type
        let mut composed: ComposedVisitor<CountVisitor, MaxDepthVisitor> =
            <CountVisitor as VisitorExt<TestTree>>::and_then(count, depth);
        <ComposedVisitor<_, _> as TreeVisitor<TestTree>>::visit(
            &mut composed,
            ElementId::new(1),
            5,
        );

        let (c, d) = composed.into_parts();
        assert_eq!(c.count, 1);
        assert_eq!(d.max_depth, 5);
    }

    #[test]
    fn test_conditional_visitor() {
        let mut tree = TestTree::new();
        tree.insert(ElementId::new(1), None);
        tree.insert(ElementId::new(2), None);
        tree.insert(ElementId::new(3), None);
        tree.insert(ElementId::new(4), None);

        let collector: CollectVisitor<ElementId> = CollectVisitor::new();

        // Only collect even IDs
        let mut conditional: ConditionalVisitor<_, _, ElementId> =
            ConditionalVisitor::new(collector, |id: ElementId, _| id.get() % 2 == 0);

        <ConditionalVisitor<_, _, _> as TreeVisitor<TestTree>>::visit(
            &mut conditional,
            ElementId::new(1),
            0,
        ); // Odd - skip
        <ConditionalVisitor<_, _, _> as TreeVisitor<TestTree>>::visit(
            &mut conditional,
            ElementId::new(2),
            0,
        ); // Even - collect
        <ConditionalVisitor<_, _, _> as TreeVisitor<TestTree>>::visit(
            &mut conditional,
            ElementId::new(3),
            0,
        ); // Odd - skip
        <ConditionalVisitor<_, _, _> as TreeVisitor<TestTree>>::visit(
            &mut conditional,
            ElementId::new(4),
            0,
        ); // Even - collect

        let collector = conditional.into_inner();
        assert_eq!(collector.collected.len(), 2);
    }
}
