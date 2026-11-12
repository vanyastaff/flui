//! The compositor tree.

pub(crate) mod annotations;
mod layer_tree;
// Compiled in every build, so `LayerNode` has one layout whatever the
// features: a hot-reload plugin and its host build this crate with
// different feature sets and pass layer trees across the library boundary.
#[cfg_attr(
    not(any(test, feature = "testing")),
    expect(dead_code, reason = "only the testing damage oracle stamps boundaries")
)]
mod stamp;

pub use layer_tree::{LayerNode, LayerTree};
#[cfg(any(test, feature = "testing"))]
pub use stamp::{BoundaryStamp, ContentToken};
