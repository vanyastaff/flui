//! RenderTree - Slab-based render object storage with tree operations.
//!
//! This module provides the core storage infrastructure for render objects:
//!
//! - [`RenderTree`]: Slab-based storage with O(1) access by RenderId
//! - [`RenderNode`]: Type-erased enum for heterogeneous tree storage
//! - [`RenderEntry`]: Protocol-specific storage unit
//! - [`NodeLinks`]: Shared tree structure data
//! - [`RenderState`]: Protocol-specific state (geometry, constraints, flags)
//!
//! # Architecture
//!
//! ```text
//! RenderTree
//!   └─ nodes: Slab<RenderNode>
//!        └─ RenderNode::Box(RenderEntry<BoxProtocol>)
//!             ├─ render_object: Box<dyn RenderObject<P>>  (owned by value, no lock)
//!             ├─ state: RenderState<BoxProtocol>
//!             │    ├─ flags: RenderFlagsCell
//!             │    ├─ layout: P::LayoutRecord  (last geometry and constraints)
//!             │    └─ offset: OffsetCell
//!             └─ links: NodeLinks
//!                  ├─ parent: Option<RenderId>
//!                  ├─ children: Vec<RenderId>
//!                  └─ depth: u16
//! ```
//!
//! # Ids
//!
//! A `RenderId` is a generational key: its index names a slab slot and its
//! generation the occupant, so an id kept past `remove` misses instead of
//! naming the node inserted into the same slot later.
//!
//! # Why a separate tree
//!
//! Render objects do not hold parent/child pointers themselves. A separate
//! `RenderTree` structure with Slab storage gives:
//! - O(1) access by ID
//! - Cache-friendly contiguous memory
//! - Safe ID-based references (no raw pointers in user code)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::{TextContextHandle, pipeline::PipelineOwner};
//!
//! // The owner holds the tree and edits it, marking what each edit
//! // invalidates; code outside this crate only reads the tree.
//! let mut owner = PipelineOwner::new(TextContextHandle::standalone());
//! let root_id = owner.set_root_render_object(Box::new(MyRenderBox::new()));
//!
//! // Access the node
//! if let Some(node) = owner.render_tree().get(root_id) {
//!     assert!(node.is_box());
//!     assert!(node.needs_layout());
//! }
//! ```

mod entry;
mod erased;
mod flags;
mod links;
mod node;
mod node_slab;
mod state;
mod tree;

// Public exports
pub use entry::RenderEntry;
pub use erased::{
    ErasedConstraints, ErasedConstraintsMismatch, ErasedGeometry, ErasedGeometryMismatch,
};
pub use flags::{RenderFlags, RenderFlagsCell};
pub use links::NodeLinks;
pub use node::RenderNode;
pub use state::RenderState;
pub use state::{BoxLayoutCache, IntrinsicDimension, ProtocolLayoutCache};
pub(crate) use state::{BoxLayoutRecord, LayoutRecord, SliverLayoutRecord};
// Type aliases for convenience
pub(crate) use node_slab::{EnterRefusal, EnteredNode, TreeLease};
pub use state::{BoxRenderState, SliverRenderState};
pub use tree::{ChildrenUpdate, RenderTree};
