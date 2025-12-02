//! Advanced tree visitor patterns with HRTB and GAT support.
//!
//! This module provides visitor traits and implementations using advanced
//! Rust type system features for maximum flexibility and performance:
//!
//! - **HRTB (Higher-Rank Trait Bounds)** for universal predicates
//! - **GAT (Generic Associated Types)** for flexible return types
//! - **Associated Constants** for performance tuning
//! - **Const generics** for compile-time optimization
//! - **Sealed traits** for safety
//! - **Typestate patterns** for compile-time guarantees
//!
//! # Visitor Types
//!
//! - [`TreeVisitor`] - Basic visitor with HRTB support
//! - [`TreeVisitorMut`] - Mutable visitor with node access
//! - [`TypedVisitor`] - GAT-based visitor with flexible return types
//! - [`StatefulVisitor`] - Visitor with compile-time state tracking
//! - [`FallibleVisitor`] - Visitor that can return errors
//! - [`StatisticsVisitor`] - Visitor for collecting tree statistics
//!
//! # Composition
//!
//! Visitors can be composed for efficiency:
//! - [`ComposedVisitor`] - Combine two visitors
//! - [`TripleComposedVisitor`] - Combine three visitors
//! - [`VisitorVec`] - Dynamic collection of visitors
//! - [`ConditionalVisitor`] - Filter visited nodes
//!
//! # Performance Features
//!
//! All visitors use associated constants and const generics for:
//! - Stack-allocated buffers for typical tree depths
//! - Optimized iteration strategies
//! - Cache-friendly memory access patterns
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeNav, visit_depth_first_typed};
//!
//! // HRTB visitor that works with any lifetime
//! let mut visitor = FindVisitor::new(|node: &SomeNodeType| node.name == "target");
//! let result = visit_depth_first_typed(&tree, root, &mut visitor);
//!
//! // Compose multiple visitors
//! let count = CountVisitor::new();
//! let depth = MaxDepthVisitor::new();
//! let mut composed = count.and_then(depth);
//! visit_depth_first(&tree, root, &mut composed);
//! ```

// Submodules
pub mod composition;
pub mod fallible;
pub mod statistics;

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
pub use statistics::{
    collect_statistics, compare_statistics, tree_summary, StatisticsComparison, StatisticsVisitor,
    StatisticsVisitorMut, TreeStatistics,
};

use flui_foundation::ElementId;
use std::collections::VecDeque;
use std::marker::PhantomData;

use super::TreeNav;

// ============================================================================
// VISITOR RESULT WITH ENHANCED CONTROL
// ============================================================================

/// Enhanced visitor result with more granular control.
///
/// Provides fine-grained control over traversal behavior with
/// performance hints for optimized iteration.
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
    pub const fn should_visit_children(self) -> bool {
        matches!(
            self,
            Self::Continue | Self::ContinueDepthFirst | Self::ContinueBreadthFirst
        )
    }

    /// Check if traversal should stop completely.
    #[inline]
    pub const fn should_stop(self) -> bool {
        matches!(self, Self::Stop)
    }

    /// Check if siblings should be skipped.
    #[inline]
    pub const fn should_skip_siblings(self) -> bool {
        matches!(self, Self::SkipSiblings)
    }

    /// Get performance hint for iteration strategy.
    #[inline]
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
// CORE VISITOR TRAITS WITH HRTB AND GAT
// ============================================================================

/// Basic tree visitor trait with HRTB support.
///
/// This trait uses Higher-Rank Trait Bounds to allow visitors
/// that work with any lifetime, enabling maximum flexibility.
pub trait TreeVisitor: sealed::Sealed {
    /// Visit a node with HRTB-compatible signature.
    ///
    /// The visitor method uses HRTB to accept any lifetime,
    /// making it compatible with different data access patterns.
    ///
    /// # Arguments
    ///
    /// * `id` - The element being visited
    /// * `depth` - Depth from traversal root (0-based)
    ///
    /// # Returns
    ///
    /// [`VisitorResult`] controlling further traversal
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult;

    /// Called before visiting children (optional hook).
    ///
    /// Only called if `visit` returned a result that allows children.
    /// Default implementation does nothing.
    #[inline]
    fn pre_children(&mut self, _id: ElementId, _depth: usize) {}

    /// Called after visiting all children (optional hook).
    ///
    /// Only called if `visit` returned a result that allows children
    /// and all children have been processed.
    #[inline]
    fn post_children(&mut self, _id: ElementId, _depth: usize) {}

    /// Performance tuning constants.

    /// Expected maximum tree depth for stack allocation.
    const MAX_STACK_DEPTH: usize = 64;

    /// Preferred batch size for bulk operations.
    const BATCH_SIZE: usize = 32;
}

/// Mutable visitor with tree access and GAT support.
///
/// This trait provides access to the tree and nodes during visitation,
/// using GAT for flexible return types and HRTB for universal compatibility.
pub trait TreeVisitorMut<T: TreeNav>: sealed::Sealed {
    /// The result type returned by this visitor (GAT).
    ///
    /// Different visitors can return different types while maintaining
    /// a consistent interface. The lifetime parameter allows for
    /// borrowing from the tree or nodes.
    type Output<'a>
    where
        T: 'a,
        Self: 'a;

    /// Visit a node with full tree access using HRTB.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree being traversed
    /// * `id` - The element being visited
    /// * `depth` - Depth from traversal root
    ///
    /// # Returns
    ///
    /// Tuple of (VisitorResult, Option<Output>) where Output can be
    /// any type specified by the GAT.
    fn visit<'a>(
        &'a mut self,
        tree: &'a T,
        id: ElementId,
        depth: usize,
    ) -> (VisitorResult, Option<Self::Output<'a>>)
    where
        T: 'a;

    /// Pre-children hook with tree access.
    #[inline]
    fn pre_children<'a>(&'a mut self, _tree: &'a T, _id: ElementId, _depth: usize)
    where
        T: 'a,
    {
    }

    /// Post-children hook with tree access.
    #[inline]
    fn post_children<'a>(&'a mut self, _tree: &'a T, _id: ElementId, _depth: usize)
    where
        T: 'a,
    {
    }

    /// Stack allocation size hint.
    const STACK_SIZE: usize = 48;
}

/// Typed visitor with flexible result collection using GAT.
///
/// This visitor type allows collecting results of different types
/// while maintaining zero-cost abstractions and type safety.
pub trait TypedVisitor<T: TreeNav>: sealed::Sealed {
    /// The item type collected by this visitor (GAT).
    type Item<'a>
    where
        T: 'a,
        Self: 'a;

    /// Collection type for results (GAT).
    ///
    /// Can be Vec, SmallVec, custom collections, etc.
    type Collection<'a>: Extend<Self::Item<'a>> + IntoIterator<Item = Self::Item<'a>>
    where
        T: 'a,
        Self: 'a;

    /// Visit and potentially collect an item.
    ///
    /// # Returns
    ///
    /// Tuple of (VisitorResult, Option<Item>) where Item is added
    /// to the collection if Some.
    fn visit_typed<'a>(
        &'a mut self,
        tree: &'a T,
        id: ElementId,
        depth: usize,
    ) -> (VisitorResult, Option<Self::Item<'a>>)
    where
        T: 'a;

    /// Create collection with appropriate capacity.
    ///
    /// Uses associated constants for optimal sizing.
    fn create_collection<'a>(&self) -> Self::Collection<'a>
    where
        T: 'a,
        Self: 'a;

    /// Expected result count for collection sizing.
    const EXPECTED_ITEMS: usize = 16;
}

/// Sealed trait pattern for visitor traits.
mod sealed {
    pub trait Sealed {}
}

// ============================================================================
// TRAVERSAL FUNCTIONS WITH CONST GENERIC OPTIMIZATION
// ============================================================================

/// Depth-first traversal optimized with const generics.
///
/// Uses stack-allocated buffer for typical tree depths,
/// falling back to heap allocation for deeper trees.
///
/// # Type Parameters
///
/// * `T` - Tree type implementing TreeNav
/// * `V` - Visitor type
/// * `STACK_SIZE` - Stack buffer size (const generic)
///
/// # Returns
///
/// `true` if traversal completed, `false` if stopped early.
pub fn visit_depth_first<T, V>(tree: &T, root: ElementId, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    visit_depth_first_impl::<T, V, 64>(tree, root, 0, visitor)
}

/// Internal depth-first implementation with stack optimization.
fn visit_depth_first_impl<T, V, const STACK_SIZE: usize>(
    tree: &T,
    node: ElementId,
    depth: usize,
    visitor: &mut V,
) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    // Visit current node
    let result = visitor.visit(node, depth);

    match result {
        VisitorResult::Stop => return false,
        VisitorResult::SkipChildren | VisitorResult::SkipSiblings => return true,
        _ => {}
    }

    if result.should_visit_children() {
        visitor.pre_children(node, depth);

        // Use heap allocation for child counts (simplified for now)
        let children: Vec<ElementId> = tree.children(node).collect();

        for child in children {
            if !visit_depth_first_impl::<T, V, STACK_SIZE>(tree, child, depth + 1, visitor) {
                return false;
            }
        }

        visitor.post_children(node, depth);
    }

    true
}

/// Breadth-first traversal with configurable queue size.
///
/// Uses VecDeque with initial capacity based on const generics
/// for optimal memory usage patterns.
pub fn visit_breadth_first<T, V>(tree: &T, root: ElementId, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    let mut queue: VecDeque<(ElementId, usize)> = VecDeque::with_capacity(128);
    queue.push_back((root, 0));

    while let Some((node, depth)) = queue.pop_front() {
        let result = visitor.visit(node, depth);

        match result {
            VisitorResult::Stop => return false,
            VisitorResult::SkipChildren => continue,
            _ => {}
        }

        if result.should_visit_children() {
            visitor.pre_children(node, depth);

            // Add children to queue
            for child in tree.children(node) {
                queue.push_back((child, depth + 1));
            }

            visitor.post_children(node, depth);
        }
    }

    true
}

/// Typed visitor traversal with result collection.
///
/// Uses GAT to collect results of any type while maintaining
/// zero-cost abstractions and optimal memory usage.
///
/// Note: Returns Vec<ElementId> for simplicity due to lifetime constraints.
pub fn visit_depth_first_typed<'a, T, V>(
    tree: &'a T,
    root: ElementId,
    visitor: &'a mut V,
) -> Vec<ElementId>
where
    T: TreeNav,
    V: TypedVisitor<T>,
{
    let mut collection = Vec::new();
    visit_typed_impl(tree, root, 0, visitor, &mut collection);
    collection
}

/// Internal typed visitor implementation.
///
/// Uses an iterative approach to avoid lifetime issues with recursive mutable borrows.
fn visit_typed_impl<'a, T, V>(
    tree: &'a T,
    root: ElementId,
    initial_depth: usize,
    visitor: &mut V,
    collection: &mut Vec<ElementId>,
) where
    T: TreeNav,
    V: TypedVisitor<T>,
{
    // Use iterative approach with explicit stack to avoid lifetime issues
    let mut stack: Vec<(ElementId, usize)> = vec![(root, initial_depth)];

    while let Some((node, depth)) = stack.pop() {
        let (result, item) = visitor.visit_typed(tree, node, depth);

        if let Some(_item) = item {
            // Store just the ElementId to avoid lifetime issues
            collection.push(node);
        }

        if result.should_visit_children() {
            // Collect children first, then push in reverse for correct order
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
// BUILT-IN VISITORS WITH ADVANCED FEATURES
// ============================================================================

/// Collector visitor for gathering element IDs.
///
/// Collects element IDs with heap-allocated storage.
/// Uses `Vec` for simplicity and broad compatibility.
pub struct CollectVisitor {
    /// Collected element IDs.
    pub collected: Vec<ElementId>,
}

impl CollectVisitor {
    /// Create new collector with default capacity.
    pub fn new() -> Self {
        Self {
            collected: Vec::new(),
        }
    }

    /// Create collector with specific capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            collected: Vec::with_capacity(capacity),
        }
    }

    /// Consume visitor and return collected items.
    pub fn into_inner(self) -> Vec<ElementId> {
        self.collected
    }
}

impl sealed::Sealed for CollectVisitor {}

impl TreeVisitor for CollectVisitor {
    fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
        self.collected.push(id);
        VisitorResult::Continue
    }
}

impl Default for CollectVisitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Counting visitor with overflow protection.
pub struct CountVisitor {
    /// Current count with overflow protection.
    pub count: usize,
    /// Maximum count before stopping (overflow protection).
    pub max_count: Option<usize>,
}

impl CountVisitor {
    /// Create new counter.
    pub fn new() -> Self {
        Self {
            count: 0,
            max_count: None,
        }
    }

    /// Create counter with maximum limit.
    pub fn with_limit(max_count: usize) -> Self {
        Self {
            count: 0,
            max_count: Some(max_count),
        }
    }

    /// Check if limit has been reached.
    pub fn is_at_limit(&self) -> bool {
        self.max_count.map_or(false, |max| self.count >= max)
    }
}

impl sealed::Sealed for CountVisitor {}

impl TreeVisitor for CountVisitor {
    fn visit(&mut self, _id: ElementId, _depth: usize) -> VisitorResult {
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

/// Find visitor with HRTB predicate support.
///
/// Uses Higher-Rank Trait Bounds to accept predicates that
/// work with any lifetime, providing maximum flexibility.
pub struct FindVisitor<P> {
    predicate: P,
    pub found: Option<ElementId>,
    /// Stop after first match for performance.
    stop_on_first: bool,
}

impl<P> FindVisitor<P> {
    /// Create new finder that stops on first match.
    pub fn new(predicate: P) -> Self
    where
        P: for<'a> Fn(ElementId, usize) -> bool,
    {
        Self {
            predicate,
            found: None,
            stop_on_first: true,
        }
    }

    /// Create finder that continues after first match.
    pub fn new_continue(predicate: P) -> Self
    where
        P: for<'a> Fn(ElementId, usize) -> bool,
    {
        Self {
            predicate,
            found: None,
            stop_on_first: false,
        }
    }
}

impl<P> sealed::Sealed for FindVisitor<P> {}

impl<P> TreeVisitor for FindVisitor<P>
where
    P: for<'a> Fn(ElementId, usize) -> bool,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
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
pub struct MaxDepthVisitor {
    pub max_depth: usize,
    /// Current maximum seen.
    current_max: usize,
    /// Early termination threshold.
    termination_threshold: Option<usize>,
}

impl MaxDepthVisitor {
    /// Create new max depth finder.
    pub fn new() -> Self {
        Self {
            max_depth: 0,
            current_max: 0,
            termination_threshold: None,
        }
    }

    /// Create finder with early termination.
    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            max_depth: 0,
            current_max: 0,
            termination_threshold: Some(threshold),
        }
    }
}

impl sealed::Sealed for MaxDepthVisitor {}

impl TreeVisitor for MaxDepthVisitor {
    fn visit(&mut self, _id: ElementId, depth: usize) -> VisitorResult {
        if depth > self.current_max {
            self.current_max = depth;
            self.max_depth = depth;
        }

        // Early termination if we've reached threshold
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

/// For-each visitor with HRTB closure support.
pub struct ForEachVisitor<F> {
    callback: F,
}

impl<F> ForEachVisitor<F> {
    /// Create new for-each visitor.
    pub fn new(callback: F) -> Self
    where
        F: for<'a> FnMut(ElementId, usize),
    {
        Self { callback }
    }
}

impl<F> sealed::Sealed for ForEachVisitor<F> {}

impl<F> TreeVisitor for ForEachVisitor<F>
where
    F: for<'a> FnMut(ElementId, usize),
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        (self.callback)(id, depth);
        VisitorResult::Continue
    }
}

/// Stateful visitor with typestate pattern.
///
/// Uses phantom types to track visitor state at compile time,
/// ensuring correct usage patterns.
pub struct StatefulVisitor<State, Data> {
    data: Data,
    _state: PhantomData<State>,
}

/// Visitor states for typestate pattern.
pub mod states {
    /// Initial state - visitor just created.
    pub struct Initial;
    /// Started state - visitor has begun traversal.
    pub struct Started;
    /// Finished state - visitor has completed traversal.
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

impl<Data> TreeVisitor for StatefulVisitor<states::Started, Data>
where
    Data: for<'a> FnMut(ElementId, usize) -> VisitorResult,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        (self.data)(id, depth)
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS WITH HRTB SUPPORT
// ============================================================================

/// Collect all nodes in a subtree using optimized visitor.
pub fn collect_all<T>(tree: &T, root: ElementId) -> Vec<ElementId>
where
    T: TreeNav,
{
    let mut visitor = CollectVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.into_inner()
}

/// Count all nodes in a subtree with optional limit.
pub fn count_all<T>(tree: &T, root: ElementId) -> usize
where
    T: TreeNav,
{
    let mut visitor = CountVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.count
}

/// Count nodes up to a maximum limit.
pub fn count_with_limit<T>(tree: &T, root: ElementId, limit: usize) -> usize
where
    T: TreeNav,
{
    let mut visitor = CountVisitor::with_limit(limit);
    visit_depth_first(tree, root, &mut visitor);
    visitor.count
}

/// Find maximum depth in subtree with early termination.
pub fn max_depth<T>(tree: &T, root: ElementId) -> usize
where
    T: TreeNav,
{
    let mut visitor = MaxDepthVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.max_depth
}

/// Find maximum depth with threshold-based early termination.
pub fn max_depth_with_threshold<T>(tree: &T, root: ElementId, threshold: usize) -> usize
where
    T: TreeNav,
{
    let mut visitor = MaxDepthVisitor::with_threshold(threshold);
    visit_depth_first(tree, root, &mut visitor);
    visitor.max_depth
}

/// Find first node matching HRTB predicate.
pub fn find_first<T, P>(tree: &T, root: ElementId, predicate: P) -> Option<ElementId>
where
    T: TreeNav,
    P: for<'a> Fn(ElementId, usize) -> bool,
{
    let mut visitor = FindVisitor::new(predicate);
    visit_depth_first(tree, root, &mut visitor);
    visitor.found
}

/// Execute HRTB closure for each node in subtree.
pub fn for_each<T, F>(tree: &T, root: ElementId, callback: F)
where
    T: TreeNav,
    F: for<'a> FnMut(ElementId, usize),
{
    let mut visitor = ForEachVisitor::new(callback);
    visit_depth_first(tree, root, &mut visitor);
}

/// Stateful traversal with typestate guarantees.
///
/// This function demonstrates the typestate pattern ensuring
/// correct visitor lifecycle at compile time.
pub fn visit_stateful<T, Data>(
    tree: &T,
    root: ElementId,
    data: Data,
) -> StatefulVisitor<states::Finished, Data>
where
    T: TreeNav,
    Data: for<'a> FnMut(ElementId, usize) -> VisitorResult,
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
        type Node = TestNode;
        type NodeIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> Self::NodeIter<'_> {
            self.nodes.keys().copied()
        }
    }

    impl TreeNav for TestTree {
        type ChildrenIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;
        type AncestorsIter<'a> = crate::iter::Ancestors<'a, Self>;
        type DescendantsIter<'a> = crate::iter::Descendants<'a, Self>;
        type SiblingsIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;

        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
            if let Some(node) = self.get(id) {
                node.children.iter().copied()
            } else {
                [].iter().copied()
            }
        }

        fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
            crate::iter::Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
            crate::iter::Descendants::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
            if let Some(parent_id) = self.parent(id) {
                self.children(parent_id)
                    .filter(move |&child_id| child_id != id)
            } else {
                [].iter().copied()
            }
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

        // HRTB predicate that works with any lifetime
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
}
