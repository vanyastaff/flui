//! Test support: structural and diagnostic inspection of a [`LayerTree`].
//!
//! Compiled for this crate's own tests and for consumers that enable the
//! `testing` feature. Fixtures are built with [`SceneBuilder`] or the tree's
//! own [`LayerTree::new`] / [`LayerTree::push_child`]; this module
//! only reads.
//!
//! [`LayerTree`]: crate::LayerTree
//! [`LayerTree::new`]: crate::LayerTree::new
//! [`LayerTree::push_child`]: crate::LayerTree::push_child
//! [`SceneBuilder`]: crate::SceneBuilder

pub mod inspect;
