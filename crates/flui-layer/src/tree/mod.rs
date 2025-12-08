//! LayerTree - Tree storage for compositor layers
//!
//! This module implements the fourth of FLUI's five trees (View, Element, Render, Layer, Semantics).
//! Following Flutter's architecture, Layers are stored in a separate tree for compositor operations.
//!
//! # Architecture
//!
//! ```text
//! LayerTree
//!   ├─ nodes: Slab<LayerNodeStorage>
//!   └─ root: Option<LayerId>
//!
//! LayerNodeStorage
//!   └─ ConcreteLayerNode (layer, parent, children, metadata)
//! ```
//!
//! # Tree Trait Integration
//!
//! LayerTree implements `TreeRead<LayerId>` and `TreeNav<LayerId>` from flui-tree,
//! enabling generic tree algorithms and visitors.
//!
//! ```rust,ignore
//! use flui_layer::LayerTree;
//! use flui_tree::{TreeRead, TreeNav};
//!
//! let tree = LayerTree::new();
//!
//! // Use generic tree operations
//! for id in tree.node_ids() {
//!     if let Some(node) = tree.get(id) {
//!         // Process node
//!     }
//! }
//! ```

mod layer_tree;
mod tree_traits;

pub use layer_tree::{ConcreteLayerNode, LayerNode, LayerTree};
