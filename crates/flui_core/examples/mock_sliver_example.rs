//! Example demonstrating how to use MockSliverRender for testing sliver objects

use flui_core::render::RenderSliver; // Import trait to access arity() method
use flui_core::testing::MockSliverRender;
use flui_types::SliverGeometry;

fn main() {
    println!("MockSliverRender Example\n");
    println!("========================\n");

    // Example 1: Create a leaf sliver mock
    println!("1. Leaf Sliver Mock:");
    let geometry = SliverGeometry::simple(500.0, 300.0);
    let mock = MockSliverRender::leaf(geometry);
    println!("   - Geometry: scroll_extent={}, paint_extent={}",
             geometry.scroll_extent, geometry.paint_extent);
    println!("   - Layout calls: {}", mock.layout_call_count());
    println!("   - Paint calls: {}", mock.paint_call_count());
    println!("   - Arity: {:?}", mock.arity());
    println!();

    // Example 2: Create a single-child sliver mock
    println!("2. Single-Child Sliver Mock:");
    let geometry = SliverGeometry::simple(1000.0, 600.0);
    let mock = MockSliverRender::single_child(geometry);
    println!("   - Geometry: scroll_extent={}, paint_extent={}",
             geometry.scroll_extent, geometry.paint_extent);
    println!("   - Arity: {:?}", mock.arity());
    println!();

    // Example 3: Create a multi-child sliver mock
    println!("3. Multi-Child Sliver Mock:");
    let geometry = SliverGeometry::simple(2000.0, 800.0);
    let mock = MockSliverRender::multi_child(geometry, 10);
    println!("   - Geometry: scroll_extent={}, paint_extent={}",
             geometry.scroll_extent, geometry.paint_extent);
    println!("   - Child count: 10");
    println!("   - Arity: {:?}", mock.arity());
    println!();

    // Example 4: Use convenience constructor
    println!("4. Convenience Constructor:");
    let mock = MockSliverRender::with_extents(5000.0, 1200.0);
    println!("   - Scroll extent: 5000.0");
    println!("   - Paint extent: 1200.0");
    println!();

    // Example 5: Test call tracking
    println!("5. Call Tracking:");
    let mut mock = MockSliverRender::leaf(SliverGeometry::simple(100.0, 100.0));
    println!("   - Initial layout calls: {}", mock.layout_call_count());
    println!("   - Initial paint calls: {}", mock.paint_call_count());

    // In a real test, you would call mock.layout() and mock.paint()
    // For demonstration, we'll just show the API
    println!("   - After reset: {}", {
        mock.reset();
        mock.layout_call_count()
    });
    println!();

    println!("Usage in Tests:");
    println!("==============");
    println!(r#"
#[test]
fn test_sliver_layout() {{
    use flui_core::testing::MockSliverRender;
    use flui_types::SliverGeometry;

    let mut mock = MockSliverRender::with_extents(1000.0, 300.0);

    // In a real test, you would:
    // 1. Create a SliverLayoutContext
    // 2. Call mock.layout(&ctx)
    // 3. Verify the geometry and call counts

    assert_eq!(mock.layout_call_count(), 0);
    // mock.layout(&ctx);
    // assert_eq!(mock.layout_call_count(), 1);
}}
"#);
}
