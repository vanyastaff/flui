//! Integration tests for the paint pipeline.
//!
//! These tests verify that painting works correctly through the render tree,
//! paint commands are recorded properly, and child painting propagates correctly.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::objects::r#box::basic::{RenderPadding, RenderSizedBox};
use flui_rendering::pipeline::{Paint, PaintingContext};
use flui_rendering::traits::{RenderBox, RenderObject};
use flui_types::geometry::Radius;
use flui_types::{EdgeInsets, Offset, Point, Rect, Size};

// ============================================================================
// Helper Functions
// ============================================================================

fn create_sized_box(width: f32, height: f32) -> Box<dyn RenderBox> {
    Box::new(RenderSizedBox::fixed(width, height))
}

fn layout_render_box(render_box: &mut dyn RenderBox, max_size: Size) -> Size {
    let constraints = BoxConstraints::loose(max_size);
    render_box.perform_layout(constraints)
}

// ============================================================================
// Basic Paint Tests
// ============================================================================

#[test]
fn test_painting_context_creation() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
    let context = PaintingContext::from_bounds(bounds);

    assert_eq!(context.estimated_bounds(), bounds);
}

#[test]
fn test_canvas_creation() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
    let mut context = PaintingContext::from_bounds(bounds);

    // Get canvas and verify it exists
    let canvas = context.canvas();
    assert!(canvas.is_empty());
}

#[test]
fn test_canvas_draw_rect() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let rect = Rect::from_ltwh(10.0, 10.0, 50.0, 50.0);
    let paint = Paint::default();
    context.canvas().draw_rect(rect, &paint);

    // Verify command was recorded
    assert_eq!(context.canvas().len(), 1);
}

#[test]
fn test_canvas_draw_circle() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let center = Point::new(100.0, 100.0);
    let radius = 50.0;
    let paint = Paint::default();
    context.canvas().draw_circle(center, radius, &paint);

    assert_eq!(context.canvas().len(), 1);
}

#[test]
fn test_canvas_draw_line() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(100.0, 100.0);
    let paint = Paint::default();
    context.canvas().draw_line(p1, p2, &paint);

    assert_eq!(context.canvas().len(), 1);
}

// ============================================================================
// Canvas State Tests
// ============================================================================

#[test]
fn test_canvas_save_restore() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    context.canvas().save();
    context.canvas().translate(10.0, 10.0);
    context.canvas().restore();

    // Canvas.len() only counts DrawCommands in display_list.
    // save/restore/translate modify internal state but don't add DrawCommands.
    // The transform is baked into subsequent draw commands.
    assert_eq!(context.canvas().len(), 0);
}

#[test]
fn test_canvas_translate() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    context.canvas().translate(50.0, 25.0);

    // translate() modifies internal transform state, doesn't add DrawCommand
    assert_eq!(context.canvas().len(), 0);
}

#[test]
fn test_canvas_scale() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    // Use scaled() which takes a single uniform scale factor
    context.canvas().scaled(2.0);

    // scaled() modifies internal transform state, doesn't add DrawCommand
    assert_eq!(context.canvas().len(), 0);
}

#[test]
fn test_canvas_rotate() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    context.canvas().rotate(std::f32::consts::PI / 4.0);

    // rotate() modifies internal transform state, doesn't add DrawCommand
    assert_eq!(context.canvas().len(), 0);
}

// ============================================================================
// Clip Tests
// ============================================================================

#[test]
fn test_canvas_clip_rect() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let clip_rect = Rect::from_ltwh(10.0, 10.0, 100.0, 100.0);
    context.canvas().clip_rect(clip_rect);

    assert_eq!(context.canvas().len(), 1);
}

#[test]
fn test_canvas_clip_rrect() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let clip_rrect = flui_types::RRect::from_rect_and_radius(
        Rect::from_ltwh(10.0, 10.0, 100.0, 100.0),
        Radius::circular(10.0),
    );
    context.canvas().clip_rrect(clip_rrect);

    assert_eq!(context.canvas().len(), 1);
}

// ============================================================================
// Multiple Commands Tests
// ============================================================================

#[test]
fn test_multiple_draw_commands() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    let paint = Paint::default();

    // Draw multiple shapes
    context
        .canvas()
        .draw_rect(Rect::from_ltwh(0.0, 0.0, 50.0, 50.0), &paint);
    context
        .canvas()
        .draw_rect(Rect::from_ltwh(60.0, 0.0, 50.0, 50.0), &paint);
    context
        .canvas()
        .draw_circle(Point::new(100.0, 100.0), 25.0, &paint);

    assert_eq!(context.canvas().len(), 3);
}

#[test]
fn test_complex_paint_sequence() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    let paint = Paint::default();

    // Complex sequence: save, clip, draw, restore
    context.canvas().save();
    context
        .canvas()
        .clip_rect(Rect::from_ltwh(10.0, 10.0, 100.0, 100.0));
    context
        .canvas()
        .draw_rect(Rect::from_ltwh(20.0, 20.0, 50.0, 50.0), &paint);
    context.canvas().restore();

    // Canvas.len() only counts DrawCommands: clip_rect + draw_rect = 2
    // save/restore modify internal state but don't add DrawCommands
    assert_eq!(context.canvas().len(), 2);
}

// ============================================================================
// Render Object Paint Tests
// ============================================================================

#[test]
fn test_sized_box_paint() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Layout first
    let size = layout_render_box(&mut sized_box, Size::new(200.0, 200.0));
    assert_eq!(size, Size::new(100.0, 50.0));

    // Paint
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    sized_box.paint(&mut context, Offset::ZERO);

    // SizedBox itself doesn't draw anything, just paints children
    // So canvas should be empty
    assert!(context.canvas().is_empty());
}

#[test]
fn test_padding_paint() {
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    // Layout first
    let size = layout_render_box(&mut padding, Size::new(200.0, 200.0));
    assert_eq!(size, Size::new(120.0, 70.0));

    // Paint
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    RenderBox::paint(&padding, &mut context, Offset::ZERO);

    // Padding paints its child at an offset
    // The actual child painting behavior depends on paint_child implementation
}

#[test]
fn test_paint_at_offset() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Layout first
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    // Paint at an offset
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    let offset = Offset::new(25.0, 25.0);
    sized_box.paint(&mut context, offset);

    // Verify painting occurred (sized_box doesn't draw, but method executes)
}

// ============================================================================
// Layer Tests
// ============================================================================

#[test]
fn test_stop_recording_creates_picture() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);
    let paint = Paint::default();

    // Draw something to start recording
    context
        .canvas()
        .draw_rect(Rect::from_ltwh(0.0, 0.0, 50.0, 50.0), &paint);

    // Stop recording
    context.stop_recording_if_needed();

    // Root layer should exist and contain picture layer
    let root = context.root_layer();
    assert!(root.is_some());
}

#[test]
fn test_repaint_composited_child() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);

    // Use the static method to repaint
    let _layer = PaintingContext::repaint_composited_child(bounds, |context, offset| {
        let paint = Paint::default();
        context.canvas().translate(offset.dx, offset.dy);
        context
            .canvas()
            .draw_rect(Rect::from_ltwh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    // Should return an OffsetLayer
}

// ============================================================================
// Paint Bounds Tests
// ============================================================================

#[test]
fn test_paint_bounds_sized_box() {
    let sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Before layout, paint_bounds should be zero
    let bounds = sized_box.paint_bounds();
    assert_eq!(bounds, Rect::from_ltwh(0.0, 0.0, 0.0, 0.0));
}

#[test]
fn test_paint_bounds_after_layout() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Layout
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    // After layout, paint_bounds should reflect size
    let bounds = sized_box.paint_bounds();
    assert_eq!(bounds, Rect::from_ltwh(0.0, 0.0, 100.0, 50.0));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_empty_canvas() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
    let mut context = PaintingContext::from_bounds(bounds);

    let canvas = context.canvas();
    assert!(canvas.is_empty());
    assert_eq!(canvas.bounds(), Rect::ZERO);
}

#[test]
fn test_zero_size_bounds() {
    let bounds = Rect::ZERO;
    let mut context = PaintingContext::from_bounds(bounds);

    // Should still be able to draw
    let paint = Paint::default();
    context
        .canvas()
        .draw_rect(Rect::from_ltwh(0.0, 0.0, 10.0, 10.0), &paint);

    assert_eq!(context.canvas().len(), 1);
}

#[test]
fn test_nested_save_restore() {
    let bounds = Rect::from_ltwh(0.0, 0.0, 200.0, 200.0);
    let mut context = PaintingContext::from_bounds(bounds);

    context.canvas().save();
    context.canvas().translate(10.0, 10.0);
    context.canvas().save();
    context.canvas().scaled(2.0);
    context.canvas().restore();
    context.canvas().restore();

    // Canvas.len() only counts DrawCommands in display_list.
    // save/restore/translate/scaled modify internal state but don't add DrawCommands.
    assert_eq!(context.canvas().len(), 0);
}
