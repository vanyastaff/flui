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
//! ## Configurable Traversal
//!
//! - [`DepthFirstIter`] - Pre-order or post-order DFS
//! - [`BreadthFirstIter`] - Level-order traversal
//!
//! ## Sibling Traversal
//!
//! - [`Siblings`] - Forward or backward through siblings
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
pub use render::{RenderAncestors, RenderChildren, RenderDescendants};
pub use siblings::{Siblings, SiblingsDirection};
