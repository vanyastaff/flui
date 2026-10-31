//! Structured assertion helpers for snapshot-backed harness tests.
//!
//! Prefer these over substring-matching [`crate::testing::Probe::dump`] output
//! so CI can pin render-object contracts without visual inspection.

use flui_foundation::geometry::Size;
use flui_foundation::inspect::SnapshotNode;

/// Asserts that `node` writes every field name in `required`.
#[track_caller]
pub fn assert_properties(node: &SnapshotNode, required: &[&str]) {
    let names: Vec<&str> = node.fields.iter().map(|field| field.name).collect();
    for &req in required {
        assert!(
            names.contains(&req),
            "missing field {req:?} on {:?}; have {names:?}",
            node.kind,
        );
    }
}

/// Asserts that a descendant of kind `kind` exists, exactly once, and writes the `required`
/// fields of its own description (not the tree's facts).
#[track_caller]
pub fn assert_descendant_properties(tree: &SnapshotNode, kind: &str, required: &[&str]) {
    let node = tree
        .find_by_kind(kind)
        .unique()
        .unwrap_or_else(|error| panic!("{error} in the render snapshot"));
    assert_properties(node, required);
}

/// Asserts the tree recorded committed box geometry for the node.
#[track_caller]
pub fn assert_has_committed_size(node: &SnapshotNode) {
    assert!(
        node.get_fact::<Size>("size").is_ok(),
        "expected committed size on {:?}",
        node.kind,
    );
}

/// Asserts the tree recorded committed sliver geometry for the node.
#[track_caller]
pub fn assert_has_committed_geometry(node: &SnapshotNode) {
    assert!(
        node.fact("geometry").is_some(),
        "expected committed geometry on {:?}",
        node.kind,
    );
}
