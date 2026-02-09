//! Advanced tree visitor patterns with HRTB and GAT support.
//!
//! This module provides visitor traits and implementations using advanced
//! Rust type system features for maximum flexibility and performance.

// Submodules
pub mod composition;
pub mod fallible;

// Re-exports from submodules
pub use composition::{
    ComposedVisitor, ConditionalVisitor, DynVisitor, MappedVisitor, TripleComposedVisitor,
    VisitorExt, VisitorVec,
};
pub use fallible::{
    try_collect, try_for_each, validate_depth, visit_fallible, visit_fallible_breadth_first,
    visit_fallible_with_path, DepthLimitExceeded, DepthLimitVisitor, FallibleVisitor,
    FallibleVisitorMut, TryCollectVisitor, TryForEachVisitor, VisitorError,
};

use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;

use super::TreeNav;
use flui_foundation::Identifier;

// ============================================================================
// VISITOR RESULT WITH ENHANCED CONTROL
// ============================================================================

/// Enhanced visitor result with more granular control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisitorResult {
    /// Continue normal traversal.
    Continue,
    /// Skip this node's children but continue with siblings.
    SkipChildren,
    /// Skip remaining siblings at this level.
    SkipSiblings,
    /// Stop traversal completely.
    Stop,
    /// Continue but suggest depth-first optimization.
    ContinueDepthFirst,
    /// Continue but suggest breadth-first optimization.
    ContinueBreadthFirst,
}

impl VisitorResult {
    /// Check if traversal should continue.
    #[inline]
    #[must_use]
    pub const fn should_continue(self) -> bool {
        matches!(
            self,
            Self::Continue
                | Self::SkipChildren
                | Self::ContinueDepthFirst
                | Self::ContinueBreadthFirst
        )
    }

    /// Check if children should be visited.
    #[inline]
    #[must_use]
    pub const fn should_visit_children(self) -> bool {
        matches!(
            self,
            Self::Continue | Self::ContinueDepthFirst | Self::ContinueBreadthFirst
        )
    }

    /// Check if traversal should stop completely.
    #[inline]
    #[must_use]
    pub const fn should_stop(self) -> bool {
        matches!(self, Self::Stop)
    }

    /// Check if siblings should be skipped.
    #[inline]
    #[must_use]
    pub const fn should_skip_siblings(self) -> bool {
        matches!(self, Self::SkipSiblings)
    }

    /// Get performance hint for iteration strategy.
    #[inline]
    #[must_use]
    pub const fn iteration_hint(self) -> IterationHint {
        match self {
            Self::ContinueDepthFirst => IterationHint::DepthFirst,
            Self::ContinueBreadthFirst => IterationHint::BreadthFirst,
            _ => IterationHint::Default,
        }
    }
}

/// Iteration strategy hint for performance optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterationHint {
    /// Use default strategy.
    Default,
    /// Prefer depth-first traversal.
    DepthFirst,
    /// Prefer breadth-first traversal.
    BreadthFirst,
}

// ============================================================================
// CORE VISITOR TRAITS
// ============================================================================

/// Basic tree visitor trait.
///
/// Generic over the ID type `I` and tree type `T`.
pub trait TreeVisitor<I: Identifier, T: TreeNav<I>>: sealed::Sealed {
    /// Visit a node.
    ///
    /// # Arguments
    ///
    /// * `id` - The element being visited
    /// * `depth` - Depth from traversal root (0-based)
    ///
    /// # Returns
    ///
    /// [`VisitorResult`] controlling further traversal
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult;

    /// Called before visiting children (optional hook).
    #[inline]
    fn pre_children(&mut self, _id: I, _depth: usize) {}

    /// Called after visiting all children (optional hook).
    #[inline]
    fn post_children(&mut self, _id: I, _depth: usize) {}

    /// Expected maximum tree depth for stack allocation.
    const MAX_STACK_DEPTH: usize = 64;

    /// Preferred batch size for bulk operations.
    const BATCH_SIZE: usize = 32;
}

/// Mutable visitor with tree access and GAT support.
pub trait TreeVisitorMut<I: Identifier, T: TreeNav<I>>: sealed::Sealed {
    /// The result type returned by this visitor (GAT).
    type Output<'a>
    where
        T: 'a,
        Self: 'a;

    /// Visit a node with full tree access.
    fn visit<'a>(
        &'a mut self,
        tree: &'a T,
        id: I,
        depth: usize,
    ) -> (VisitorResult, Option<Self::Output<'a>>)
    where
        T: 'a;

    /// Pre-children hook with tree access.
    #[inline]
    fn pre_children<'a>(&'a mut self, _tree: &'a T, _id: I, _depth: usize)
    where
        T: 'a,
    {
    }

    /// Post-children hook with tree access.
    #[inline]
    fn post_children<'a>(&'a mut self, _tree: &'a T, _id: I, _depth: usize)
    where
        T: 'a,
    {
    }

    /// Stack allocation size hint.
    const STACK_SIZE: usize = 48;
}

/// Typed visitor with flexible result collection using GAT.
pub trait TypedVisitor<I: Identifier, T: TreeNav<I>>: sealed::Sealed {
    /// The item type collected by this visitor (GAT).
    type Item<'a>
    where
        T: 'a,
        Self: 'a;

    /// Collection type for results (GAT).
    type Collection<'a>: Extend<Self::Item<'a>> + IntoIterator<Item = Self::Item<'a>>
    where
        T: 'a,
        Self: 'a;

    /// Visit and potentially collect an item.
    fn visit_typed<'a>(
        &'a mut self,
        tree: &'a T,
        id: I,
        depth: usize,
    ) -> (VisitorResult, Option<Self::Item<'a>>)
    where
        T: 'a;

    /// Create collection with appropriate capacity.
    fn create_collection<'a>(&self) -> Self::Collection<'a>
    where
        T: 'a,
        Self: 'a;

    /// Expected result count for collection sizing.
    const EXPECTED_ITEMS: usize = 16;
}

/// Sealed trait pattern for visitor traits.
pub(crate) mod sealed {
    pub trait Sealed {}
}

// ============================================================================
// TRAVERSAL FUNCTIONS
// ============================================================================

/// Internal traversal control signal.
///
/// Richer than `bool` to distinguish "stop everything" from "skip remaining siblings."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraversalSignal {
    /// Continue normal traversal.
    Continue,
    /// Skip remaining siblings at the caller's level.
    SkipSiblings,
    /// Stop traversal completely.
    Stop,
}

/// Depth-first traversal.
pub fn visit_depth_first<I, T, V>(tree: &T, root: I, visitor: &mut V) -> bool
where
    I: Identifier,
    T: TreeNav<I>,
    V: TreeVisitor<I, T>,
{
    visit_depth_first_impl(tree, root, 0, visitor) != TraversalSignal::Stop
}

/// Internal depth-first implementation with correct `SkipSiblings` propagation.
fn visit_depth_first_impl<I, T, V>(
    tree: &T,
    node: I,
    depth: usize,
    visitor: &mut V,
) -> TraversalSignal
where
    I: Identifier,
    T: TreeNav<I>,
    V: TreeVisitor<I, T>,
{
    let result = visitor.visit(node, depth);

    match result {
        VisitorResult::Stop => return TraversalSignal::Stop,
        // SkipSiblings: tell the parent to stop iterating siblings after this node.
        VisitorResult::SkipSiblings => return TraversalSignal::SkipSiblings,
        VisitorResult::SkipChildren => return TraversalSignal::Continue,
        _ => {}
    }

    if result.should_visit_children() {
        visitor.pre_children(node, depth);

        let children: Vec<I> = tree.children(node).collect();

        for child in children {
            match visit_depth_first_impl(tree, child, depth + 1, visitor) {
                TraversalSignal::Stop => return TraversalSignal::Stop,
                // A child said "skip my siblings" — break out of the children loop
                // but continue traversal at the parent level.
                TraversalSignal::SkipSiblings => break,
                TraversalSignal::Continue => {}
            }
        }

        visitor.post_children(node, depth);
    }

    TraversalSignal::Continue
}

/// Breadth-first traversal.
pub fn visit_breadth_first<I, T, V>(tree: &T, root: I, visitor: &mut V) -> bool
where
    I: Identifier,
    T: TreeNav<I>,
    V: TreeVisitor<I, T>,
{
    let mut queue: VecDeque<(I, usize, Option<I>)> = VecDeque::with_capacity(128);
    queue.push_back((root, 0, None));

    // When SkipSiblings is returned, we record the (parent, depth) to skip.
    let mut skip_parent: Option<(I, usize)> = None;

    while let Some((node, depth, parent)) = queue.pop_front() {
        // If we're skipping siblings under a specific parent at a specific depth,
        // skip nodes that share the same parent and depth.
        if let Some((skip_p, skip_d)) = skip_parent {
            if depth == skip_d && parent == Some(skip_p) {
                continue;
            }
            // Once we've moved past the skipped level, clear the flag.
            if depth != skip_d {
                skip_parent = None;
            }
        }

        let result = visitor.visit(node, depth);

        match result {
            VisitorResult::Stop => return false,
            VisitorResult::SkipSiblings => {
                // Record parent to skip remaining siblings at this level.
                if let Some(p) = parent {
                    skip_parent = Some((p, depth));
                }
                continue;
            }
            VisitorResult::SkipChildren => continue,
            _ => {}
        }

        if result.should_visit_children() {
            visitor.pre_children(node, depth);

            for child in tree.children(node) {
                queue.push_back((child, depth + 1, Some(node)));
            }

            visitor.post_children(node, depth);
        }
    }

    true
}

/// Typed visitor traversal with result collection.
pub fn visit_depth_first_typed<'a, I, T, V>(tree: &'a T, root: I, visitor: &'a mut V) -> Vec<I>
where
    I: Identifier,
    T: TreeNav<I>,
    V: TypedVisitor<I, T>,
{
    let mut collection = Vec::new();
    visit_typed_impl(tree, root, 0, visitor, &mut collection);
    collection
}

/// Internal typed visitor implementation.
fn visit_typed_impl<I, T, V>(
    tree: &T,
    root: I,
    initial_depth: usize,
    visitor: &mut V,
    collection: &mut Vec<I>,
) where
    I: Identifier,
    T: TreeNav<I>,
    V: TypedVisitor<I, T>,
{
    let mut stack: Vec<(I, usize)> = vec![(root, initial_depth)];

    while let Some((node, depth)) = stack.pop() {
        let (result, item) = visitor.visit_typed(tree, node, depth);

        if item.is_some() {
            collection.push(node);
        }

        if result.should_visit_children() {
            let children: Vec<_> = tree.children(node).collect();
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }

        if result.should_stop() {
            break;
        }
    }
}

// ============================================================================
// BUILT-IN VISITORS
// ============================================================================

/// Collector visitor for gathering element IDs.
#[derive(Debug)]
pub struct CollectVisitor<I> {
    /// Collected element IDs.
    pub collected: Vec<I>,
}

impl<I> CollectVisitor<I> {
    /// Create new collector with default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            collected: Vec::new(),
        }
    }

    /// Create collector with specific capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            collected: Vec::with_capacity(capacity),
        }
    }

    /// Consume visitor and return collected items.
    #[must_use]
    pub fn into_inner(self) -> Vec<I> {
        self.collected
    }
}

impl<I> sealed::Sealed for CollectVisitor<I> {}

impl<I: Identifier, T: TreeNav<I>> TreeVisitor<I, T> for CollectVisitor<I> {
    fn visit(&mut self, id: I, _depth: usize) -> VisitorResult {
        self.collected.push(id);
        VisitorResult::Continue
    }
}

impl<I> Default for CollectVisitor<I> {
    fn default() -> Self {
        Self::new()
    }
}

/// Counting visitor with overflow protection.
#[derive(Debug)]
pub struct CountVisitor {
    /// Current count of visited nodes.
    pub count: usize,
    /// Maximum count before stopping (overflow protection).
    pub max_count: Option<usize>,
}

impl CountVisitor {
    /// Create new counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: 0,
            max_count: None,
        }
    }

    /// Create counter with maximum limit.
    #[must_use]
    pub fn with_limit(max_count: usize) -> Self {
        Self {
            count: 0,
            max_count: Some(max_count),
        }
    }

    /// Get current count.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Check if limit has been reached.
    #[must_use]
    pub fn is_at_limit(&self) -> bool {
        self.max_count.is_some_and(|max| self.count >= max)
    }
}

impl sealed::Sealed for CountVisitor {}

impl<I: Identifier, T: TreeNav<I>> TreeVisitor<I, T> for CountVisitor {
    fn visit(&mut self, _id: I, _depth: usize) -> VisitorResult {
        if self.is_at_limit() {
            return VisitorResult::Stop;
        }

        self.count += 1;
        VisitorResult::Continue
    }
}

impl Default for CountVisitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Find visitor with predicate support.
pub struct FindVisitor<I, P> {
    predicate: P,
    /// The found element, if any matched the predicate.
    pub found: Option<I>,
    stop_on_first: bool,
}

impl<I, P> FindVisitor<I, P> {
    /// Create new finder that stops on first match.
    pub fn new(predicate: P) -> Self {
        Self {
            predicate,
            found: None,
            stop_on_first: true,
        }
    }

    /// Create finder that continues after first match.
    pub fn new_continue(predicate: P) -> Self {
        Self {
            predicate,
            found: None,
            stop_on_first: false,
        }
    }

    /// Get found element.
    pub fn found(&self) -> Option<I>
    where
        I: Copy,
    {
        self.found
    }
}

impl<I: fmt::Debug, P> fmt::Debug for FindVisitor<I, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FindVisitor")
            .field("found", &self.found)
            .field("stop_on_first", &self.stop_on_first)
            .finish_non_exhaustive()
    }
}

impl<I, P> sealed::Sealed for FindVisitor<I, P> {}

impl<I, T, P> TreeVisitor<I, T> for FindVisitor<I, P>
where
    I: Identifier,
    T: TreeNav<I>,
    P: Fn(I, usize) -> bool,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        if (self.predicate)(id, depth) {
            self.found = Some(id);
            if self.stop_on_first {
                return VisitorResult::Stop;
            }
        }
        VisitorResult::Continue
    }
}

/// Max depth finder with early termination optimization.
#[derive(Debug)]
pub struct MaxDepthVisitor {
    /// The maximum depth encountered so far.
    pub max_depth: usize,
    current_max: usize,
    termination_threshold: Option<usize>,
}

impl MaxDepthVisitor {
    /// Create new max depth finder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_depth: 0,
            current_max: 0,
            termination_threshold: None,
        }
    }

    /// Create finder with early termination.
    #[must_use]
    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            max_depth: 0,
            current_max: 0,
            termination_threshold: Some(threshold),
        }
    }
}

impl sealed::Sealed for MaxDepthVisitor {}

impl<I: Identifier, T: TreeNav<I>> TreeVisitor<I, T> for MaxDepthVisitor {
    fn visit(&mut self, _id: I, depth: usize) -> VisitorResult {
        if depth > self.current_max {
            self.current_max = depth;
            self.max_depth = depth;
        }

        if let Some(threshold) = self.termination_threshold {
            if depth >= threshold {
                return VisitorResult::Stop;
            }
        }

        VisitorResult::Continue
    }
}

impl Default for MaxDepthVisitor {
    fn default() -> Self {
        Self::new()
    }
}

/// For-each visitor with closure support.
pub struct ForEachVisitor<I, F> {
    callback: F,
    _marker: PhantomData<I>,
}

impl<I, F> ForEachVisitor<I, F> {
    /// Create new for-each visitor.
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            _marker: PhantomData,
        }
    }
}

impl<I, F> fmt::Debug for ForEachVisitor<I, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForEachVisitor").finish_non_exhaustive()
    }
}

impl<I, F> sealed::Sealed for ForEachVisitor<I, F> {}

impl<I, T, F> TreeVisitor<I, T> for ForEachVisitor<I, F>
where
    I: Identifier,
    T: TreeNav<I>,
    F: FnMut(I, usize),
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        (self.callback)(id, depth);
        VisitorResult::Continue
    }
}

/// Stateful visitor with typestate pattern.
pub struct StatefulVisitor<State, Data> {
    data: Data,
    _state: PhantomData<State>,
}

impl<State, Data> fmt::Debug for StatefulVisitor<State, Data> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulVisitor").finish_non_exhaustive()
    }
}

/// Visitor states for typestate pattern.
pub mod states {
    /// Initial state - visitor just created.
    #[derive(Debug)]
    pub struct Initial;
    /// Started state - visitor has begun traversal.
    #[derive(Debug)]
    pub struct Started;
    /// Finished state - visitor has completed traversal.
    #[derive(Debug)]
    pub struct Finished;
}

impl<Data> StatefulVisitor<states::Initial, Data> {
    /// Create new stateful visitor in Initial state.
    pub fn new(data: Data) -> Self {
        Self {
            data,
            _state: PhantomData,
        }
    }

    /// Start traversal, transitioning to Started state.
    pub fn start(self) -> StatefulVisitor<states::Started, Data> {
        StatefulVisitor {
            data: self.data,
            _state: PhantomData,
        }
    }
}

impl<Data> StatefulVisitor<states::Started, Data> {
    /// Finish traversal, transitioning to Finished state.
    pub fn finish(self) -> StatefulVisitor<states::Finished, Data> {
        StatefulVisitor {
            data: self.data,
            _state: PhantomData,
        }
    }

    /// Access data during traversal.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// Mutably access data during traversal.
    pub fn data_mut(&mut self) -> &mut Data {
        &mut self.data
    }
}

impl<Data> StatefulVisitor<states::Finished, Data> {
    /// Consume visitor and extract final data.
    pub fn into_data(self) -> Data {
        self.data
    }
}

impl<Data> sealed::Sealed for StatefulVisitor<states::Started, Data> {}

impl<I, T, Data> TreeVisitor<I, T> for StatefulVisitor<states::Started, Data>
where
    I: Identifier,
    T: TreeNav<I>,
    Data: FnMut(I, usize) -> VisitorResult,
{
    fn visit(&mut self, id: I, depth: usize) -> VisitorResult {
        (self.data)(id, depth)
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Collect all nodes in a subtree.
pub fn collect_all<I, T>(tree: &T, root: I) -> Vec<I>
where
    I: Identifier,
    T: TreeNav<I>,
{
    let mut visitor = CollectVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.into_inner()
}

/// Count all nodes in a subtree.
pub fn count_all<I, T>(tree: &T, root: I) -> usize
where
    I: Identifier,
    T: TreeNav<I>,
{
    let mut visitor = CountVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.count
}

/// Count nodes up to a maximum limit.
pub fn count_with_limit<I, T>(tree: &T, root: I, limit: usize) -> usize
where
    I: Identifier,
    T: TreeNav<I>,
{
    let mut visitor = CountVisitor::with_limit(limit);
    visit_depth_first(tree, root, &mut visitor);
    visitor.count
}

/// Find maximum depth in subtree.
pub fn max_depth<I, T>(tree: &T, root: I) -> usize
where
    I: Identifier,
    T: TreeNav<I>,
{
    let mut visitor = MaxDepthVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.max_depth
}

/// Find maximum depth with threshold-based early termination.
pub fn max_depth_with_threshold<I, T>(tree: &T, root: I, threshold: usize) -> usize
where
    I: Identifier,
    T: TreeNav<I>,
{
    let mut visitor = MaxDepthVisitor::with_threshold(threshold);
    visit_depth_first(tree, root, &mut visitor);
    visitor.max_depth
}

/// Find first node matching predicate.
pub fn find_first<I, T, P>(tree: &T, root: I, predicate: P) -> Option<I>
where
    I: Identifier,
    T: TreeNav<I>,
    P: Fn(I, usize) -> bool,
{
    let mut visitor = FindVisitor::new(predicate);
    visit_depth_first(tree, root, &mut visitor);
    visitor.found
}

/// Execute closure for each node in subtree.
pub fn for_each<I, T, F>(tree: &T, root: I, callback: F)
where
    I: Identifier,
    T: TreeNav<I>,
    F: FnMut(I, usize),
{
    let mut visitor = ForEachVisitor::new(callback);
    visit_depth_first(tree, root, &mut visitor);
}

/// Stateful traversal with typestate guarantees.
pub fn visit_stateful<I, T, Data>(
    tree: &T,
    root: I,
    data: Data,
) -> StatefulVisitor<states::Finished, Data>
where
    I: Identifier,
    T: TreeNav<I>,
    Data: FnMut(I, usize) -> VisitorResult,
{
    let mut visitor = StatefulVisitor::new(data).start();
    visit_depth_first(tree, root, &mut visitor);
    visitor.finish()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TreeRead;
    use flui_foundation::ElementId;

    // Test tree implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    }

    struct TestTree {
        nodes: std::collections::HashMap<ElementId, TestNode>,
    }

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

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            self.nodes.keys().copied()
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
            crate::iter::Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            crate::iter::DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
        }
    }

    #[test]
    fn test_collect_visitor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));

        let collected = collect_all(&tree, root);
        assert_eq!(collected.len(), 3);
        assert!(collected.contains(&root));
        assert!(collected.contains(&child1));
        assert!(collected.contains(&child2));
    }

    #[test]
    fn test_count_visitor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child = ElementId::new(2);

        tree.insert(root, None);
        tree.insert(child, Some(root));

        assert_eq!(count_all(&tree, root), 2);
        assert_eq!(count_with_limit(&tree, root, 1), 1);
    }

    #[test]
    fn test_find_visitor_hrtb() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child = ElementId::new(2);

        tree.insert(root, None);
        tree.insert(child, Some(root));

        let found = find_first(&tree, root, |id, _depth| id == child);
        assert_eq!(found, Some(child));

        let not_found = find_first(&tree, root, |_id, depth| depth > 5);
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_max_depth_visitor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child = ElementId::new(2);
        let grandchild = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child, Some(root));
        tree.insert(grandchild, Some(child));

        assert_eq!(max_depth(&tree, root), 2);
        assert_eq!(max_depth_with_threshold(&tree, root, 1), 1);
    }

    #[test]
    fn test_for_each_visitor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child = ElementId::new(2);

        tree.insert(root, None);
        tree.insert(child, Some(root));

        let mut visited = Vec::new();
        for_each(&tree, root, |id, depth| {
            visited.push((id, depth));
        });

        assert_eq!(visited.len(), 2);
        assert!(visited.contains(&(root, 0)));
        assert!(visited.contains(&(child, 1)));
    }

    #[test]
    fn test_stateful_visitor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        tree.insert(root, None);

        let mut count = 0;
        let finished = visit_stateful(&tree, root, |_id, _depth| {
            count += 1;
            VisitorResult::Continue
        });

        let _final_data = finished.into_data();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_visitor_result_methods() {
        assert!(VisitorResult::Continue.should_continue());
        assert!(VisitorResult::Continue.should_visit_children());
        assert!(!VisitorResult::Stop.should_continue());
        assert!(!VisitorResult::SkipChildren.should_visit_children());

        assert_eq!(
            VisitorResult::ContinueDepthFirst.iteration_hint(),
            IterationHint::DepthFirst
        );
    }

    // Helper visitor that returns SkipSiblings when it encounters a target node.
    struct SkipSiblingsVisitor {
        target: ElementId,
        visited: Vec<ElementId>,
    }

    impl sealed::Sealed for SkipSiblingsVisitor {}

    impl TreeVisitor<ElementId, TestTree> for SkipSiblingsVisitor {
        fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
            self.visited.push(id);
            if id == self.target {
                VisitorResult::SkipSiblings
            } else {
                VisitorResult::Continue
            }
        }
    }

    #[test]
    fn test_skip_siblings_dfs() {
        // Tree:  root -> [child1, child2, child3]
        //                  child1 -> [gc1]
        // When child1 returns SkipSiblings, child2 and child3 should NOT be visited.
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);
        let child3 = ElementId::new(4);
        let gc1 = ElementId::new(5);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));
        tree.insert(child3, Some(root));
        tree.insert(gc1, Some(child1));

        let mut visitor = SkipSiblingsVisitor {
            target: child1,
            visited: Vec::new(),
        };
        let completed = visit_depth_first(&tree, root, &mut visitor);

        assert!(completed, "traversal should not be stopped");
        // root is visited, then child1 triggers SkipSiblings.
        // child1's children (gc1) are NOT visited because SkipSiblings skips children too.
        // child2 and child3 are NOT visited because they are siblings of child1.
        assert!(visitor.visited.contains(&root));
        assert!(visitor.visited.contains(&child1));
        assert!(
            !visitor.visited.contains(&child2),
            "child2 should be skipped (sibling of child1)"
        );
        assert!(
            !visitor.visited.contains(&child3),
            "child3 should be skipped (sibling of child1)"
        );
    }

    #[test]
    fn test_skip_siblings_bfs() {
        // Same tree structure, same expectations for BFS.
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);
        let child3 = ElementId::new(4);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));
        tree.insert(child3, Some(root));

        let mut visitor = SkipSiblingsVisitor {
            target: child1,
            visited: Vec::new(),
        };
        let completed = visit_breadth_first(&tree, root, &mut visitor);

        assert!(completed, "traversal should not be stopped");
        assert!(visitor.visited.contains(&root));
        assert!(visitor.visited.contains(&child1));
        assert!(
            !visitor.visited.contains(&child2),
            "child2 should be skipped (sibling of child1)"
        );
        assert!(
            !visitor.visited.contains(&child3),
            "child3 should be skipped (sibling of child1)"
        );
    }

    // Helper visitor that returns Stop on the first node.
    struct StopImmediatelyVisitor {
        call_count: usize,
    }

    impl sealed::Sealed for StopImmediatelyVisitor {}

    impl TreeVisitor<ElementId, TestTree> for StopImmediatelyVisitor {
        fn visit(&mut self, _id: ElementId, _depth: usize) -> VisitorResult {
            self.call_count += 1;
            VisitorResult::Stop
        }
    }

    #[test]
    fn test_visitor_result_stop_terminates() {
        // Build a tree with root -> [child1, child2], child1 -> [gc1]
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);
        let gc1 = ElementId::new(4);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));
        tree.insert(gc1, Some(child1));

        // DFS: visitor returns Stop on the very first node (root).
        // It should be called exactly once.
        let mut visitor = StopImmediatelyVisitor { call_count: 0 };
        let completed = visit_depth_first(&tree, root, &mut visitor);

        assert!(!completed, "traversal should report stopped (return false)");
        assert_eq!(
            visitor.call_count, 1,
            "visitor should be called exactly once when Stop is returned immediately"
        );

        // BFS: same expectation.
        let mut visitor_bfs = StopImmediatelyVisitor { call_count: 0 };
        let completed_bfs = visit_breadth_first(&tree, root, &mut visitor_bfs);

        assert!(
            !completed_bfs,
            "BFS traversal should report stopped (return false)"
        );
        assert_eq!(
            visitor_bfs.call_count, 1,
            "BFS visitor should be called exactly once when Stop is returned immediately"
        );
    }

    // Helper visitor that returns SkipChildren on a specific target node.
    struct SkipChildrenVisitor {
        target: ElementId,
        visited: Vec<ElementId>,
    }

    impl sealed::Sealed for SkipChildrenVisitor {}

    impl TreeVisitor<ElementId, TestTree> for SkipChildrenVisitor {
        fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
            self.visited.push(id);
            if id == self.target {
                VisitorResult::SkipChildren
            } else {
                VisitorResult::Continue
            }
        }
    }

    #[test]
    fn test_visitor_result_skip_children_on_leaf() {
        // Tree: root -> [leaf1, leaf2, leaf3]
        // leaf1, leaf2, leaf3 have no children (they are leaves).
        // Returning SkipChildren on leaf1 should be equivalent to Continue:
        // iteration must continue to siblings leaf2 and leaf3.
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let leaf1 = ElementId::new(2);
        let leaf2 = ElementId::new(3);
        let leaf3 = ElementId::new(4);

        tree.insert(root, None);
        tree.insert(leaf1, Some(root));
        tree.insert(leaf2, Some(root));
        tree.insert(leaf3, Some(root));

        // DFS
        let mut visitor = SkipChildrenVisitor {
            target: leaf1,
            visited: Vec::new(),
        };
        let completed = visit_depth_first(&tree, root, &mut visitor);

        assert!(completed, "traversal should complete normally");
        assert_eq!(
            visitor.visited.len(),
            4,
            "all 4 nodes should be visited: root, leaf1, leaf2, leaf3"
        );
        assert!(visitor.visited.contains(&root));
        assert!(visitor.visited.contains(&leaf1));
        assert!(
            visitor.visited.contains(&leaf2),
            "leaf2 should still be visited after SkipChildren on sibling leaf1"
        );
        assert!(
            visitor.visited.contains(&leaf3),
            "leaf3 should still be visited after SkipChildren on sibling leaf1"
        );

        // BFS: same expectations.
        let mut visitor_bfs = SkipChildrenVisitor {
            target: leaf1,
            visited: Vec::new(),
        };
        let completed_bfs = visit_breadth_first(&tree, root, &mut visitor_bfs);

        assert!(completed_bfs, "BFS traversal should complete normally");
        assert_eq!(
            visitor_bfs.visited.len(),
            4,
            "BFS: all 4 nodes should be visited"
        );
        assert!(
            visitor_bfs.visited.contains(&leaf2),
            "BFS: leaf2 should still be visited"
        );
        assert!(
            visitor_bfs.visited.contains(&leaf3),
            "BFS: leaf3 should still be visited"
        );
    }
}
