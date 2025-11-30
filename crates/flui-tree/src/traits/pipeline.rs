//! Abstract pipeline traits for tree-based operations.
//!
//! These traits define patterns for layout, paint, and hit-test operations
//! without depending on concrete types. The concrete implementations live
//! in `flui_rendering`.
//!
//! # Design Philosophy
//!
//! This module provides:
//! - **Abstract patterns**: Generic traits that define how operations flow through the tree
//! - **Callback-based API**: Operations are performed via closures, avoiding type dependencies
//! - **Visitor patterns**: For walking the tree during different pipeline phases
//!
//! The concrete types (`BoxConstraints`, `Canvas`, `HitTestResult`) remain in `flui_rendering`.
//! This keeps `flui-tree` dependency-free while enabling maximum code reuse.

use crate::iter::RenderChildrenCollector;
use crate::traits::RenderTreeAccess;
use flui_foundation::ElementId;

// ============================================================================
// TREE VISITOR TRAITS
// ============================================================================

/// A visitor that processes nodes during tree traversal.
///
/// This trait enables generic tree walking algorithms that can be used
/// for layout, paint, hit-test, and other operations.
///
/// # Type Parameters
///
/// - `C`: Context passed down the tree (e.g., constraints, transform)
/// - `R`: Result accumulated up the tree (e.g., size, canvas, hit result)
pub trait TreeVisitor<C, R> {
    /// Called before visiting a node's children.
    ///
    /// Returns the context to pass to children, or `None` to skip children.
    fn pre_visit(&mut self, id: ElementId, context: &C) -> Option<C>;

    /// Called after visiting a node's children.
    ///
    /// Receives the results from all children and returns the combined result.
    fn post_visit(&mut self, id: ElementId, context: &C, child_results: Vec<R>) -> R;
}

/// A simpler visitor that doesn't need to combine child results.
pub trait SimpleTreeVisitor<C> {
    /// Visit a node with the given context.
    fn visit(&mut self, id: ElementId, context: &C);
}

// ============================================================================
// TREE OPERATION TRAIT
// ============================================================================

/// Defines a tree-based operation that can be applied to render elements.
///
/// This is the most generic form of tree operations, using callbacks
/// to avoid type dependencies.
///
/// # Example
///
/// ```rust,ignore
/// struct LayoutOperation<'a> {
///     tree: &'a mut MyTree,
/// }
///
/// impl TreeOperation for LayoutOperation<'_> {
///     type Input = BoxConstraints;
///     type Output = Size;
///     type Error = LayoutError;
///
///     fn apply(&mut self, id: ElementId, input: Self::Input) -> Result<Self::Output, Self::Error> {
///         // Perform layout...
///     }
/// }
/// ```
pub trait TreeOperation {
    /// Input passed down from parent to child.
    type Input;
    /// Output returned from child to parent.
    type Output;
    /// Error type for failures.
    type Error;

    /// Apply the operation to a single element.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the operation fails for this element.
    fn apply(&mut self, id: ElementId, input: Self::Input) -> Result<Self::Output, Self::Error>;

    /// Apply to children and collect results.
    fn apply_to_children(
        &mut self,
        children: &[ElementId],
        input: Self::Input,
    ) -> Vec<Result<Self::Output, Self::Error>>
    where
        Self::Input: Clone,
    {
        children
            .iter()
            .map(|&child| self.apply(child, input.clone()))
            .collect()
    }
}

// ============================================================================
// LAYOUT VISITOR TRAIT
// ============================================================================

/// Abstract layout visitor pattern.
///
/// This trait is implemented by tree types that support layout operations.
/// The concrete constraint and size types are specified by the implementor.
pub trait LayoutVisitable {
    /// Constraint type (e.g., `BoxConstraints`, `SliverConstraints`).
    type Constraints;
    /// Geometry result type (e.g., Size, `SliverGeometry`).
    type Geometry;
    /// Position type (e.g., Offset).
    type Position;

    /// Layout a single element with constraints.
    fn layout_element(&mut self, id: ElementId, constraints: Self::Constraints) -> Self::Geometry;

    /// Set the position of an element relative to its parent.
    fn set_position(&mut self, id: ElementId, position: Self::Position);

    /// Get the position of an element.
    fn get_position(&self, id: ElementId) -> Option<Self::Position>;

    /// Get the geometry of an element (after layout).
    fn get_geometry(&self, id: ElementId) -> Option<Self::Geometry>;
}

/// Extension trait for layout operations.
pub trait LayoutVisitableExt: LayoutVisitable + RenderTreeAccess {
    /// Layout all render children of an element.
    ///
    /// Uses [`RenderChildrenCollector`] to collect children first, avoiding borrow conflicts.
    fn layout_render_children(
        &mut self,
        parent: ElementId,
        constraints: Self::Constraints,
    ) -> Vec<Self::Geometry>
    where
        Self::Constraints: Clone,
    {
        let collector = RenderChildrenCollector::new(self, parent);
        let children = collector.into_variable();

        children
            .copied()
            .map(|child| self.layout_element(child, constraints.clone()))
            .collect()
    }
}

impl<T: LayoutVisitable + RenderTreeAccess> LayoutVisitableExt for T {}

// ============================================================================
// PAINT VISITOR TRAIT
// ============================================================================

/// Abstract paint visitor pattern.
pub trait PaintVisitable {
    /// Position/transform type.
    type Position;
    /// Canvas/paint result type.
    type PaintResult;

    /// Paint a single element at the given position.
    fn paint_element(&mut self, id: ElementId, position: Self::Position) -> Self::PaintResult;

    /// Combine paint results (e.g., compose canvases).
    fn combine_paint_results(&self, results: Vec<Self::PaintResult>) -> Self::PaintResult;
}

/// Extension trait for paint operations.
pub trait PaintVisitableExt: PaintVisitable + RenderTreeAccess {
    /// Paint all render children of an element.
    ///
    /// Uses [`RenderChildrenCollector`] to collect children first, avoiding borrow conflicts.
    fn paint_render_children(
        &mut self,
        parent: ElementId,
        base_position: Self::Position,
    ) -> Vec<Self::PaintResult>
    where
        Self::Position: Clone,
    {
        let collector = RenderChildrenCollector::new(self, parent);
        let children = collector.into_variable();

        children
            .copied()
            .map(|child| self.paint_element(child, base_position.clone()))
            .collect()
    }
}

impl<T: PaintVisitable + RenderTreeAccess> PaintVisitableExt for T {}

// ============================================================================
// HIT TEST VISITOR TRAIT
// ============================================================================

/// Abstract hit test visitor pattern.
pub trait HitTestVisitable {
    /// Position type for hit testing.
    type Position;
    /// Hit test accumulator type.
    type HitResult;

    /// Test if a position hits an element.
    ///
    /// Returns `true` if the element or any of its children were hit.
    fn hit_test_element(
        &self,
        id: ElementId,
        position: Self::Position,
        result: &mut Self::HitResult,
    ) -> bool;

    /// Transform a position for child hit testing.
    fn transform_position_for_child(
        &self,
        parent: ElementId,
        child: ElementId,
        position: Self::Position,
    ) -> Self::Position;
}

/// Extension trait for hit test operations.
pub trait HitTestVisitableExt: HitTestVisitable + RenderTreeAccess {
    /// Hit test all render children (in reverse order for proper z-ordering).
    ///
    /// Uses [`RenderChildrenCollector`] to collect children first, then iterates
    /// in reverse for proper z-order (topmost element tested first).
    fn hit_test_render_children(
        &self,
        parent: ElementId,
        position: Self::Position,
        result: &mut Self::HitResult,
    ) -> bool
    where
        Self::Position: Clone,
    {
        let collector = RenderChildrenCollector::new(self, parent);
        let children = collector.into_variable();

        // Iterate in reverse for proper z-order (topmost tested first)
        for &child in children.iter().rev() {
            let child_position = self.transform_position_for_child(parent, child, position.clone());
            if self.hit_test_element(child, child_position, result) {
                return true;
            }
        }
        false
    }
}

impl<T: HitTestVisitable + RenderTreeAccess> HitTestVisitableExt for T {}

// ============================================================================
// CALLBACK-BASED OPERATIONS
// ============================================================================

/// Callback-based layout operation.
///
/// This allows performing layout without knowing the concrete types at compile time.
/// The callbacks handle the type-specific logic.
pub fn layout_with_callback<T, C, G, F>(
    tree: &T,
    root: ElementId,
    initial_constraints: C,
    mut layout_fn: F,
) -> G
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, C, &[G]) -> G,
    C: Clone,
    G: Default,
{
    layout_recursive(tree, root, initial_constraints, &mut layout_fn)
}

fn layout_recursive<T, C, G, F>(tree: &T, id: ElementId, constraints: C, layout_fn: &mut F) -> G
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, C, &[G]) -> G,
    C: Clone,
    G: Default,
{
    use crate::iter::RenderChildren;

    if !tree.contains(id) || !tree.is_render_element(id) {
        return G::default();
    }

    // Layout children first (post-order traversal)
    let child_geometries: Vec<G> = RenderChildren::new(tree, id)
        .map(|child| layout_recursive(tree, child, constraints.clone(), layout_fn))
        .collect();

    // Then layout self with child results
    layout_fn(id, constraints, &child_geometries)
}

/// Callback-based paint operation.
pub fn paint_with_callback<T, P, R, F>(
    tree: &T,
    root: ElementId,
    initial_position: P,
    mut paint_fn: F,
) -> R
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, P, Vec<R>) -> R,
    P: Clone,
    R: Default,
{
    paint_recursive(tree, root, initial_position, &mut paint_fn)
}

fn paint_recursive<T, P, R, F>(tree: &T, id: ElementId, position: P, paint_fn: &mut F) -> R
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, P, Vec<R>) -> R,
    P: Clone,
    R: Default,
{
    use crate::iter::RenderChildren;

    if !tree.contains(id) || !tree.is_render_element(id) {
        return R::default();
    }

    // Paint children first
    let child_results: Vec<R> = RenderChildren::new(tree, id)
        .map(|child| paint_recursive(tree, child, position.clone(), paint_fn))
        .collect();

    // Then paint self
    paint_fn(id, position, child_results)
}

/// Callback-based hit test operation.
///
/// Returns `Some(path)` if hit, `None` if not hit.
/// The path contains all elements from root to hit element.
pub fn hit_test_with_callback<T, P, F>(
    tree: &T,
    root: ElementId,
    position: P,
    mut hit_test_fn: F,
) -> Option<Vec<ElementId>>
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, &P) -> bool,
    P: Clone,
{
    let mut path = Vec::new();
    if hit_test_recursive(tree, root, &position, &mut hit_test_fn, &mut path) {
        Some(path)
    } else {
        None
    }
}

fn hit_test_recursive<T, P, F>(
    tree: &T,
    id: ElementId,
    position: &P,
    hit_test_fn: &mut F,
    path: &mut Vec<ElementId>,
) -> bool
where
    T: RenderTreeAccess,
    F: FnMut(ElementId, &P) -> bool,
    P: Clone,
{
    if !tree.contains(id) || !tree.is_render_element(id) {
        return false;
    }

    // Check if this element is hit
    if !hit_test_fn(id, position) {
        return false;
    }

    // Add to path
    path.push(id);

    // Check children in reverse order (top-most first)
    let collector = RenderChildrenCollector::new(tree, id);
    let children = collector.into_variable();

    for &child in children.iter().rev() {
        if hit_test_recursive(tree, child, position, hit_test_fn, path) {
            return true;
        }
    }

    // This element was hit, but no children were
    true
}

// ============================================================================
// PHASE COORDINATOR TRAIT
// ============================================================================

/// Coordinates the different phases of the rendering pipeline.
///
/// This trait abstracts the pipeline phases (build, layout, paint)
/// allowing different implementations and testing.
pub trait PipelinePhaseCoordinator {
    /// Check if any work is pending in the build phase.
    fn has_pending_build(&self) -> bool;

    /// Check if any work is pending in the layout phase.
    fn has_pending_layout(&self) -> bool;

    /// Check if any work is pending in the paint phase.
    fn has_pending_paint(&self) -> bool;

    /// Check if any phase has pending work.
    fn has_pending_work(&self) -> bool {
        self.has_pending_build() || self.has_pending_layout() || self.has_pending_paint()
    }

    /// Request a build for an element.
    fn request_build(&mut self, id: ElementId);

    /// Request layout for an element.
    fn request_layout(&mut self, id: ElementId);

    /// Request paint for an element.
    fn request_paint(&mut self, id: ElementId);
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{TreeNav, TreeRead};
    use flui_foundation::Slot;
    use std::any::Any;

    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        is_render: bool,
        size: (f32, f32),
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(
            &mut self,
            parent: Option<ElementId>,
            is_render: bool,
            size: (f32, f32),
        ) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(TestNode {
                parent,
                children: Vec::new(),
                is_render,
                size,
            }));

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

        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
            None
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            self.render_object(id)
        }

        fn render_state_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    #[test]
    fn test_layout_with_callback() {
        let mut tree = TestTree::new();

        // Build: root -> [child1, child2]
        let root = tree.insert(None, true, (100.0, 100.0));
        let child1 = tree.insert(Some(root), true, (50.0, 30.0));
        let _child2 = tree.insert(Some(root), true, (50.0, 40.0));

        let result = layout_with_callback(
            &tree,
            root,
            (), // No constraints for test
            |id, _constraints, child_sizes: &[(f32, f32)]| {
                if let Some(node) = tree.get(id) {
                    if child_sizes.is_empty() {
                        node.size
                    } else {
                        // Sum heights of children
                        let total_height: f32 = child_sizes.iter().map(|(_, h)| h).sum();
                        (node.size.0, total_height)
                    }
                } else {
                    (0.0, 0.0)
                }
            },
        );

        // Root should have sum of child heights (30 + 40 = 70)
        assert_eq!(result, (100.0, 70.0));
    }

    #[test]
    fn test_hit_test_with_callback() {
        let mut tree = TestTree::new();

        let root = tree.insert(None, true, (100.0, 100.0));
        let child1 = tree.insert(Some(root), true, (50.0, 50.0));
        let _child2 = tree.insert(Some(root), true, (50.0, 50.0));

        // Hit test that only hits root and child1
        let path = hit_test_with_callback(&tree, root, (25.0, 25.0), |id, _pos| {
            // Simple hit test - all render elements are hit
            tree.is_render_element(id)
        });

        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.contains(&root));
    }
}
