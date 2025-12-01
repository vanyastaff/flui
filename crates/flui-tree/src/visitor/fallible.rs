//! Fallible visitor pattern for error-aware tree traversal.
//!
//! This module provides visitor traits that can return errors during traversal,
//! enabling proper error handling and propagation in tree operations.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{FallibleVisitor, visit_fallible, TreeNav};
//!
//! struct ValidationVisitor;
//!
//! impl FallibleVisitor for ValidationVisitor {
//!     type Error = String;
//!
//!     fn visit(&mut self, id: ElementId, depth: usize) -> Result<VisitorResult, Self::Error> {
//!         if depth > 100 {
//!             return Err("Tree too deep".to_string());
//!         }
//!         Ok(VisitorResult::Continue)
//!     }
//! }
//!
//! let result = visit_fallible(&tree, root, &mut ValidationVisitor);
//! ```

use super::{sealed, VisitorResult};
use crate::TreeNav;
use flui_foundation::ElementId;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

// ============================================================================
// FALLIBLE VISITOR TRAIT
// ============================================================================

/// A visitor that can fail during traversal.
///
/// Unlike [`TreeVisitor`](super::TreeVisitor), this visitor returns
/// `Result<VisitorResult, E>`, allowing proper error handling.
pub trait FallibleVisitor: sealed::Sealed {
    /// The error type returned by this visitor.
    type Error: Error + Send + Sync + 'static;

    /// Visit a node, potentially returning an error.
    ///
    /// # Arguments
    ///
    /// * `id` - The element being visited
    /// * `depth` - Depth from traversal root (0-based)
    ///
    /// # Returns
    ///
    /// - `Ok(VisitorResult)` - Continue with the given traversal directive
    /// - `Err(error)` - Stop traversal and propagate the error
    fn visit(&mut self, id: ElementId, depth: usize) -> Result<VisitorResult, Self::Error>;

    /// Called before visiting children (optional hook).
    ///
    /// # Returns
    ///
    /// - `Ok(())` - Continue to children
    /// - `Err(error)` - Stop traversal and propagate the error
    #[inline]
    fn pre_children(&mut self, _id: ElementId, _depth: usize) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called after visiting all children (optional hook).
    #[inline]
    fn post_children(&mut self, _id: ElementId, _depth: usize) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// A visitor with mutable tree access that can fail.
pub trait FallibleVisitorMut<T: TreeNav>: sealed::Sealed {
    /// The error type returned by this visitor.
    type Error: Error + Send + Sync + 'static;

    /// The output type for this visitor (GAT).
    type Output<'a>
    where
        T: 'a,
        Self: 'a;

    /// Visit a node with tree access, potentially returning an error.
    fn visit<'a>(
        &'a mut self,
        tree: &'a T,
        id: ElementId,
        depth: usize,
    ) -> Result<(VisitorResult, Option<Self::Output<'a>>), Self::Error>
    where
        T: 'a;
}

// ============================================================================
// VISITOR ERROR WRAPPER
// ============================================================================

/// Error wrapper for visitor errors with context.
#[derive(Debug)]
pub struct VisitorError<E: Error> {
    /// The underlying error.
    pub inner: E,
    /// The element where the error occurred.
    pub element: ElementId,
    /// The depth at which the error occurred.
    pub depth: usize,
    /// Optional path from root to error location.
    pub path: Option<Vec<ElementId>>,
}

impl<E: Error> VisitorError<E> {
    /// Create a new visitor error.
    pub fn new(inner: E, element: ElementId, depth: usize) -> Self {
        Self {
            inner,
            element,
            depth,
            path: None,
        }
    }

    /// Create with path information.
    pub fn with_path(inner: E, element: ElementId, depth: usize, path: Vec<ElementId>) -> Self {
        Self {
            inner,
            element,
            depth,
            path: Some(path),
        }
    }

    /// Get the underlying error.
    pub fn into_inner(self) -> E {
        self.inner
    }
}

impl<E: Error> fmt::Display for VisitorError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "visitor error at element {} (depth {}): {}",
            self.element.get(),
            self.depth,
            self.inner
        )
    }
}

impl<E: Error + 'static> Error for VisitorError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.inner)
    }
}

// ============================================================================
// TRAVERSAL FUNCTIONS
// ============================================================================

/// Depth-first traversal with a fallible visitor.
///
/// Stops traversal on first error and returns it.
pub fn visit_fallible<T, V>(
    tree: &T,
    root: ElementId,
    visitor: &mut V,
) -> Result<bool, VisitorError<V::Error>>
where
    T: TreeNav,
    V: FallibleVisitor,
{
    visit_fallible_impl(tree, root, 0, visitor)
}

fn visit_fallible_impl<T, V>(
    tree: &T,
    node: ElementId,
    depth: usize,
    visitor: &mut V,
) -> Result<bool, VisitorError<V::Error>>
where
    T: TreeNav,
    V: FallibleVisitor,
{
    // Visit current node
    let result = visitor
        .visit(node, depth)
        .map_err(|e| VisitorError::new(e, node, depth))?;

    match result {
        VisitorResult::Stop => return Ok(false),
        VisitorResult::SkipChildren | VisitorResult::SkipSiblings => return Ok(true),
        _ => {}
    }

    if result.should_visit_children() {
        visitor
            .pre_children(node, depth)
            .map_err(|e| VisitorError::new(e, node, depth))?;

        let children: Vec<ElementId> = tree.children(node).collect();

        for child in children {
            if !visit_fallible_impl(tree, child, depth + 1, visitor)? {
                return Ok(false);
            }
        }

        visitor
            .post_children(node, depth)
            .map_err(|e| VisitorError::new(e, node, depth))?;
    }

    Ok(true)
}

/// Breadth-first traversal with a fallible visitor.
pub fn visit_fallible_breadth_first<T, V>(
    tree: &T,
    root: ElementId,
    visitor: &mut V,
) -> Result<bool, VisitorError<V::Error>>
where
    T: TreeNav,
    V: FallibleVisitor,
{
    let mut queue: VecDeque<(ElementId, usize)> = VecDeque::with_capacity(128);
    queue.push_back((root, 0));

    while let Some((node, depth)) = queue.pop_front() {
        let result = visitor
            .visit(node, depth)
            .map_err(|e| VisitorError::new(e, node, depth))?;

        match result {
            VisitorResult::Stop => return Ok(false),
            VisitorResult::SkipChildren => continue,
            _ => {}
        }

        if result.should_visit_children() {
            for child in tree.children(node) {
                queue.push_back((child, depth + 1));
            }
        }
    }

    Ok(true)
}

/// Traverse with path tracking for detailed error context.
pub fn visit_fallible_with_path<T, V>(
    tree: &T,
    root: ElementId,
    visitor: &mut V,
) -> Result<bool, VisitorError<V::Error>>
where
    T: TreeNav,
    V: FallibleVisitor,
{
    let mut path = vec![root];
    visit_fallible_with_path_impl(tree, root, 0, &mut path, visitor)
}

fn visit_fallible_with_path_impl<T, V>(
    tree: &T,
    node: ElementId,
    depth: usize,
    path: &mut Vec<ElementId>,
    visitor: &mut V,
) -> Result<bool, VisitorError<V::Error>>
where
    T: TreeNav,
    V: FallibleVisitor,
{
    let result = visitor
        .visit(node, depth)
        .map_err(|e| VisitorError::with_path(e, node, depth, path.clone()))?;

    match result {
        VisitorResult::Stop => return Ok(false),
        VisitorResult::SkipChildren | VisitorResult::SkipSiblings => return Ok(true),
        _ => {}
    }

    if result.should_visit_children() {
        let children: Vec<ElementId> = tree.children(node).collect();

        for child in children {
            path.push(child);
            let continue_traversal =
                visit_fallible_with_path_impl(tree, child, depth + 1, path, visitor)?;
            path.pop();

            if !continue_traversal {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

// ============================================================================
// BUILT-IN FALLIBLE VISITORS
// ============================================================================

/// A fallible visitor that validates depth limits.
pub struct DepthLimitVisitor {
    max_depth: usize,
}

impl DepthLimitVisitor {
    /// Create a new depth limit visitor.
    pub fn new(max_depth: usize) -> Self {
        Self { max_depth }
    }
}

/// Error returned when depth limit is exceeded.
#[derive(Debug, Clone)]
pub struct DepthLimitExceeded {
    pub actual_depth: usize,
    pub max_depth: usize,
}

impl fmt::Display for DepthLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "depth limit exceeded: {} > {}",
            self.actual_depth, self.max_depth
        )
    }
}

impl Error for DepthLimitExceeded {}

impl sealed::Sealed for DepthLimitVisitor {}

impl FallibleVisitor for DepthLimitVisitor {
    type Error = DepthLimitExceeded;

    fn visit(&mut self, _id: ElementId, depth: usize) -> Result<VisitorResult, Self::Error> {
        if depth > self.max_depth {
            Err(DepthLimitExceeded {
                actual_depth: depth,
                max_depth: self.max_depth,
            })
        } else {
            Ok(VisitorResult::Continue)
        }
    }
}

/// A fallible visitor that applies a fallible closure.
pub struct TryForEachVisitor<F, E> {
    callback: F,
    _marker: std::marker::PhantomData<E>,
}

impl<F, E> TryForEachVisitor<F, E> {
    /// Create a new try-for-each visitor.
    pub fn new(callback: F) -> Self
    where
        F: FnMut(ElementId, usize) -> Result<(), E>,
        E: Error + Send + Sync + 'static,
    {
        Self {
            callback,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<F, E> sealed::Sealed for TryForEachVisitor<F, E> {}

impl<F, E> FallibleVisitor for TryForEachVisitor<F, E>
where
    F: FnMut(ElementId, usize) -> Result<(), E>,
    E: Error + Send + Sync + 'static,
{
    type Error = E;

    fn visit(&mut self, id: ElementId, depth: usize) -> Result<VisitorResult, Self::Error> {
        (self.callback)(id, depth)?;
        Ok(VisitorResult::Continue)
    }
}

/// A fallible visitor that collects with validation.
pub struct TryCollectVisitor<F, E> {
    predicate: F,
    collected: Vec<ElementId>,
    _marker: std::marker::PhantomData<E>,
}

impl<F, E> TryCollectVisitor<F, E> {
    /// Create a new try-collect visitor.
    pub fn new(predicate: F) -> Self
    where
        F: FnMut(ElementId, usize) -> Result<bool, E>,
        E: Error + Send + Sync + 'static,
    {
        Self {
            predicate,
            collected: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Get collected elements.
    pub fn collected(&self) -> &[ElementId] {
        &self.collected
    }

    /// Consume and return collected elements.
    pub fn into_collected(self) -> Vec<ElementId> {
        self.collected
    }
}

impl<F, E> sealed::Sealed for TryCollectVisitor<F, E> {}

impl<F, E> FallibleVisitor for TryCollectVisitor<F, E>
where
    F: FnMut(ElementId, usize) -> Result<bool, E>,
    E: Error + Send + Sync + 'static,
{
    type Error = E;

    fn visit(&mut self, id: ElementId, depth: usize) -> Result<VisitorResult, Self::Error> {
        if (self.predicate)(id, depth)? {
            self.collected.push(id);
        }
        Ok(VisitorResult::Continue)
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Execute a fallible closure for each node.
pub fn try_for_each<T, F, E>(tree: &T, root: ElementId, callback: F) -> Result<(), VisitorError<E>>
where
    T: TreeNav,
    F: FnMut(ElementId, usize) -> Result<(), E>,
    E: Error + Send + Sync + 'static,
{
    let mut visitor = TryForEachVisitor::new(callback);
    visit_fallible(tree, root, &mut visitor)?;
    Ok(())
}

/// Collect nodes with validation, stopping on first error.
pub fn try_collect<T, F, E>(
    tree: &T,
    root: ElementId,
    predicate: F,
) -> Result<Vec<ElementId>, VisitorError<E>>
where
    T: TreeNav,
    F: FnMut(ElementId, usize) -> Result<bool, E>,
    E: Error + Send + Sync + 'static,
{
    let mut visitor = TryCollectVisitor::new(predicate);
    visit_fallible(tree, root, &mut visitor)?;
    Ok(visitor.into_collected())
}

/// Validate tree depth doesn't exceed limit.
pub fn validate_depth<T: TreeNav>(
    tree: &T,
    root: ElementId,
    max_depth: usize,
) -> Result<(), VisitorError<DepthLimitExceeded>> {
    let mut visitor = DepthLimitVisitor::new(max_depth);
    visit_fallible(tree, root, &mut visitor)?;
    Ok(())
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_depth_limit_exceeded() {
        let error = DepthLimitExceeded {
            actual_depth: 150,
            max_depth: 100,
        };

        assert!(error.to_string().contains("150"));
        assert!(error.to_string().contains("100"));
    }

    #[test]
    fn test_visitor_error() {
        let inner = DepthLimitExceeded {
            actual_depth: 10,
            max_depth: 5,
        };
        let id = ElementId::new(42);
        let error = VisitorError::new(inner, id, 10);

        assert_eq!(error.element, id);
        assert_eq!(error.depth, 10);
        assert!(error.to_string().contains("42"));
        assert!(error.to_string().contains("10"));
    }

    #[test]
    fn test_visitor_error_with_path() {
        let inner = DepthLimitExceeded {
            actual_depth: 10,
            max_depth: 5,
        };
        let id = ElementId::new(42);
        let path = vec![ElementId::new(1), ElementId::new(2), id];
        let error = VisitorError::with_path(inner, id, 10, path.clone());

        assert!(error.path.is_some());
        assert_eq!(error.path.as_ref().unwrap().len(), 3);
    }
}
