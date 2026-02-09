//! Basic usage example for flui_types
//!
//! This example demonstrates the fundamental types used in FLUI:
//! - Pixels: The primary unit for layout and rendering
//! - Point: 2D coordinates
//! - Rect: Rectangular regions
//! - Size: Dimensions
//! - Color: RGBA colors with blending

use flui_types::geometry::{px, Edges, Point, Rect, Size};
use flui_types::styling::Color;

fn main() {
    println!("=== FLUI Types Basic Usage ===\n");

    // 1. Working with Pixels
    println!("1. Pixels:");
    let width = px(100.0);
    let height = px(50.0);
    println!("   Width: {:?}", width);
    println!("   Height: {:?}", height);
    println!("   Sum: {:?}", width + height);
    println!("   Scaled: {:?}\n", width * 2.0);

    // 2. Creating Points
    println!("2. Points:");
    let origin = Point::ZERO;
    let position = Point::new(px(100.0), px(200.0));
    println!("   Origin: {:?}", origin);
    println!("   Position: {:?}", position);
    println!("   Distance: {:?}\n", origin.distance(position));

    // 3. Working with Rectangles
    println!("3. Rectangles:");
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    println!("   Rect1: {:?}", rect1);
    println!("   Rect2: {:?}", rect2);

    if let Some(intersection) = rect1.intersect(&rect2) {
        println!("   Intersection: {:?}", intersection);
    }

    let union = rect1.union(&rect2);
    println!("   Union: {:?}", union);

    let center_point = Point::new(px(75.0), px(75.0));
    println!("   Contains center: {}\n", rect1.contains(center_point));

    // 4. Working with Sizes
    println!("4. Sizes:");
    let size = Size::new(px(800.0), px(600.0));
    println!("   Size: {:?}", size);
    println!("   Area: {:?}", size.area());
    println!("   Aspect ratio: {:.2}\n", size.aspect_ratio());

    // 5. Creating and Blending Colors
    println!("5. Colors:");
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    let semi_transparent_red = Color::rgba(255, 0, 0, 128);

    println!("   Red: {:?}", red);
    println!("   Blue: {:?}", blue);
    println!("   Semi-transparent red: {:?}", semi_transparent_red);

    let purple = Color::lerp(red, blue, 0.5);
    println!("   Purple (50% mix): {:?}", purple);

    let blended = semi_transparent_red.blend_over(blue);
    println!("   Blended over blue: {:?}", blended);

    let lighter = red.lighten(0.3);
    println!("   Lighter red: {:?}", lighter);

    let darker = blue.darken(0.3);
    println!("   Darker blue: {:?}\n", darker);

    // 6. Practical Example: Button Layout
    println!("6. Practical Example - Button Layout:");

    let button_size = Size::new(px(120.0), px(40.0));
    let button_position = Point::new(px(20.0), px(20.0));
    let button_rect = Rect::from_origin_size(button_position, button_size);

    println!("   Button bounds: {:?}", button_rect);

    let padding = Edges::all(px(10.0));
    let content_rect = padding.deflate_rect(button_rect);
    println!("   Content area (with padding): {:?}", content_rect);

    let button_color = Color::from_hex("#4CAF50").unwrap();
    let hover_color = button_color.lighten(0.1);

    println!("   Button color: {}", button_color.to_hex());
    println!("   Hover color: {}", hover_color.to_hex());

    // Hit testing
    let click_point = Point::new(px(60.0), px(40.0));
    let is_clicked = button_rect.contains(click_point);
    println!(
        "   Click at {:?} hits button: {}\n",
        click_point, is_clicked
    );

    // 7. Type Safety Example
    println!("7. Type Safety:");
    println!("   The type system prevents mixing incompatible units.");
    println!("   For example, you cannot directly add Pixels and DevicePixels.");
    println!(
        "   You must explicitly convert between units using to_pixels() or to_device_pixels()."
    );
    println!("   This catches bugs at compile time!\n");

    println!("=== Example Complete ===");
}
