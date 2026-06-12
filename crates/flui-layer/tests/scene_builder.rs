//! Integration tests for `SceneBuilder` + `SceneCompositor`.
//!
//! Extracted from `src/compositor.rs` inline tests in Mythos Step 10.

use flui_foundation::LayerId;
use flui_layer::{CanvasLayer, Layer, LayerTree, SceneBuilder, SceneCompositor};
use flui_types::{
    geometry::px,
    painting::{Clip, TextureId},
    Matrix4, Offset, Rect,
};

#[test]
fn test_scene_builder_new() {
    let mut tree = LayerTree::new();
    let builder = SceneBuilder::new(&mut tree);

    assert_eq!(builder.depth(), 0);
    assert!(builder.current().is_none());
    assert!(builder.root().is_none());
}

#[test]
fn test_scene_builder_push_offset() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let id = builder.push_offset(Offset::new(px(10.0), px(20.0)));

    assert_eq!(builder.depth(), 1);
    assert_eq!(builder.current(), Some(id));
    assert_eq!(builder.root(), Some(id));
}

#[test]
fn test_scene_builder_push_pop() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let offset_id = builder.push_offset(Offset::new(px(10.0), px(20.0)));
    assert_eq!(builder.depth(), 1);

    let opacity_id = builder.push_opacity(0.5);
    assert_eq!(builder.depth(), 2);

    builder.pop().unwrap();
    assert_eq!(builder.depth(), 1);
    assert_eq!(builder.current(), Some(offset_id));

    builder.pop().unwrap();
    assert_eq!(builder.depth(), 0);

    // Root should still be offset
    assert_eq!(builder.root(), Some(offset_id));

    // Verify tree structure
    let children = tree.children(offset_id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], opacity_id);

    // Cross-check the same structure through the shared inspection walker
    // (dogfoods `testing::inspect::structure` on a SceneBuilder-built tree).
    assert_eq!(
        flui_layer::testing::inspect::structure(&tree),
        vec!["Offset", "Opacity"],
    );
}

#[test]
fn test_scene_builder_add_canvas() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let offset_id = builder.push_offset(Offset::ZERO);
    let canvas_id = builder.add_canvas(CanvasLayer::new());

    // Canvas should not be pushed onto stack
    assert_eq!(builder.depth(), 1);
    assert_eq!(builder.current(), Some(offset_id));

    // Finish building to release borrow
    let _root = builder.build();

    // Now we can check tree structure
    let children = tree.children(offset_id).unwrap();
    assert!(children.contains(&canvas_id));
}

#[test]
fn test_scene_builder_add_picture() {
    use flui_painting::Canvas;
    use flui_types::{painting::Paint, Color, Rect};

    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    // Record a picture
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
        &Paint::fill(Color::RED),
    );
    let picture = canvas.finish();

    // Add picture to scene
    let offset_id = builder.push_offset(Offset::ZERO);
    let picture_id = builder.add_picture(picture);

    // Picture should not be pushed onto stack
    assert_eq!(builder.depth(), 1);
    assert_eq!(builder.current(), Some(offset_id));

    // Finish building to release borrow
    let _root = builder.build();

    // Verify tree structure
    let children = tree.children(offset_id).unwrap();
    assert!(children.contains(&picture_id));

    // Verify layer type
    let layer = tree.get_layer(picture_id).unwrap();
    assert!(layer.is_picture());
}

#[test]
fn test_scene_builder_add_texture() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let _ = builder.push_offset(Offset::ZERO);
    let texture_id = builder.add_texture(
        TextureId::new(42),
        Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)),
    );

    let _ = builder.build();

    let layer = tree.get_layer(texture_id).unwrap();
    assert!(layer.is_texture());
}

#[test]
fn test_scene_builder_nested_transforms() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    // Build: offset -> opacity -> transform -> canvas
    let _ = builder.push_offset(Offset::new(px(100.0), px(50.0)));
    let _ = builder.push_opacity(0.8);
    let _ = builder.push_transform(Matrix4::scaling(2.0, 2.0, 1.0));
    let _ = builder.add_canvas(CanvasLayer::new());
    builder.pop().unwrap();
    builder.pop().unwrap();
    builder.pop().unwrap();

    let root = builder.build().unwrap();
    assert!(tree.get_layer(root).unwrap().is_offset());
}

#[test]
fn test_scene_builder_clip_rect() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let clip_id = builder.push_clip_rect(
        Rect::from_ltwh(px(0.0), px(0.0), px(200.0), px(200.0)),
        Clip::HardEdge,
    );
    builder.pop().unwrap();

    let layer = tree.get_layer(clip_id).unwrap();
    assert!(layer.is_clip_rect());
}

#[test]
fn test_scene_builder_build() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let _ = builder.push_offset(Offset::ZERO);
    let _ = builder.add_canvas(CanvasLayer::new());
    builder.pop().unwrap();

    let root = builder.build();
    assert!(root.is_some());

    // Tree should have root set
    assert_eq!(tree.root(), root);
}

#[test]
fn test_scene_builder_build_and_reset() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let _ = builder.push_offset(Offset::ZERO);
    builder.pop().unwrap();

    let root1 = builder.build_and_reset();
    assert!(root1.is_some());
    assert!(builder.root().is_none());
    assert_eq!(builder.depth(), 0);

    // Can build another scene
    let _ = builder.push_opacity(1.0);
    builder.pop().unwrap();

    let root2 = builder.build_and_reset();
    assert!(root2.is_some());
    assert_ne!(root1, root2);
}

#[test]
fn test_scene_builder_pop_to_depth() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    let _ = builder.push_offset(Offset::ZERO);
    let _ = builder.push_opacity(0.5);
    let _ = builder.push_transform(Matrix4::IDENTITY);
    assert_eq!(builder.depth(), 3);

    builder.pop_to_depth(1);
    assert_eq!(builder.depth(), 1);

    builder.pop_to_depth(0);
    assert_eq!(builder.depth(), 0);
}

#[test]
fn test_scene_builder_try_pop() {
    let mut tree = LayerTree::new();
    let mut builder = SceneBuilder::new(&mut tree);

    assert!(builder.try_pop().is_none());

    let id = builder.push_offset(Offset::ZERO);
    assert_eq!(builder.try_pop(), Some(id));
    assert!(builder.try_pop().is_none());
}

#[test]
fn test_scene_compositor_new() {
    let compositor = SceneCompositor::new();
    assert!(compositor.retained_layers().is_empty());
}

#[test]
fn test_scene_compositor_retain() {
    let mut compositor = SceneCompositor::new();
    let id = LayerId::new(1);

    compositor.retain(id);
    assert!(compositor.is_retained(id));
    assert_eq!(compositor.retained_layers().len(), 1);

    // Retaining same ID again should not duplicate
    compositor.retain(id);
    assert_eq!(compositor.retained_layers().len(), 1);
}

// `release` and `clear_retained` removed in U1 (zero-consumer scaffolding).
// SceneCompositor retention is currently write-only; reset must go through
// a fresh `SceneCompositor::new()`. Tests covering those deleted methods
// were removed alongside the methods.

#[test]
fn test_scene_compositor_stats() {
    let mut compositor = SceneCompositor::new();
    let mut tree = LayerTree::new();

    let _ = tree.insert(Layer::from(CanvasLayer::new()));
    let _ = tree.insert(Layer::from(CanvasLayer::new()));

    compositor.update_stats(&tree);
    assert_eq!(compositor.stats().total_layers, 2);

    compositor.reset_stats();
    assert_eq!(compositor.stats().total_layers, 0);
}
