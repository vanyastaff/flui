//! Tree iterators for traversal.
//!
//! This module provides zero-allocation iterators for common tree
//! traversal patterns. All iterators are designed to:
//!
//! - **Be zero-allocation** for simple cases (using stack arrays)
//! - **Support arbitrary depths** (with heap fallback)
//! - **Be composable** with standard iterator adaptors
//!
//! # Iterator Types
//!
//! ## Ancestor Traversal
//!
//! - [`Ancestors`] - From node to root
//! - [`AncestorsWithDepth`] - With depth information
//! - [`RenderAncestors`] - Only render elements
//!
//! ## Descendant Traversal
//!
//! - [`Descendants`] - Pre-order depth-first (parent before children)
//! - [`DescendantsWithDepth`] - With depth information
//! - [`RenderDescendants`] - Only render elements
//!
//! ## Render-Specific Traversal
//!
//! - [`RenderChildren`] - Immediate render children (stops at render boundaries)
//! - [`RenderChildrenWithIndex`] - Render children with their index
//! - [`RenderSiblings`] - Render siblings of an element
//! - [`RenderSubtree`] - BFS traversal with depth info
//! - [`RenderLeaves`] - Leaf render elements (no render children)
//! - [`RenderPath`] - Path from root to a target element
//!
//! ## Configurable Traversal
//!
//! - [`DepthFirstIter`] - Pre-order or post-order DFS
//! - [`BreadthFirstIter`] - Level-order traversal
//!
//! ## Sibling Traversal
//!
//! - [`Siblings`] - Forward or backward through siblings
//!
//! # Utility Functions
//!
//! - [`find_render_ancestor`] - Find nearest render ancestor
//! - [`render_parent`] - Alias for `find_render_ancestor`
//! - [`collect_render_children`] - Collect all render children
//! - [`count_render_elements`] - Count render elements in subtree
//! - [`count_render_children`] - Count render children
//! - [`first_render_child`] / [`last_render_child`] - Get first/last render child
//! - [`nth_render_child`] - Get nth render child
//! - [`has_render_children`] - Check if element has render children
//! - [`is_render_leaf`] - Check if element is a render leaf
//! - [`find_render_root`] - Find topmost render ancestor
//! - [`render_depth`] - Calculate render depth
//! - [`is_render_descendant`] - Check if one element is descendant of another
//! - [`lowest_common_render_ancestor`] - Find LCA of two elements
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeNav, Ancestors, Descendants};
//!
//! // Find all ancestors
//! let path_to_root: Vec<_> = tree.ancestors(node_id).collect();
//!
//! // Find all descendants at depth 2
//! let level_2: Vec<_> = tree
//!     .descendants_with_depth(root)
//!     .filter(|(_, depth)| *depth == 2)
//!     .map(|(id, _)| id)
//!     .collect();
//! ```

mod ancestors;
mod breadth_first;
mod depth_first;
mod descendants;
mod render;
mod siblings;

pub use ancestors::{Ancestors, AncestorsWithDepth};
pub use breadth_first::BreadthFirstIter;
pub use depth_first::{DepthFirstIter, DepthFirstOrder};
pub use descendants::{Descendants, DescendantsWithDepth};
pub use render::{
    collect_render_children, count_render_children, count_render_elements, find_render_ancestor,
    find_render_root, first_render_child, has_render_children, is_render_descendant,
    is_render_leaf, last_render_child, lowest_common_render_ancestor, nth_render_child,
    render_depth, render_parent, RenderAncestors, RenderChildren, RenderChildrenCollector,
    RenderChildrenWithIndex, RenderDescendants, RenderLeaves, RenderPath, RenderSiblings,
    RenderSubtree, RenderSubtreeItem, SiblingDirection,
};
pub use siblings::{Siblings, SiblingsDirection};
