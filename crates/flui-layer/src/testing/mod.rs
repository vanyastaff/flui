//! Test support: structural and diagnostic inspection of a [`LayerTree`].
//!
//! Compiled for this crate's own tests and for consumers that enable the
//! `testing` feature. Fixtures are built with the tree's own
//! [`LayerTree::new`] / [`LayerTree::push_child`], or read back from a scene
//! with [`SceneVersion::flatten`]; this module only reads.
//!
//! [`LayerTree`]: crate::LayerTree
//! [`LayerTree::new`]: crate::LayerTree::new
//! [`LayerTree::push_child`]: crate::LayerTree::push_child
//! [`SceneVersion::flatten`]: crate::SceneVersion::flatten

pub mod inspect;
