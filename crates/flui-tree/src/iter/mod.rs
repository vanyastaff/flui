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
//!
//! ## Descendant Traversal
//!
//! - [`Descendants`] - Pre-order depth-first (parent before children)
//! - [`DescendantsWithDepth`] - With depth information
//!
//! ## Configurable Traversal
//!
//! - `DepthFirstIter` - Pre-order or post-order DFS (planned)
//! - `BreadthFirstIter` - Level-order traversal (planned)
//!
//! ## Sibling Traversal
//!
//! - [`Siblings`] - Forward or backward through siblings
//!
//! # Design Philosophy
//!
//! flui-tree provides ONLY generic tree iterators. Domain-specific
//! iterators live in their respective crates:
//!
//! - **`flui_rendering`**: `RenderChildren`, `RenderAncestors`,
//!   `RenderDescendants`
//! - **flui-element**: Element-specific iterators
//!
//! # Example
//!
//! ```
//! # use flui_tree::{Ancestors, TreeNav, TreeRead};
//! # use flui_foundation::ElementId;
//! # struct N { parent: Option<ElementId>, children: Vec<ElementId> }
//! # struct T(Vec<Option<N>>);
//! # impl T { fn ins(&mut self, p: Option<ElementId>) -> ElementId {
//! #     let id = ElementId::new(self.0.len()+1);
//! #     self.0.push(Some(N { parent: p, children: vec![] }));
//! #     if let Some(pid) = p { self.0[pid.index() as usize].as_mut().unwrap().children.push(id); }
//! #     id
//! # }}
//! # impl TreeRead<ElementId> for T {
//! #     type Node = N;
//! #     fn get(&self, id: ElementId) -> Option<&N> { self.0.get(id.index() as usize)?.as_ref() }
//! #     fn len(&self) -> usize { self.0.iter().flatten().count() }
//! #     fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
//! #         (0..self.0.len()).filter_map(|i| if self.0[i].is_some() { Some(ElementId::new(i+1)) } else { None })
//! #     }
//! # }
//! # impl TreeNav<ElementId> for T {
//! #     fn parent(&self, id: ElementId) -> Option<ElementId> { self.get(id)?.parent }
//! #     fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
//! #         self.get(id).into_iter().flat_map(|n| n.children.iter().copied())
//! #     }
//! #     fn ancestors(&self, s: ElementId) -> impl Iterator<Item = ElementId> + '_ { Ancestors::new(self, s) }
//! #     fn descendants(&self, r: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
//! #         flui_tree::DescendantsWithDepth::new(self, r)
//! #     }
//! #     fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
//! #         self.parent(id).into_iter().flat_map(move |p| self.children(p).filter(move |&c| c != id))
//! #     }
//! # }
//! # let mut tree = T(vec![]);
//! # let root = tree.ins(None);
//! # let child1 = tree.ins(Some(root));
//! # let child2 = tree.ins(Some(root));
//! # let gc1 = tree.ins(Some(child1));
//! # let gc2 = tree.ins(Some(child2));
//! # let node_id = gc1;
//! use flui_tree::Descendants;
//!
//! // Find all ancestors
//! let path_to_root: Vec<_> = tree.ancestors(node_id).collect();
//!
//! // Find all descendants at depth 2
//! let level_2: Vec<_> = tree
//!     .descendants(root)
//!     .filter(|(_, depth)| *depth == 2)
//!     .map(|(id, _)| id)
//!     .collect();
//! assert_eq!(level_2, vec![gc1, gc2]);
//! ```

mod ancestors;
mod descendants;
mod siblings;
pub mod slot;

// Cycle 3 T-6: `breadth_first.rs`, `depth_first.rs`, `cursor.rs`,
// `path.rs` deleted (~3,800 LOC, zero in-workspace consumers per
// audit Appendix A). The kept iterators are the ones with real
// consumers: `Ancestors`, `Descendants`, `Siblings`, `Slot*`.

pub use ancestors::{Ancestors, AncestorsWithDepth};
pub use descendants::{Descendants, DescendantsWithDepth};
pub use siblings::{AllSiblings, Siblings, SiblingsDirection};
pub use slot::{IndexedSlot, Slot, SlotBuilder, SlotIter};
