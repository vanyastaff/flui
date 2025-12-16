//! RenderTree - Slab-based render object storage with tree operations.
//!
//! This module provides `RenderTree`, the central data structure for storing
//! and managing render objects in the rendering pipeline.
//!
//! # Architecture
//!
//! ```text
//! RenderTree
//!   ├─ nodes: Slab<RenderNode>   (O(1) access by RenderId)
//!   └─ root: Option<RenderId>    (root of the tree)
//! ```
//!
//! # Slab Offset Pattern
//!
//! RenderId uses 1-based indexing (NonZeroUsize), while Slab uses 0-based:
//! - `RenderId(1)` → `nodes[0]`
//! - `RenderId(2)` → `nodes[1]`
//! - etc.
//!
//! # Flutter Equivalence
//!
//! In Flutter, render objects form a tree via parent/child pointers stored
//! directly on each object. We use a separate `RenderTree` structure with
//! Slab storage for:
//! - O(1) access by ID
//! - Cache-friendly contiguous memory
//! - Safe ID-based references (no raw pointers in user code)

mod render_tree;

pub use render_tree::{RenderNode, RenderTree};
