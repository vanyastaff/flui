//! Integration tests for `LayerTree` + `LayerNode`.
//!
//! Extracted from `src/tree/layer_tree.rs` inline tests so
//! the production module stays focused on storage logic. Test scenarios are
//! preserved verbatim; the `use super::*; use crate::layer::CanvasLayer;`
//! pair becomes `use flui_layer::*;` for integration access.

use flui_foundation::ElementId;
use flui_layer::{CanvasLayer, Layer, LayerNode, LayerTree};
// Cycle 3 T-2: `tree.remove(id)` resolves through the unified trait.
use flui_tree::TreeWrite;
use flui_types::{Offset, geometry::px};

#[test]
fn test_layer_tree_new() {
    let tree = LayerTree::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    assert!(tree.root().is_none());
}

#[test]
fn test_layer_tree_with_capacity() {
    let tree = LayerTree::with_capacity(100);
    assert!(tree.is_empty());
}

#[test]
fn test_layer_tree_insert() {
    let mut tree = LayerTree::new();
    let layer = Layer::from(CanvasLayer::new());
    let id = tree.insert(layer);

    assert!(!tree.is_empty());
    assert_eq!(tree.len(), 1);
    assert!(tree.contains(id));
    assert_eq!(id.get(), 1); // First ID should be 1
}

#[test]
fn test_layer_tree_get() {
    let mut tree = LayerTree::new();
    let layer = Layer::from(CanvasLayer::new());
    let id = tree.insert(layer);

    let node = tree.get(id);
    assert!(node.is_some());
    assert!(node.unwrap().layer().is_canvas());
}

#[test]
fn test_layer_tree_get_layer() {
    let mut tree = LayerTree::new();
    let layer = Layer::from(CanvasLayer::new());
    let id = tree.insert(layer);

    let layer = tree.get_layer(id);
    assert!(layer.is_some());
    assert!(layer.unwrap().is_canvas());
}

#[test]
fn test_layer_tree_remove() {
    let mut tree = LayerTree::new();
    let layer = Layer::from(CanvasLayer::new());
    let id = tree.insert(layer);

    assert!(tree.contains(id));

    let removed = tree.remove(id);
    assert!(removed.is_some());
    assert!(!tree.contains(id));
    assert!(tree.is_empty());
}

#[test]
fn test_layer_tree_parent_child() {
    let mut tree = LayerTree::new();

    let parent_layer = Layer::from(CanvasLayer::new());
    let child_layer = Layer::from(CanvasLayer::new());

    let parent_id = tree.insert(parent_layer);
    let child_id = tree.insert(child_layer);

    tree.add_child(parent_id, child_id);

    // Check parent has child
    let children = tree.children(parent_id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], child_id);

    // Check child has parent
    let parent = tree.parent(child_id);
    assert_eq!(parent, Some(parent_id));
}

#[test]
fn test_layer_tree_remove_child() {
    let mut tree = LayerTree::new();

    let parent_id = tree.insert(Layer::from(CanvasLayer::new()));
    let child_id = tree.insert(Layer::from(CanvasLayer::new()));

    tree.add_child(parent_id, child_id);
    assert_eq!(tree.children(parent_id).unwrap().len(), 1);

    tree.remove_child(parent_id, child_id);
    assert_eq!(tree.children(parent_id).unwrap().len(), 0);
    assert!(tree.parent(child_id).is_none());
}

#[test]
fn test_layer_tree_set_root() {
    let mut tree = LayerTree::new();
    let id = tree.insert(Layer::from(CanvasLayer::new()));

    assert!(tree.root().is_none());
    tree.set_root(Some(id));
    assert_eq!(tree.root(), Some(id));
}

#[test]
fn test_layer_tree_clear() {
    let mut tree = LayerTree::new();
    let id = tree.insert(Layer::from(CanvasLayer::new()));
    tree.set_root(Some(id));

    tree.clear();
    assert!(tree.is_empty());
    assert!(tree.root().is_none());
}

#[test]
fn test_layer_tree_iter() {
    let mut tree = LayerTree::new();
    let id1 = tree.insert(Layer::from(CanvasLayer::new()));
    let id2 = tree.insert(Layer::from(CanvasLayer::new()));

    let ids: Vec<_> = tree.layer_ids().collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[test]
fn test_layer_node_with_element_id() {
    let element_id = ElementId::new(42);
    let node = LayerNode::new(Layer::from(CanvasLayer::new())).with_element_id(element_id);

    assert_eq!(node.element_id(), Some(element_id));
}

#[test]
fn test_layer_node_with_offset() {
    let offset = Offset::new(px(10.0), px(20.0));
    let node = LayerNode::new(Layer::from(CanvasLayer::new())).with_offset(offset);

    assert_eq!(node.offset(), Some(offset));
}

#[test]
fn test_layer_node_needs_compositing_delegates_to_enum() {
    // needs_compositing() delegates to the Layer enum method. The previous
    // cached field defaulted to `true` for every variant, which diverged
    // from `Layer::needs_compositing` — Canvas/Picture/Offset and friends
    // actually return `false`. Asserting the delegation contract here locks
    // the answer to the variant-computed value.
    let canvas = LayerNode::new(Layer::from(CanvasLayer::new()));
    assert!(
        !canvas.needs_compositing(),
        "Canvas layer does not need compositing"
    );
}

// ========== Layer Composition Tests ==========

#[test]
fn test_clear_children() {
    let mut tree = LayerTree::new();

    // Create parent with multiple children
    let parent_id = tree.insert(Layer::from(CanvasLayer::new()));
    let child1_id = tree.insert(Layer::from(CanvasLayer::new()));
    let child2_id = tree.insert(Layer::from(CanvasLayer::new()));
    let child3_id = tree.insert(Layer::from(CanvasLayer::new()));

    tree.add_child(parent_id, child1_id);
    tree.add_child(parent_id, child2_id);
    tree.add_child(parent_id, child3_id);

    // Verify children were added
    assert_eq!(tree.children(parent_id).unwrap().len(), 3);

    // Clear all children
    tree.clear_children(parent_id);

    // Verify children were cleared
    assert_eq!(tree.children(parent_id).unwrap().len(), 0);

    // Verify children still exist in tree (not removed, just unlinked)
    assert!(tree.contains(child1_id));
    assert!(tree.contains(child2_id));
    assert!(tree.contains(child3_id));

    // Verify children no longer have parent reference
    assert!(tree.parent(child1_id).is_none());
    assert!(tree.parent(child2_id).is_none());
    assert!(tree.parent(child3_id).is_none());
}

#[test]
fn test_append_layer() {
    let mut tree = LayerTree::new();

    // Create container layer
    let container_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Create picture layer
    let picture_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Append to container (Flutter PaintingContext pattern)
    tree.append_layer(container_id, picture_id);

    // Verify layer was appended
    let children = tree.children(container_id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], picture_id);

    // Verify parent-child relationship
    assert_eq!(tree.parent(picture_id), Some(container_id));
}

#[test]
fn test_append_layer_multiple_times() {
    let mut tree = LayerTree::new();

    let container_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer1_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer2_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer3_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Append layers one by one
    tree.append_layer(container_id, layer1_id);
    tree.append_layer(container_id, layer2_id);
    tree.append_layer(container_id, layer3_id);

    // Verify all layers were appended in order
    let children = tree.children(container_id).unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], layer1_id);
    assert_eq!(children[1], layer2_id);
    assert_eq!(children[2], layer3_id);
}

#[test]
fn test_append_layers_bulk() {
    let mut tree = LayerTree::new();

    let container_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer1_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer2_id = tree.insert(Layer::from(CanvasLayer::new()));
    let layer3_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Append multiple layers at once
    tree.append_layers(container_id, &[layer1_id, layer2_id, layer3_id]);

    // Verify all layers were appended in order
    let children = tree.children(container_id).unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], layer1_id);
    assert_eq!(children[1], layer2_id);
    assert_eq!(children[2], layer3_id);
}

#[test]
fn test_append_layers_empty() {
    let mut tree = LayerTree::new();

    let container_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Append empty slice - should be no-op
    tree.append_layers(container_id, &[]);

    // Verify no children were added
    let children = tree.children(container_id).unwrap();
    assert_eq!(children.len(), 0);
}

#[test]
fn test_layer_composition_integration() {
    // Simulate PaintingContext workflow:
    // 1. Create container layer (e.g., OffsetLayer)
    // 2. Record and append picture layers
    // 3. Clear and rebuild

    let mut tree = LayerTree::new();

    // Step 1: Create container
    let container_id = tree.insert(Layer::from(CanvasLayer::new()));

    // Step 2: Append some picture layers
    let picture1_id = tree.insert(Layer::from(CanvasLayer::new()));
    let picture2_id = tree.insert(Layer::from(CanvasLayer::new()));
    tree.append_layers(container_id, &[picture1_id, picture2_id]);

    assert_eq!(tree.children(container_id).unwrap().len(), 2);

    // Step 3: Clear and rebuild (simulating repaint)
    tree.clear_children(container_id);
    assert_eq!(tree.children(container_id).unwrap().len(), 0);

    // Append new layers
    let new_picture1_id = tree.insert(Layer::from(CanvasLayer::new()));
    let new_picture2_id = tree.insert(Layer::from(CanvasLayer::new()));
    let new_picture3_id = tree.insert(Layer::from(CanvasLayer::new()));
    tree.append_layers(
        container_id,
        &[new_picture1_id, new_picture2_id, new_picture3_id],
    );

    // Verify new structure
    let children = tree.children(container_id).unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], new_picture1_id);
    assert_eq!(children[1], new_picture2_id);
    assert_eq!(children[2], new_picture3_id);

    // Old picture layers should still exist (just unlinked)
    assert!(tree.contains(picture1_id));
    assert!(tree.contains(picture2_id));
}
