//! Structured assertion helpers for diagnostics-backed harness tests.
//!
//! Prefer these over substring-matching [`crate::testing::Probe::dump`] output
//! so CI can pin render-object contracts without visual inspection.

use flui_foundation::DiagnosticsNode;

/// Asserts that `node` exposes every property name in `required`.
#[track_caller]
pub fn assert_properties(node: &DiagnosticsNode, required: &[&str]) {
    let names: Vec<&str> = node
        .properties()
        .iter()
        .map(flui_foundation::DiagnosticsProperty::name)
        .collect();
    for &req in required {
        assert!(
            names.contains(&req),
            "missing property {req:?} on {:?}; have {names:?}",
            node.name(),
        );
    }
}

/// Asserts that a descendant typed `type_name` exists and has `required` config
/// properties on its self-description node (before counting runtime fields).
#[track_caller]
pub fn assert_descendant_properties(tree: &DiagnosticsNode, type_name: &str, required: &[&str]) {
    let node = tree
        .find_descendant(type_name)
        .unwrap_or_else(|| panic!("no descendant named {type_name:?} in diagnostics tree"));
    assert_properties(node, required);
}

/// Asserts the pipeline layered committed box geometry onto the node.
#[track_caller]
pub fn assert_has_committed_size(node: &DiagnosticsNode) {
    assert!(
        node.get_property("size").is_some(),
        "expected committed size on {:?}",
        node.name(),
    );
}

/// Asserts the pipeline layered committed sliver geometry onto the node.
#[track_caller]
pub fn assert_has_committed_geometry(node: &DiagnosticsNode) {
    assert!(
        node.get_property("geometry").is_some(),
        "expected committed geometry on {:?}",
        node.name(),
    );
}
