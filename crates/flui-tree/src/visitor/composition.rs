//! Visitor composition for combining multiple visitors.
//!
//! This module provides utilities for composing multiple visitors
//! into a single traversal pass, improving efficiency.

use super::{sealed, TreeVisitor, VisitorResult};
use crate::TreeNav;
use flui_foundation::Identifier;
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

impl<I, T, A, B> TreeVisitor<I, T> for ComposedVisitor<A, B>
where
    I: Identifier,
    T: TreeNav<I>,
    A: TreeVisitor<I, T>,
    B: TreeVisitor<I, T>,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);

        // Combine results - most restrictive wins
        combine_results(result_a, result_b)
    }

    fn pre_children(&mut self, id: I, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
    }

    fn post_children(&mut self, id: I, depth: usize) {
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

impl<I, T, A, B, C> TreeVisitor<I, T> for TripleComposedVisitor<A, B, C>
where
    I: Identifier,
    T: TreeNav<I>,
    A: TreeVisitor<I, T>,
    B: TreeVisitor<I, T>,
    C: TreeVisitor<I, T>,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);
        let result_c = self.third.visit(id, depth);

        combine_results(combine_results(result_a, result_b), result_c)
    }

    fn pre_children(&mut self, id: I, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
        self.third.pre_children(id, depth);
    }

    fn post_children(&mut self, id: I, depth: usize) {
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
pub struct VisitorVec<I: Identifier> {
    visitors: Vec<Box<dyn DynVisitor<I>>>,
}

/// Object-safe visitor trait for dynamic dispatch.
///
/// Generic over ID type to support any tree.
pub trait DynVisitor<I: Identifier>: Send + Sync {
    /// Visit a node.
    fn visit_dyn(&mut self, id: I, depth: usize) -> VisitorResult;

    /// Pre-children hook.
    fn pre_children_dyn(&mut self, id: I, depth: usize);

    /// Post-children hook.
    fn post_children_dyn(&mut self, id: I, depth: usize);
}

impl<I: Identifier> VisitorVec<I> {
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
    pub fn push(&mut self, visitor: Box<dyn DynVisitor<I>>) {
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

impl<I: Identifier> Default for VisitorVec<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Identifier> sealed::Sealed for VisitorVec<I> {}

impl<I: Identifier, T: TreeNav<I>> TreeVisitor<I, T> for VisitorVec<I> {
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
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

    fn pre_children(&mut self, id: I, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.pre_children_dyn(id, depth);
        }
    }

    fn post_children(&mut self, id: I, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.post_children_dyn(id, depth);
        }
    }
}

// ============================================================================
// CONDITIONAL VISITOR
// ============================================================================

/// A visitor that only visits nodes matching a predicate.
pub struct ConditionalVisitor<V, P, I> {
    visitor: V,
    predicate: P,
    _marker: PhantomData<I>,
}

impl<V, P, I> ConditionalVisitor<V, P, I> {
    /// Create a new conditional visitor.
    pub fn new(visitor: V, predicate: P) -> Self
    where
        I: Identifier,
        P: FnMut(I, usize) -> bool,
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

impl<V, P, I> sealed::Sealed for ConditionalVisitor<V, P, I> {}

impl<I, T, V, P> TreeVisitor<I, T> for ConditionalVisitor<V, P, I>
where
    I: Identifier,
    T: TreeNav<I>,
    V: TreeVisitor<I, T>,
    P: FnMut(I, usize) -> bool,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        if (self.predicate)(id, depth) {
            self.visitor.visit(id, depth)
        } else {
            VisitorResult::Continue
        }
    }

    fn pre_children(&mut self, id: I, depth: usize) {
        self.visitor.pre_children(id, depth);
    }

    fn post_children(&mut self, id: I, depth: usize) {
        self.visitor.post_children(id, depth);
    }
}

// ============================================================================
// MAPPED VISITOR
// ============================================================================

/// A visitor that transforms node IDs before visiting.
pub struct MappedVisitor<V, F, I> {
    visitor: V,
    mapper: F,
    _marker: PhantomData<I>,
}

impl<V, F, I> MappedVisitor<V, F, I> {
    /// Create a new mapped visitor.
    pub fn new(visitor: V, mapper: F) -> Self
    where
        I: Identifier,
        F: FnMut(I, usize) -> I,
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

impl<V, F, I> sealed::Sealed for MappedVisitor<V, F, I> {}

impl<I, T, V, F> TreeVisitor<I, T> for MappedVisitor<V, F, I>
where
    I: Identifier,
    T: TreeNav<I>,
    V: TreeVisitor<I, T>,
    F: FnMut(I, usize) -> I,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.visit(mapped_id, depth)
    }

    fn pre_children(&mut self, id: I, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.pre_children(mapped_id, depth);
    }

    fn post_children(&mut self, id: I, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.post_children(mapped_id, depth);
    }
}

// ============================================================================
// EXTENSION TRAIT FOR COMPOSITION
// ============================================================================

/// Extension trait for composing visitors.
pub trait VisitorExt<I: Identifier, T: TreeNav<I>>: TreeVisitor<I, T> + Sized {
    /// Compose with another visitor.
    fn and_then<V: TreeVisitor<I, T>>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Add a third visitor to composition.
    fn and_also<V: TreeVisitor<I, T>>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Only visit nodes matching predicate.
    fn filter<P>(self, predicate: P) -> ConditionalVisitor<Self, P, I>
    where
        P: FnMut(I, usize) -> bool,
    {
        ConditionalVisitor::new(self, predicate)
    }

    /// Transform IDs before visiting.
    fn map_ids<F>(self, mapper: F) -> MappedVisitor<Self, F, I>
    where
        F: FnMut(I, usize) -> I,
    {
        MappedVisitor::new(self, mapper)
    }
}

// Blanket implementation
impl<I: Identifier, T: TreeNav<I>, V: TreeVisitor<I, T> + Sized> VisitorExt<I, T> for V {}

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

    impl TreeRead<ElementId> for TestTree {
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

    impl TreeNav<ElementId> for TestTree {
        type ChildrenIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
        type AncestorsIter<'a> = crate::iter::Ancestors<'a, ElementId, Self>;
        type DescendantsIter<'a> = crate::iter::DescendantsWithDepth<'a, ElementId, Self>;
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
        let result = <ComposedVisitor<_, _> as TreeVisitor<ElementId, TestTree>>::visit(
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

        // Test composition via extension trait
        let mut composed: ComposedVisitor<CountVisitor, MaxDepthVisitor> =
            <CountVisitor as VisitorExt<ElementId, TestTree>>::and_then(count, depth);
        <ComposedVisitor<_, _> as TreeVisitor<ElementId, TestTree>>::visit(
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

        <ConditionalVisitor<_, _, _> as TreeVisitor<ElementId, TestTree>>::visit(
            &mut conditional,
            ElementId::new(1),
            0,
        ); // Odd - skip
        <ConditionalVisitor<_, _, _> as TreeVisitor<ElementId, TestTree>>::visit(
            &mut conditional,
            ElementId::new(2),
            0,
        ); // Even - collect
        <ConditionalVisitor<_, _, _> as TreeVisitor<ElementId, TestTree>>::visit(
            &mut conditional,
            ElementId::new(3),
            0,
        ); // Odd - skip
        <ConditionalVisitor<_, _, _> as TreeVisitor<ElementId, TestTree>>::visit(
            &mut conditional,
            ElementId::new(4),
            0,
        ); // Even - collect

        let collector = conditional.into_inner();
        assert_eq!(collector.collected.len(), 2);
    }
}
