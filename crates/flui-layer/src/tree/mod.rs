//! The compositor tree.

// The tree-side reads of the retained scene's rules: the oracles its own
// reads are compared against.
#[cfg(any(test, feature = "testing"))]
mod annotations;
#[cfg(any(test, feature = "testing"))]
mod follower;
mod layer_tree;
// Compiled in every build, so `LayerNode` has one layout whatever the
// features: a hot-reload plugin and its host build this crate with
// different feature sets and pass layer trees across the library boundary.
#[cfg_attr(
    not(any(test, feature = "testing")),
    expect(dead_code, reason = "only the testing damage oracle stamps boundaries")
)]
mod stamp;

#[cfg(any(test, feature = "testing"))]
pub use follower::{leader_origin, resolve_follower_transform};
pub use layer_tree::{LayerNode, LayerTree};
#[cfg(any(test, feature = "testing"))]
pub use stamp::{BoundaryStamp, ContentToken};
