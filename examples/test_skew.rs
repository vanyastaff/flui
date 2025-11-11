//! Test Skew Transform
//!
//! This example demonstrates the skew transform (currently a stub).
//! Skew implementation is complete in Transform API and WgpuPainter.
//!
//! To test skew visually:
//! 1. Use Canvas::transform(Transform::skew(skew_x, skew_y))
//! 2. Draw shapes or text after applying the transform
//! 3. The skew will be decomposed by WgpuPainter::transform_matrix()

use flui_types::geometry::Transform;
use std::f32::consts::PI;

fn main() {
    println!("=== Skew Transform Test ===\n");

    // Demonstrate Transform API with skew
    println!("1. Italic text effect (horizontal skew):");
    let italic = Transform::skew(0.2, 0.0);
    println!("   Transform::skew(0.2, 0.0) = {:?}", italic);
    println!("   Shear angle: {:.1}°", 0.2f32.to_degrees());

    println!("\n2. Strong italic (more horizontal skew):");
    let strong_italic = Transform::skew(0.4, 0.0);
    println!("   Transform::skew(0.4, 0.0) = {:?}", strong_italic);
    println!("   Shear angle: {:.1}°", 0.4f32.to_degrees());

    println!("\n3. Perspective effect (both axes):");
    let perspective = Transform::skew(0.3, 0.3);
    println!("   Transform::skew(0.3, 0.3) = {:?}", perspective);

    println!("\n4. Composing skew with other transforms:");
    let composed = Transform::translate(50.0, 50.0)
        .then(Transform::skew(0.2, 0.0))
        .then(Transform::scale(1.5));
    println!("   translate → skew → scale = {:?}", composed);

    println!("\n5. Converting to Matrix4:");
    let matrix: flui_types::geometry::Matrix4 = italic.into();
    println!("   Skew matrix:");
    println!("   [ {:.3}  {:.3}  {:.3}  {:.3} ]", matrix.m[0], matrix.m[4], matrix.m[8], matrix.m[12]);
    println!("   [ {:.3}  {:.3}  {:.3}  {:.3} ]", matrix.m[1], matrix.m[5], matrix.m[9], matrix.m[13]);
    println!("   [ {:.3}  {:.3}  {:.3}  {:.3} ]", matrix.m[2], matrix.m[6], matrix.m[10], matrix.m[14]);
    println!("   [ {:.3}  {:.3}  {:.3}  {:.3} ]", matrix.m[3], matrix.m[7], matrix.m[11], matrix.m[15]);

    println!("\n=== Test Complete ===");
    println!("\nNote: Skew is fully implemented in:");
    println!("  - Transform API (flui_types::geometry::Transform)");
    println!("  - WgpuPainter::skew() (uses Transform internally)");
    println!("  - Canvas::transform() (accepts Transform via Into<Matrix4>)");
}
