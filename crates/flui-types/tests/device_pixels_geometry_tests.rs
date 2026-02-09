//! Tests for DevicePixels geometry operations (Phase 10: User Story 8)
//!
//! This module tests pixel-perfect GPU rendering with DevicePixels type,
//! including conversions and geometric operations.

use flui_types::geometry::{device_px, px, Offset, Point, Rect, Size, Vec2};

// ============================================================================
// Point<DevicePixels> Operations
// ============================================================================

#[test]
fn test_device_point_construction() {
    let p = Point::new(device_px(100), device_px(200));

    assert_eq!(p.x, device_px(100));
    assert_eq!(p.y, device_px(200));
}

#[test]
fn test_device_point_origin() {
    let origin = Point::origin();

    assert_eq!(origin.x.get(), 0);
    assert_eq!(origin.y.get(), 0);
}

#[test]
fn test_device_point_addition() {
    let p1 = Point::new(device_px(100), device_px(200));
    let offset = Offset::new(device_px(50), device_px(75));

    let p2 = p1 + offset;

    assert_eq!(p2.x, device_px(150));
    assert_eq!(p2.y, device_px(275));
}

#[test]
fn test_device_point_subtraction_gives_vec2() {
    let p1 = Point::new(device_px(100), device_px(200));
    let p2 = Point::new(device_px(30), device_px(50));

    let vec = p1 - p2;

    assert_eq!(vec.x, device_px(70));
    assert_eq!(vec.y, device_px(150));
}

#[test]
fn test_device_point_distance() {
    let p1 = Point::new(device_px(0), device_px(0));
    let p2 = Point::new(device_px(3), device_px(4));

    let dist = p1.distance(p2);

    // 3-4-5 triangle
    assert_eq!(dist, device_px(5));
}

#[test]
fn test_device_point_to_logical_pixels() {
    let device_point = Point::new(device_px(200), device_px(400));
    let scale = 2.0;

    let logical_point = Point::new(
        device_point.x.to_pixels(scale),
        device_point.y.to_pixels(scale),
    );

    assert_eq!(logical_point.x, px(100.0));
    assert_eq!(logical_point.y, px(200.0));
}

// ============================================================================
// Rect<DevicePixels> Operations
// ============================================================================

#[test]
fn test_device_rect_construction() {
    let rect = Rect::from_xywh(device_px(10), device_px(20), device_px(100), device_px(200));

    assert_eq!(rect.left(), device_px(10));
    assert_eq!(rect.top(), device_px(20));
    assert_eq!(rect.width(), device_px(100));
    assert_eq!(rect.height(), device_px(200));
}

#[test]
fn test_device_rect_from_ltrb() {
    let rect = Rect::from_ltrb(device_px(10), device_px(20), device_px(110), device_px(220));

    assert_eq!(rect.left(), device_px(10));
    assert_eq!(rect.top(), device_px(20));
    assert_eq!(rect.right(), device_px(110));
    assert_eq!(rect.bottom(), device_px(220));
    assert_eq!(rect.width(), device_px(100));
    assert_eq!(rect.height(), device_px(200));
}

#[test]
fn test_device_rect_contains_point() {
    let rect = Rect::from_xywh(device_px(0), device_px(0), device_px(100), device_px(100));

    let inside = Point::new(device_px(50), device_px(50));
    let outside = Point::new(device_px(150), device_px(150));
    let on_edge = Point::new(device_px(0), device_px(0));

    assert!(rect.contains(inside));
    assert!(!rect.contains(outside));
    assert!(rect.contains(on_edge));
}

#[test]
fn test_device_rect_intersect() {
    let rect1 = Rect::from_xywh(device_px(0), device_px(0), device_px(100), device_px(100));
    let rect2 = Rect::from_xywh(device_px(50), device_px(50), device_px(100), device_px(100));

    let intersection = rect1.intersect(&rect2);

    assert_eq!(intersection.left(), device_px(50));
    assert_eq!(intersection.top(), device_px(50));
    assert_eq!(intersection.right(), device_px(100));
    assert_eq!(intersection.bottom(), device_px(100));
}

#[test]
fn test_device_rect_intersect_no_overlap() {
    let rect1 = Rect::from_xywh(device_px(0), device_px(0), device_px(100), device_px(100));
    let rect2 = Rect::from_xywh(
        device_px(200),
        device_px(200),
        device_px(100),
        device_px(100),
    );

    let intersection = rect1.intersect(&rect2);

    assert!(intersection.is_empty());
}

#[test]
fn test_device_rect_union() {
    let rect1 = Rect::from_xywh(device_px(0), device_px(0), device_px(100), device_px(100));
    let rect2 = Rect::from_xywh(device_px(50), device_px(50), device_px(100), device_px(100));

    let union = rect1.union(&rect2);

    assert_eq!(union.left(), device_px(0));
    assert_eq!(union.top(), device_px(0));
    assert_eq!(union.right(), device_px(150));
    assert_eq!(union.bottom(), device_px(150));
}

#[test]
fn test_device_rect_inflate() {
    let rect = Rect::from_xywh(device_px(10), device_px(10), device_px(80), device_px(80));

    let inflated = rect.inflate(device_px(10));

    assert_eq!(inflated.left(), device_px(0));
    assert_eq!(inflated.top(), device_px(0));
    assert_eq!(inflated.right(), device_px(100));
    assert_eq!(inflated.bottom(), device_px(100));
}

#[test]
fn test_device_rect_inset() {
    let rect = Rect::from_xywh(device_px(0), device_px(0), device_px(100), device_px(100));

    let inset = rect.inset(device_px(10));

    assert_eq!(inset.left(), device_px(10));
    assert_eq!(inset.top(), device_px(10));
    assert_eq!(inset.right(), device_px(90));
    assert_eq!(inset.bottom(), device_px(90));
}

#[test]
fn test_device_rect_to_logical_pixels() {
    let device_rect = Rect::from_xywh(
        device_px(200),
        device_px(400),
        device_px(600),
        device_px(800),
    );
    let scale = 2.0;

    let logical_rect = Rect::from_xywh(
        device_rect.left().to_pixels(scale),
        device_rect.top().to_pixels(scale),
        device_rect.width().to_pixels(scale),
        device_rect.height().to_pixels(scale),
    );

    assert_eq!(logical_rect.left(), px(100.0));
    assert_eq!(logical_rect.top(), px(200.0));
    assert_eq!(logical_rect.width(), px(300.0));
    assert_eq!(logical_rect.height(), px(400.0));
}

// ============================================================================
// Size<DevicePixels> Operations
// ============================================================================

#[test]
fn test_device_size_construction() {
    let size = Size::new(device_px(100), device_px(200));

    assert_eq!(size.width, device_px(100));
    assert_eq!(size.height, device_px(200));
}

#[test]
fn test_device_size_area() {
    let size = Size::new(device_px(100), device_px(200));

    let area = size.area();

    // area() returns i32 for DevicePixels
    assert_eq!(area, 20000);
}

#[test]
fn test_device_size_is_empty() {
    let empty = Size::new(device_px(0), device_px(100));
    let non_empty = Size::new(device_px(100), device_px(200));

    assert!(empty.is_empty());
    assert!(!non_empty.is_empty());
}

// ============================================================================
// GPU Rendering Alignment Tests
// ============================================================================

#[test]
fn test_gpu_pixel_alignment_1x() {
    // On 1x display, logical pixels map directly to device pixels
    let scale = 1.0;
    let logical_rect = Rect::from_xywh(px(10.5), px(20.5), px(100.0), px(200.0));

    // Convert to device pixels (with rounding)
    let device_rect = Rect::from_xywh(
        logical_rect.left().to_device_pixels(scale),
        logical_rect.top().to_device_pixels(scale),
        logical_rect.width().to_device_pixels(scale),
        logical_rect.height().to_device_pixels(scale),
    );

    // Device pixels should be rounded to integers
    assert_eq!(device_rect.left(), device_px(11)); // 10.5 rounds to 11
    assert_eq!(device_rect.top(), device_px(21)); // 20.5 rounds to 21
    assert_eq!(device_rect.width(), device_px(100));
    assert_eq!(device_rect.height(), device_px(200));
}

#[test]
fn test_gpu_pixel_alignment_2x() {
    // On 2x display (retina), logical pixels are 2 device pixels
    let scale = 2.0;
    let logical_rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(200.0));

    let device_rect = Rect::from_xywh(
        logical_rect.left().to_device_pixels(scale),
        logical_rect.top().to_device_pixels(scale),
        logical_rect.width().to_device_pixels(scale),
        logical_rect.height().to_device_pixels(scale),
    );

    assert_eq!(device_rect.left(), device_px(20));
    assert_eq!(device_rect.top(), device_px(40));
    assert_eq!(device_rect.width(), device_px(200));
    assert_eq!(device_rect.height(), device_px(400));
}

#[test]
fn test_gpu_pixel_alignment_fractional_scale() {
    // On 1.5x display, 1 logical pixel = 1.5 device pixels
    let scale = 1.5;
    let logical_rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(200.0));

    let device_rect = Rect::from_xywh(
        logical_rect.left().to_device_pixels(scale),
        logical_rect.top().to_device_pixels(scale),
        logical_rect.width().to_device_pixels(scale),
        logical_rect.height().to_device_pixels(scale),
    );

    assert_eq!(device_rect.left(), device_px(15));
    assert_eq!(device_rect.top(), device_px(30));
    assert_eq!(device_rect.width(), device_px(150));
    assert_eq!(device_rect.height(), device_px(300));
}

// ============================================================================
// Real-World GPU Rendering Scenarios
// ============================================================================

#[test]
fn test_framebuffer_clipping_rect() {
    // Scissor rect for GPU clipping must be in device pixels
    let logical_clip = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(200.0));
    let scale = 2.0;

    let device_clip = Rect::from_xywh(
        logical_clip.left().to_device_pixels(scale),
        logical_clip.top().to_device_pixels(scale),
        logical_clip.width().to_device_pixels(scale),
        logical_clip.height().to_device_pixels(scale),
    );

    // Device pixels map 1:1 with framebuffer
    assert_eq!(device_clip.left().get(), 20);
    assert_eq!(device_clip.top().get(), 40);
    assert_eq!(device_clip.width().get(), 200);
    assert_eq!(device_clip.height().get(), 400);
}

#[test]
fn test_texture_atlas_coordinates() {
    // Texture atlas uses device pixels for precise UV mapping
    let glyph_rect = Rect::from_xywh(device_px(128), device_px(256), device_px(32), device_px(48));
    let atlas_size = Size::new(device_px(1024), device_px(1024));

    // Calculate UV coordinates (0.0 to 1.0)
    let u_min = glyph_rect.left().get() as f32 / atlas_size.width.get() as f32;
    let v_min = glyph_rect.top().get() as f32 / atlas_size.height.get() as f32;
    let u_max = glyph_rect.right().get() as f32 / atlas_size.width.get() as f32;
    let v_max = glyph_rect.bottom().get() as f32 / atlas_size.height.get() as f32;

    assert!((u_min - 0.125).abs() < 0.001); // 128/1024
    assert!((v_min - 0.25).abs() < 0.001); // 256/1024
    assert!((u_max - 0.15625).abs() < 0.001); // 160/1024
    assert!((v_max - 0.296875).abs() < 0.001); // 304/1024
}

#[test]
fn test_viewport_transformation() {
    // Window size in device pixels (framebuffer size)
    let viewport = Size::new(device_px(1920), device_px(1080));

    // Convert to logical pixels for layout
    let scale = 2.0;
    let logical_viewport = Size::new(
        viewport.width.to_pixels(scale),
        viewport.height.to_pixels(scale),
    );

    assert_eq!(logical_viewport.width, px(960.0));
    assert_eq!(logical_viewport.height, px(540.0));
}

#[test]
fn test_subpixel_rendering_alignment() {
    // Text rendering may use fractional logical coordinates
    let text_pos = Point::new(px(10.3333), px(20.6667));
    let scale = 3.0; // 3x display

    // Convert to device pixels for GPU rendering
    let device_pos = Point::new(
        text_pos.x.to_device_pixels(scale),
        text_pos.y.to_device_pixels(scale),
    );

    // Device pixels are always integers (rounded)
    assert_eq!(device_pos.x, device_px(31)); // 10.3333 * 3.0 = 31
    assert_eq!(device_pos.y, device_px(62)); // 20.6667 * 3.0 = 62
}

// ============================================================================
// Round-Trip Conversion Tests
// ============================================================================

#[test]
fn test_logical_to_device_to_logical_1x() {
    let original = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(200.0));
    let scale = 1.0;

    // Convert to device pixels
    let device_rect = Rect::from_xywh(
        original.left().to_device_pixels(scale),
        original.top().to_device_pixels(scale),
        original.width().to_device_pixels(scale),
        original.height().to_device_pixels(scale),
    );

    // Convert back to logical pixels
    let back = Rect::from_xywh(
        device_rect.left().to_pixels(scale),
        device_rect.top().to_pixels(scale),
        device_rect.width().to_pixels(scale),
        device_rect.height().to_pixels(scale),
    );

    assert_eq!(back.left(), original.left());
    assert_eq!(back.top(), original.top());
    assert_eq!(back.width(), original.width());
    assert_eq!(back.height(), original.height());
}

#[test]
fn test_logical_to_device_to_logical_2x() {
    let original = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(200.0));
    let scale = 2.0;

    let device_rect = Rect::from_xywh(
        original.left().to_device_pixels(scale),
        original.top().to_device_pixels(scale),
        original.width().to_device_pixels(scale),
        original.height().to_device_pixels(scale),
    );

    let back = Rect::from_xywh(
        device_rect.left().to_pixels(scale),
        device_rect.top().to_pixels(scale),
        device_rect.width().to_pixels(scale),
        device_rect.height().to_pixels(scale),
    );

    assert_eq!(back.left(), original.left());
    assert_eq!(back.top(), original.top());
    assert_eq!(back.width(), original.width());
    assert_eq!(back.height(), original.height());
}
