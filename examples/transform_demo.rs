//! Transform API Demo
//!
//! This example demonstrates the Transform API for 2D transformations.
//! It shows various transform types, composition, and real-world use cases.

use flui_types::geometry::{Matrix4, Offset, Transform};
use std::f32::consts::PI;

fn main() {
    println!("=== FLUI Transform API Demo ===\n");

    // Basic Transforms
    demo_basic_transforms();

    // Skew Transforms
    demo_skew_transforms();

    // Pivot Point Transforms
    demo_pivot_transforms();

    // Transform Composition
    demo_composition();

    // Transform Queries
    demo_queries();

    // Transform Inversion
    demo_inversion();

    // Real-World Use Cases
    demo_use_cases();

    println!("\n=== Demo Complete ===");
}

fn demo_basic_transforms() {
    println!("--- Basic Transforms ---");

    // Identity (no transformation)
    let identity = Transform::identity();
    println!("Identity: {:?}", identity);
    println!("  is_identity: {}", identity.is_identity());

    // Translation
    let translate = Transform::translate(50.0, 100.0);
    println!("\nTranslate(50, 100): {:?}", translate);
    let matrix: Matrix4 = translate.into();
    println!("  Matrix translation: ({}, {})", matrix.m[12], matrix.m[13]);

    // Rotation
    let rotate = Transform::rotate(PI / 4.0); // 45 degrees
    println!("\nRotate(π/4): {:?}", rotate);
    println!("  Angle in degrees: {:.1}°", (PI / 4.0).to_degrees());

    // Uniform Scale
    let scale = Transform::scale(2.0);
    println!("\nScale(2.0): {:?}", scale);

    // Non-uniform Scale
    let scale_xy = Transform::scale_xy(2.0, 3.0);
    println!("\nScaleXY(2.0, 3.0): {:?}", scale_xy);

    println!();
}

fn demo_skew_transforms() {
    println!("--- Skew Transforms (Shear) ---");

    // Italic text effect - horizontal shear
    let italic = Transform::skew(0.2, 0.0);
    println!("Italic text (skew_x=0.2): {:?}", italic);
    println!("  Shear angle: {:.1}°", 0.2f32.to_degrees());

    let matrix: Matrix4 = italic.into();
    println!("  Matrix shear component: {:.3}", matrix.m[1]);

    // Perspective effect - both axes
    let perspective = Transform::skew(0.3, 0.3);
    println!("\nPerspective (skew_x=0.3, skew_y=0.3): {:?}", perspective);

    // Strong italic for emphasis
    let strong_italic = Transform::skew(0.4, 0.0);
    println!("\nStrong italic (skew_x=0.4): {:?}", strong_italic);
    println!("  Shear angle: {:.1}°", 0.4f32.to_degrees());

    println!();
}

fn demo_pivot_transforms() {
    println!("--- Pivot Point Transforms ---");

    // Rotate around center point
    let center_x = 100.0;
    let center_y = 100.0;
    let rotate_around = Transform::rotate_around(PI / 2.0, center_x, center_y);
    println!(
        "Rotate 90° around ({}, {}): {:?}",
        center_x, center_y, rotate_around
    );

    // Verify pivot point stays fixed
    let matrix: Matrix4 = rotate_around.into();
    let x = center_x;
    let y = center_y;
    let tx = matrix.m[0] * x + matrix.m[4] * y + matrix.m[12];
    let ty = matrix.m[1] * x + matrix.m[5] * y + matrix.m[13];
    println!(
        "  Pivot ({}, {}) stays at ({:.1}, {:.1})",
        center_x, center_y, tx, ty
    );

    // Scale around center
    let scale_around = Transform::scale_around(2.0, 2.0, center_x, center_y);
    println!(
        "\nScale 2x around ({}, {}): {:?}",
        center_x, center_y, scale_around
    );

    println!();
}

fn demo_composition() {
    println!("--- Transform Composition ---");

    // Fluent API - chain transforms
    let composed = Transform::translate(50.0, 50.0)
        .then(Transform::rotate(PI / 4.0))
        .then(Transform::scale(2.0));

    println!("Compose: translate → rotate → scale");
    println!("  Result: {:?}", composed);
    println!("  has_translation: {}", composed.has_translation());
    println!("  has_rotation: {}", composed.has_rotation());
    println!("  has_scale: {}", composed.has_scale());

    // Identity optimization
    let with_identity = Transform::translate(10.0, 20.0).then(Transform::Identity);
    println!("\nWith identity optimization:");
    println!("  translate.then(Identity) = {:?}", with_identity);

    // Complex composition
    let complex = Transform::translate(100.0, 100.0)
        .then(Transform::rotate(PI / 6.0))
        .then(Transform::scale(1.5))
        .then(Transform::skew(0.1, 0.0))
        .then(Transform::translate(-100.0, -100.0));
    println!("\nComplex composition (5 transforms):");
    println!("  {:?}", complex);

    println!();
}

fn demo_queries() {
    println!("--- Transform Queries ---");

    let transform = Transform::translate(10.0, 20.0)
        .then(Transform::rotate(PI / 4.0))
        .then(Transform::scale(2.0));

    println!("Transform: translate → rotate → scale");
    println!("  is_identity: {}", transform.is_identity());
    println!("  has_translation: {}", transform.has_translation());
    println!("  has_rotation: {}", transform.has_rotation());
    println!("  has_scale: {}", transform.has_scale());
    println!("  has_skew: {}", transform.has_skew());

    let skewed = Transform::skew(0.2, 0.0);
    println!("\nSkew transform:");
    println!("  has_skew: {}", skewed.has_skew());
    println!("  has_rotation: {}", skewed.has_rotation());

    println!();
}

fn demo_inversion() {
    println!("--- Transform Inversion ---");

    // Invert translation
    let translate = Transform::translate(10.0, 20.0);
    let inv_translate = translate.inverse().unwrap();
    println!("Translate(10, 20) inverse: {:?}", inv_translate);

    // Invert rotation
    let rotate = Transform::rotate(PI / 4.0);
    let inv_rotate = rotate.inverse().unwrap();
    println!("Rotate(π/4) inverse: {:?}", inv_rotate);

    // Invert scale
    let scale = Transform::scale(2.0);
    let inv_scale = scale.inverse().unwrap();
    println!("Scale(2.0) inverse: {:?}", inv_scale);

    // Non-invertible scale
    let zero_scale = Transform::scale(0.0);
    println!("\nScale(0.0) inverse: {:?}", zero_scale.inverse());
    println!("  (None because scale-by-zero is non-invertible)");

    // Compose and invert
    let composed = Transform::translate(50.0, 50.0)
        .then(Transform::rotate(PI / 4.0))
        .then(Transform::scale(2.0));
    let inv_composed = composed.inverse().unwrap();
    println!("\nComposed transform inverse: {:?}", inv_composed);

    println!();
}

fn demo_use_cases() {
    println!("--- Real-World Use Cases ---");

    // Use Case 1: UI Container Transform
    println!("1. UI Container (translate + scale for zoom):");
    let container_transform = Transform::translate(100.0, 100.0).then(Transform::scale(1.5));
    println!("   {:?}", container_transform);

    // Use Case 2: Button Rotation Animation
    println!("\n2. Button Rotation Animation:");
    let button_center_x = 150.0;
    let button_center_y = 75.0;
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let angle = t * PI * 2.0; // 0° to 360°
        let rotation = Transform::rotate_around(angle, button_center_x, button_center_y);
        println!(
            "   t={:.2} ({}°): rotation around ({}, {})",
            t,
            angle.to_degrees() as i32,
            button_center_x,
            button_center_y
        );
    }

    // Use Case 3: Card Flip Animation
    println!("\n3. Card Flip (rotate + perspective):");
    let card_transform = Transform::rotate(PI)
        .then(Transform::skew(0.2, 0.0))
        .then(Transform::translate(0.0, 10.0));
    println!("   {:?}", card_transform);

    // Use Case 4: Italic Text
    println!("\n4. Italic Text Rendering:");
    let italic = Transform::skew(0.2, 0.0);
    println!("   {:?}", italic);
    println!("   (Apply to text canvas before drawing)");

    // Use Case 5: Parallax Scrolling
    println!("\n5. Parallax Scrolling Layers:");
    let background = Transform::translate(0.0, 50.0); // Slow
    let midground = Transform::translate(0.0, 100.0); // Medium
    let foreground = Transform::translate(0.0, 150.0); // Fast
    println!("   Background: {:?}", background);
    println!("   Midground: {:?}", midground);
    println!("   Foreground: {:?}", foreground);

    // Use Case 6: Converting from Offset
    println!("\n6. Convert Offset to Transform:");
    let offset = Offset::new(25.0, 50.0);
    let transform: Transform = offset.into();
    println!("   Offset({}, {}) → {:?}", offset.dx, offset.dy, transform);

    // Use Case 7: Converting to Matrix4
    println!("\n7. Convert Transform to Matrix4:");
    let transform = Transform::rotate(PI / 4.0).then(Transform::scale(2.0));
    let matrix: Matrix4 = transform.into();
    println!("   Transform → Matrix4 (4x4 floats)");
    println!(
        "   First row: [{:.3}, {:.3}, {:.3}, {:.3}]",
        matrix.m[0], matrix.m[1], matrix.m[2], matrix.m[3]
    );

    println!();
}
