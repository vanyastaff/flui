//! Integration tests for typed geometry system
//!
//! Tests the complete flow: unit types, conversions, operations, GPU integration

use flui_types::geometry::*;

// =============================================================================
// Type Safety Tests
// =============================================================================

#[test]
fn test_type_safety_prevents_unit_mixing() {
    // Point<Pixels> and Point<DevicePixels> are different types
    let ui_point = Point::<Pixels>::new(px(100.0), px(200.0));
    let device_point = Point::<DevicePixels>::new(device_px(800), device_px(600));

    // This should NOT compile (type mismatch):
    // let bad = ui_point + Vec2::from(device_point); // ❌

    // Must explicitly convert using manual conversion:
    let device_as_pixels = Point::<Pixels>::new(px(device_point.x.0 as f32), px(device_point.y.0 as f32));
    let _ = ui_point + Vec2::from(device_as_pixels); // ✅
}

#[test]
fn test_point_vec2_size_interop() {
    // All types work together with same unit
    let origin = Point::<Pixels>::new(px(10.0), px(20.0));
    let displacement = Vec2::<Pixels>::new(px(5.0), px(10.0));
    let size = Size::<Pixels>::new(px(100.0), px(50.0));

    // Point + Vec2 = Point
    let new_pos = origin + displacement;
    assert_eq!(new_pos.x.0, 15.0);

    // Point - Point = Vec2
    let delta: Vec2<Pixels> = new_pos - origin;
    assert_eq!(delta.x.0, 5.0);

    // Size contains Point
    assert!(size.contains(new_pos));
}

// =============================================================================
// GPU Conversion Tests
// =============================================================================

#[test]
fn test_gpu_conversion_pipeline() {
    // UI coordinates
    let ui_pos = Point::<Pixels>::new(px(100.0), px(200.0));
    let _ui_size = Size::<Pixels>::new(px(400.0), px(300.0));

    // Scale for 2x display
    let scale_factor = 2.0;
    let scaled = ui_pos.x.scale(scale_factor);

    // Convert to device pixels
    let device = scaled.to_device_pixels();
    assert_eq!(device.0, 200);

    // Convert to GPU f32
    let gpu_pos: Point<f32> = ui_pos.cast();
    assert_eq!(gpu_pos.x, 100.0);

    // To array for vertex buffer
    let vertex_pos = gpu_pos.to_array();
    assert_eq!(vertex_pos, [100.0, 200.0]);
}

#[test]
fn test_cast_conversions() {
    let p = Point::<Pixels>::new(px(100.0), px(200.0));

    // Cast to f32
    let p_f32: Point<f32> = p.cast();
    assert_eq!(p_f32.x, 100.0);

    // Cast back
    let p_px: Point<Pixels> = p_f32.cast();
    assert_eq!(p_px.x.0, 100.0);
}

// =============================================================================
// Numeric Safety Tests
// =============================================================================

#[test]
fn test_validation_safety() {
    // Valid coordinates
    let valid = Point::<f32>::new(1.0, 2.0);
    assert!(valid.is_valid());
    assert!(!valid.is_nan());

    // Invalid coordinates (NaN)
    let invalid = Point::<f32>::new(f32::NAN, 2.0);
    assert!(!invalid.is_valid());
    assert!(invalid.is_nan());
}

#[test]
fn test_finite_checks() {
    // Finite values
    let p = Point::<Pixels>::new(px(100.0), px(200.0));
    assert!(p.x.is_finite());
    assert!(p.y.is_finite());

    // Non-finite values
    let nan_px = px(f32::NAN);
    assert!(nan_px.is_nan());
    assert!(!nan_px.is_finite());

    let inf_px = px(f32::INFINITY);
    assert!(inf_px.is_infinite());
    assert!(!inf_px.is_finite());
}

// =============================================================================
// Vector Operations Tests
// =============================================================================

#[test]
fn test_vector_operations() {
    let v1 = Vec2::<f32>::new(3.0, 4.0);
    let v2 = Vec2::<f32>::new(1.0, 0.0);

    // Length
    assert_eq!(v1.length(), 5.0);
    assert_eq!(v1.length_squared(), 25.0);

    // Normalize
    let n = v1.normalize();
    assert!((n.length() - 1.0).abs() < 0.001);

    // Dot and cross products
    assert_eq!(v1.dot(&v2), 3.0);
    assert_eq!(v1.cross(&v2), -4.0);

    // Angle
    let angle = v1.angle();
    assert!((angle - 0.927).abs() < 0.001);
}

#[test]
fn test_typed_vector_operations() {
    // Vectors with units
    let v1 = Vec2::<Pixels>::new(px(3.0), px(4.0));

    // Convert to f32 for calculations
    let v1_f32: Vec2<f32> = v1.cast();
    assert_eq!(v1_f32.length(), 5.0);
}

// =============================================================================
// GPUI Traits Tests
// =============================================================================

#[test]
fn test_along_trait() {
    let p = Point::<Pixels>::new(px(10.0), px(20.0));

    // Access along axis
    assert_eq!(p.along(Axis::Horizontal).0, 10.0);
    assert_eq!(p.along(Axis::Vertical).0, 20.0);
}

#[test]
fn test_utility_traits() {
    let p = Point::<Pixels>::new(px(10.0), px(20.0));

    // Half
    let half = p.half();
    assert_eq!(half.x.0, 5.0);

    // Negate using - operator
    let neg = -p;
    assert_eq!(neg.x.0, -10.0);

    // IsZero
    let zero = Point::<Pixels>::new(px(0.0), px(0.0));
    assert!(zero.is_zero());
    assert!(!p.is_zero());
}

// =============================================================================
// Real-World Scenarios
// =============================================================================

#[test]
fn test_ui_layout_scenario() {
    // Button layout
    let button_pos = Point::<Pixels>::new(px(10.0), px(20.0));
    let button_size = Size::<Pixels>::new(px(100.0), px(40.0));

    // Mouse click
    let mouse_pos = Point::<Pixels>::new(px(50.0), px(30.0));

    // Hit test - check if mouse is inside button bounds
    let relative = mouse_pos - button_pos;
    let relative_point = Point::new(relative.x, relative.y);
    let in_button = button_size.contains(relative_point);
    assert!(in_button);
}

#[test]
fn test_animation_scenario() {
    // Animation state
    let start = Point::<Pixels>::new(px(0.0), px(0.0));
    let _end = Point::<Pixels>::new(px(100.0), px(100.0));
    let velocity = Vec2::<Pixels>::new(px(10.0), px(10.0));

    // Update position
    let mut current = start;
    for _ in 0..10 {
        current += velocity;
    }

    assert_eq!(current.x.0, 100.0);
    assert_eq!(current.y.0, 100.0);
}

#[test]
fn test_coordinate_scaling_scenario() {
    // UI design at 1x
    let design_size = Size::<Pixels>::new(px(375.0), px(667.0)); // iPhone

    // Scale to 2x Retina
    let scaled = design_size * 2.0;
    assert_eq!(scaled.width.0, 750.0);

    // Convert to device pixels
    let device_width = scaled.width.scale(1.0).to_device_pixels();
    assert_eq!(device_width.0, 750);
}

// =============================================================================
// Cross-Type Conversions
// =============================================================================

#[test]
fn test_offset_vec2_interop() {
    let vec = Vec2::<Pixels>::new(px(10.0), px(20.0));
    let offset: Offset<Pixels> = vec.into();

    assert_eq!(offset.dx.0, 10.0);
    assert_eq!(offset.dy.0, 20.0);

    let vec2: Vec2<Pixels> = offset.into();
    assert_eq!(vec2.x.0, 10.0);
}

#[test]
fn test_size_point_conversions() {
    let size = Size::<Pixels>::new(px(100.0), px(200.0));
    // Convert size to point manually (using width as x, height as y)
    let point = Point::<Pixels>::new(size.width, size.height);

    assert_eq!(point.x.0, 100.0);
    assert_eq!(point.y.0, 200.0);
}

#[test]
fn test_point_vec2_conversions() {
    let point = Point::<Pixels>::new(px(10.0), px(20.0));
    let vec: Vec2<Pixels> = point.into();

    assert_eq!(vec.x.0, 10.0);
    assert_eq!(vec.y.0, 20.0);
}

// =============================================================================
// Multi-Unit Workflow Tests
// =============================================================================

#[test]
fn test_complete_rendering_pipeline() {
    // 1. UI coordinates (logical pixels)
    let ui_button = Bounds::<Pixels>::new(
        Point::new(px(10.0), px(20.0)),
        Size::new(px(100.0), px(50.0)),
    );

    // 2. Scale for 2x display
    let scale_factor = 2.0;
    let scaled_origin_x = ui_button.origin.x.scale(scale_factor);
    let scaled_origin_y = ui_button.origin.y.scale(scale_factor);

    // 3. Convert to device pixels
    let device_x = scaled_origin_x.to_device_pixels();
    let device_y = scaled_origin_y.to_device_pixels();

    assert_eq!(device_x.0, 20);
    assert_eq!(device_y.0, 40);

    // 4. Convert to GPU coordinates (f32) - manual conversion
    let gpu_origin: Point<f32> = ui_button.origin.cast();
    let gpu_size = Size::<f32>::new(ui_button.size.width.0, ui_button.size.height.0);
    assert_eq!(gpu_origin.x, 10.0);
    assert_eq!(gpu_size.width, 100.0);

    // 5. Export to vertex buffer
    let vertices = gpu_origin.to_array();
    assert_eq!(vertices, [10.0, 20.0]);
}

#[test]
fn test_mixed_unit_operations() {
    // Different unit types in the same workflow
    let logical = Point::<Pixels>::new(px(100.0), px(200.0));
    let device = Point::<DevicePixels>::new(device_px(200), device_px(400));
    let scaled = Point::<ScaledPixels>::new(scaled_px(150.0), scaled_px(300.0));

    // Convert all to f32 for comparison
    let logical_f32: Point<f32> = logical.cast();
    let device_f32 = Point::<f32>::new(device.x.0 as f32, device.y.0 as f32);
    let scaled_f32: Point<f32> = scaled.cast();

    assert_eq!(logical_f32.x, 100.0);
    assert_eq!(device_f32.x, 200.0);
    assert_eq!(scaled_f32.x, 150.0);
}

// =============================================================================
// Unit Arithmetic Tests
// =============================================================================

#[test]
fn test_pixel_arithmetic() {
    let a = px(100.0);
    let b = px(50.0);

    // Basic arithmetic
    assert_eq!((a + b).0, 150.0);
    assert_eq!((a - b).0, 50.0);
    assert_eq!((a * 2.0).0, 200.0);
    assert_eq!((a / 2.0).0, 50.0);

    // Division of pixels yields ratio
    let ratio = a / b;
    assert_eq!(ratio, 2.0);
}

#[test]
fn test_device_pixel_arithmetic() {
    let a = device_px(1920);
    let b = device_px(1080);

    // Basic arithmetic
    assert_eq!((a + b).0, 3000);
    assert_eq!((a - b).0, 840);
    assert_eq!((a * 2).0, 3840);

    // Division yields ratio
    let ratio = a / b;
    assert_eq!(ratio, 1);
}

#[test]
fn test_scaled_pixel_arithmetic() {
    let a = scaled_px(200.0);
    let b = scaled_px(100.0);

    // Basic arithmetic
    assert_eq!((a + b).0, 300.0);
    assert_eq!((a - b).0, 100.0);
    assert_eq!((a * 2.0).0, 400.0);
    assert_eq!((a / 2.0).0, 100.0);
}

// =============================================================================
// Bounds and Geometry Tests
// =============================================================================

#[test]
fn test_bounds_with_different_units() {
    // Bounds with Pixels
    let _pixel_bounds = Bounds::<Pixels>::new(
        Point::new(px(0.0), px(0.0)),
        Size::new(px(100.0), px(100.0)),
    );

    // Bounds with DevicePixels
    let device_bounds = Bounds::<DevicePixels>::new(
        Point::new(device_px(0), device_px(0)),
        Size::new(device_px(200), device_px(200)),
    );

    // Convert device to pixels (manually)
    let device_as_pixels = Bounds::<Pixels>::new(
        Point::new(px(device_bounds.origin.x.0 as f32), px(device_bounds.origin.y.0 as f32)),
        Size::new(px(device_bounds.size.width.0 as f32), px(device_bounds.size.height.0 as f32)),
    );
    assert_eq!(device_as_pixels.size.width.0, 200.0);
}

#[test]
fn test_edges_with_units() {
    let edges_px = Edges::<Pixels>::all(px(10.0));
    let edges_device = Edges::<DevicePixels>::all(device_px(20));

    assert_eq!(edges_px.top.0, 10.0);
    assert_eq!(edges_device.top.0, 20);

    // Symmetric edges: symmetric(vertical, horizontal)
    let edges = Edges::<Pixels>::symmetric(px(10.0), px(20.0));
    assert_eq!(edges.left.0, 20.0);  // horizontal
    assert_eq!(edges.top.0, 10.0);   // vertical
}

#[test]
fn test_corners_with_units() {
    let corners_px = Corners::<Pixels>::all(px(5.0));
    let corners_device = Corners::<DevicePixels>::all(device_px(10));

    assert_eq!(corners_px.top_left.0, 5.0);
    assert_eq!(corners_device.top_left.0, 10);
}

// =============================================================================
// Percentage and Rems Tests
// =============================================================================

#[test]
fn test_percentage_unit() {
    let half = Percentage(0.5);
    let quarter = Percentage(0.25);

    assert_eq!(half.0, 0.5);
    assert_eq!(quarter.0, 0.25);

    // Percentages can be used in calculations
    let size = px(100.0);
    let half_size = size * half.0;
    assert_eq!(half_size.0, 50.0);
}

#[test]
fn test_rems_unit() {
    let base_rem = rems(1.0);
    let larger = rems(1.5);

    assert_eq!(base_rem.0, 1.0);
    assert_eq!(larger.0, 1.5);

    // Convert rems to pixels (assuming 16px base)
    let base_font_size = 16.0;
    let pixels = larger.0 * base_font_size;
    assert_eq!(pixels, 24.0);
}

// =============================================================================
// Edge Cases and Validation
// =============================================================================

#[test]
fn test_zero_values() {
    let zero_px = Pixels::ZERO;
    let zero_device = DevicePixels::ZERO;
    let zero_scaled = ScaledPixels::ZERO;

    assert_eq!(zero_px.0, 0.0);
    assert_eq!(zero_device.0, 0);
    assert_eq!(zero_scaled.0, 0.0);

    // Zero points
    let zero_point = Point::<Pixels>::new(px(0.0), px(0.0));
    assert!(zero_point.is_zero());
}

#[test]
fn test_negative_values() {
    let neg_px = px(-10.0);
    let neg_device = device_px(-20);

    assert_eq!(neg_px.abs().0, 10.0);
    assert_eq!(neg_device.abs().0, 20);

    // Negative point using - operator
    let p = Point::<Pixels>::new(px(10.0), px(20.0));
    let neg = -p;
    assert_eq!(neg.x.0, -10.0);
    assert_eq!(neg.y.0, -20.0);
}

#[test]
fn test_rounding_operations() {
    let px_val = px(123.7);

    assert_eq!(px_val.floor().0, 123.0);
    assert_eq!(px_val.round().0, 124.0);
    assert_eq!(px_val.ceil().0, 124.0);
    assert_eq!(px_val.trunc().0, 123.0);
}

#[test]
fn test_min_max_clamp() {
    let a = px(100.0);
    let b = px(200.0);

    assert_eq!(a.min(b).0, 100.0);
    assert_eq!(a.max(b).0, 200.0);

    let c = px(150.0);
    assert_eq!(c.clamp(a, b).0, 150.0);

    let d = px(50.0);
    assert_eq!(d.clamp(a, b).0, 100.0);

    let e = px(250.0);
    assert_eq!(e.clamp(a, b).0, 200.0);
}

// =============================================================================
// Array and Tuple Conversions
// =============================================================================

#[test]
fn test_array_conversions() {
    let p = Point::<f32>::new(100.0, 200.0);
    let arr = p.to_array();
    assert_eq!(arr, [100.0, 200.0]);

    let v = Vec2::<f32>::new(10.0, 20.0);
    let arr2 = v.to_array();
    assert_eq!(arr2, [10.0, 20.0]);
}

#[test]
fn test_tuple_conversions() {
    // Size from tuple
    let size: Size<f32> = (100.0, 200.0).into();
    assert_eq!(size.width, 100.0);
    assert_eq!(size.height, 200.0);

    // Offset from tuple
    let offset: Offset<f32> = (10.0, 20.0).into();
    assert_eq!(offset.dx, 10.0);
    assert_eq!(offset.dy, 20.0);

    // Point from tuple
    let point: Point<f32> = (50.0, 75.0).into();
    assert_eq!(point.x, 50.0);
    assert_eq!(point.y, 75.0);
}

// =============================================================================
// Radians Tests
// =============================================================================

#[test]
fn test_radians_conversions() {
    use std::f32::consts::PI;

    // From degrees
    assert_eq!(Radians::from_degrees(180.0).0, PI);
    assert_eq!(Radians::from_degrees(90.0).0, PI / 2.0);

    // To degrees
    assert_eq!(radians(PI).to_degrees(), 180.0);
    assert_eq!(radians(PI / 2.0).to_degrees(), 90.0);

    // Normalize
    let normalized = radians(PI * 3.0).normalize();
    assert!((normalized.0 - PI).abs() < 0.0001);
}

#[test]
fn test_radians_from_percentage() {
    use std::f32::consts::PI;

    let half = Percentage(0.5);
    let r: Radians = half.into();
    assert!((r.0 - PI).abs() < 0.0001); // 50% = 180° = π

    let quarter = Percentage(0.25);
    let r: Radians = quarter.into();
    assert!((r.0 - PI / 2.0).abs() < 0.0001); // 25% = 90° = π/2
}
