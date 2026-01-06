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
//!             ├─ render_object: RwLock<Box<dyn RenderObject<P>>>
//!             ├─ state: RenderState<BoxProtocol>
//!             │    ├─ flags: AtomicRenderFlags
//!             │    ├─ geometry: OnceCell<Size>
//!             │    ├─ constraints: OnceCell<BoxConstraints>
//!             │    └─ offset: AtomicOffset
//!             └─ links: NodeLinks
//!                  ├─ parent: Option<RenderId>
//!                  ├─ children: Vec<RenderId>
//!                  └─ depth: u16
//! ```
//!
//! # Slab Offset Pattern
//!
//! RenderId uses 1-based indexing (NonZeroUsize), while Slab uses 0-based:
//! - `RenderId(1)` → `nodes[0]`
//! - `RenderId(2)` → `nodes[1]`
//!
//! # Flutter Equivalence
//!
//! In Flutter, render objects form a tree via parent/child pointers stored
//! directly on each object. We use a separate `RenderTree` structure with
//! Slab storage for:
//! - O(1) access by ID
//! - Cache-friendly contiguous memory
//! - Safe ID-based references (no raw pointers in user code)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::storage::{RenderTree, RenderNode};
//!
//! let mut tree = RenderTree::new();
//!
//! // Insert a Box protocol node
//! let root_id = tree.insert_box(Box::new(MyRenderBox::new()));
//! tree.set_root(Some(root_id));
//!
//! // Access the node
//! if let Some(node) = tree.get(root_id) {
//!     assert!(node.is_box());
//!     assert!(node.needs_layout());
//! }
//! ```

mod entry;
mod flags;
mod links;
mod node;
mod state;
mod tree;

// Public exports
pub use entry::RenderEntry;
pub use flags::{AtomicRenderFlags, RenderFlags};
pub use links::NodeLinks;
pub use node::RenderNode;
pub use state::RenderState;
pub use tree::RenderTree;

// Type aliases for convenience
pub use state::{BoxRenderState, SliverRenderState};
