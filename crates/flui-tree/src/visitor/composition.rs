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
use flui_foundation::ElementId;

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

impl<A, B> TreeVisitor for ComposedVisitor<A, B>
where
    A: TreeVisitor,
    B: TreeVisitor,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);

        // Combine results - most restrictive wins
        combine_results(result_a, result_b)
    }

    fn pre_children(&mut self, id: ElementId, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
    }

    fn post_children(&mut self, id: ElementId, depth: usize) {
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

impl<A, B, C> TreeVisitor for TripleComposedVisitor<A, B, C>
where
    A: TreeVisitor,
    B: TreeVisitor,
    C: TreeVisitor,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        let result_a = self.first.visit(id, depth);
        let result_b = self.second.visit(id, depth);
        let result_c = self.third.visit(id, depth);

        combine_results(combine_results(result_a, result_b), result_c)
    }

    fn pre_children(&mut self, id: ElementId, depth: usize) {
        self.first.pre_children(id, depth);
        self.second.pre_children(id, depth);
        self.third.pre_children(id, depth);
    }

    fn post_children(&mut self, id: ElementId, depth: usize) {
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
pub struct VisitorVec {
    visitors: Vec<Box<dyn DynVisitor>>,
}

/// Object-safe visitor trait for dynamic dispatch.
pub trait DynVisitor: Send + Sync {
    /// Visit a node.
    fn visit_dyn(&mut self, id: ElementId, depth: usize) -> VisitorResult;

    /// Pre-children hook.
    fn pre_children_dyn(&mut self, id: ElementId, depth: usize);

    /// Post-children hook.
    fn post_children_dyn(&mut self, id: ElementId, depth: usize);
}

impl<T: TreeVisitor + Send + Sync> DynVisitor for T {
    fn visit_dyn(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        self.visit(id, depth)
    }

    fn pre_children_dyn(&mut self, id: ElementId, depth: usize) {
        self.pre_children(id, depth);
    }

    fn post_children_dyn(&mut self, id: ElementId, depth: usize) {
        self.post_children(id, depth);
    }
}

impl VisitorVec {
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
    pub fn push<V: TreeVisitor + Send + Sync + 'static>(&mut self, visitor: V) {
        self.visitors.push(Box::new(visitor));
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

impl Default for VisitorVec {
    fn default() -> Self {
        Self::new()
    }
}

impl sealed::Sealed for VisitorVec {}

impl TreeVisitor for VisitorVec {
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
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

    fn pre_children(&mut self, id: ElementId, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.pre_children_dyn(id, depth);
        }
    }

    fn post_children(&mut self, id: ElementId, depth: usize) {
        for visitor in &mut self.visitors {
            visitor.post_children_dyn(id, depth);
        }
    }
}

// ============================================================================
// CONDITIONAL VISITOR
// ============================================================================

/// A visitor that only visits nodes matching a predicate.
pub struct ConditionalVisitor<V, P> {
    visitor: V,
    predicate: P,
}

impl<V, P> ConditionalVisitor<V, P> {
    /// Create a new conditional visitor.
    pub fn new(visitor: V, predicate: P) -> Self
    where
        V: TreeVisitor,
        P: FnMut(ElementId, usize) -> bool,
    {
        Self { visitor, predicate }
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

impl<V, P> sealed::Sealed for ConditionalVisitor<V, P> {}

impl<V, P> TreeVisitor for ConditionalVisitor<V, P>
where
    V: TreeVisitor,
    P: FnMut(ElementId, usize) -> bool,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        if (self.predicate)(id, depth) {
            self.visitor.visit(id, depth)
        } else {
            VisitorResult::Continue
        }
    }

    fn pre_children(&mut self, id: ElementId, depth: usize) {
        self.visitor.pre_children(id, depth);
    }

    fn post_children(&mut self, id: ElementId, depth: usize) {
        self.visitor.post_children(id, depth);
    }
}

// ============================================================================
// MAPPED VISITOR
// ============================================================================

/// A visitor that transforms node IDs before visiting.
pub struct MappedVisitor<V, F> {
    visitor: V,
    mapper: F,
}

impl<V, F> MappedVisitor<V, F> {
    /// Create a new mapped visitor.
    pub fn new(visitor: V, mapper: F) -> Self
    where
        V: TreeVisitor,
        F: FnMut(ElementId, usize) -> ElementId,
    {
        Self { visitor, mapper }
    }

    /// Get the inner visitor.
    pub fn into_inner(self) -> V {
        self.visitor
    }
}

impl<V, F> sealed::Sealed for MappedVisitor<V, F> {}

impl<V, F> TreeVisitor for MappedVisitor<V, F>
where
    V: TreeVisitor,
    F: FnMut(ElementId, usize) -> ElementId,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.visit(mapped_id, depth)
    }

    fn pre_children(&mut self, id: ElementId, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.pre_children(mapped_id, depth);
    }

    fn post_children(&mut self, id: ElementId, depth: usize) {
        let mapped_id = (self.mapper)(id, depth);
        self.visitor.post_children(mapped_id, depth);
    }
}

// ============================================================================
// EXTENSION TRAIT FOR COMPOSITION
// ============================================================================

/// Extension trait for composing visitors.
pub trait VisitorExt: TreeVisitor + Sized {
    /// Compose with another visitor.
    fn and_then<V: TreeVisitor>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Add a third visitor to composition.
    fn and_also<V: TreeVisitor>(self, other: V) -> ComposedVisitor<Self, V> {
        ComposedVisitor::new(self, other)
    }

    /// Only visit nodes matching predicate.
    fn filter<P>(self, predicate: P) -> ConditionalVisitor<Self, P>
    where
        P: FnMut(ElementId, usize) -> bool,
    {
        ConditionalVisitor::new(self, predicate)
    }

    /// Transform IDs before visiting.
    fn map_ids<F>(self, mapper: F) -> MappedVisitor<Self, F>
    where
        F: FnMut(ElementId, usize) -> ElementId,
    {
        MappedVisitor::new(self, mapper)
    }
}

// Blanket implementation
impl<T: TreeVisitor + Sized> VisitorExt for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visitor::{CollectVisitor, CountVisitor, MaxDepthVisitor};

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
        let count = CountVisitor::new();
        let depth = MaxDepthVisitor::new();

        let mut composed = ComposedVisitor::new(count, depth);

        // Test visit method
        let result = composed.visit(ElementId::new(1), 0);
        assert_eq!(result, VisitorResult::Continue);

        let (count, depth) = composed.into_parts();
        assert_eq!(count.count, 1);
        assert_eq!(depth.max_depth, 0);
    }

    #[test]
    fn test_visitor_vec() {
        let mut vec = VisitorVec::new();
        assert!(vec.is_empty());

        vec.push(CountVisitor::new());
        vec.push(MaxDepthVisitor::new());

        assert_eq!(vec.len(), 2);
        assert!(!vec.is_empty());

        let result = vec.visit(ElementId::new(1), 0);
        assert_eq!(result, VisitorResult::Continue);
    }

    #[test]
    fn test_visitor_ext() {
        let count = CountVisitor::new();
        let depth = MaxDepthVisitor::new();

        // Test composition via extension trait
        let mut composed = count.and_then(depth);
        composed.visit(ElementId::new(1), 5);

        let (c, d) = composed.into_parts();
        assert_eq!(c.count, 1);
        assert_eq!(d.max_depth, 5);
    }

    #[test]
    fn test_conditional_visitor() {
        let collector = CollectVisitor::new();

        // Only collect even IDs
        let mut conditional = ConditionalVisitor::new(collector, |id, _| id.get() % 2 == 0);

        conditional.visit(ElementId::new(1), 0); // Odd - skip
        conditional.visit(ElementId::new(2), 0); // Even - collect
        conditional.visit(ElementId::new(3), 0); // Odd - skip
        conditional.visit(ElementId::new(4), 0); // Even - collect

        let collector = conditional.into_inner();
        assert_eq!(collector.collected.len(), 2);
    }
}
