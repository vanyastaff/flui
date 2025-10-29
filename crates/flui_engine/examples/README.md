# FLUI Engine Examples

This directory contains systematic tests and demos for the FLUI rendering engine.

## Systematic Tests

These tests provide comprehensive coverage of engine features with clear visual verification:

### Core Rendering
- **gradient_test.rs** - Gradient rendering (horizontal, vertical, radial)
- **shapes_test.rs** - Basic shapes (rect, circle, line, arc, polygon, oval)
- **transform_test.rs** - Transformations (translate, scale, rotate, skew, nested)
- **clipping_test.rs** - Clipping (rect, rrect, oval, nested, with transform)
- **alignment_test_demo.rs** - Layout alignment and positioning

### Advanced Features
- **layer_demo.rs** - Layer system demonstration
- **path_rendering_demo.rs** - Path rendering and manipulation
- **material_elevation_demo.rs** - Material Design elevation
- **material_elevation_demo_visual.rs** - Visual elevation demo
- **text_effects_demo.rs** - Text rendering effects

### Developer Tools
- **memory_leak_test.rs** - Memory leak detection
- **memory_profiling.rs** - Memory profiling tools
- **performance_overlay.rs** - Performance monitoring overlay
- **profiled_compositor.rs** - Compositor profiling
- **profiled_rendering.rs** - Rendering profiling
- **unified_devtools.rs** - Unified development tools

### Interactive Examples
- **input_tracker.rs** - Input event tracking
- **interactive_button.rs** - Interactive UI elements
- **full_pipeline.rs** - Full rendering pipeline

## Running Examples

To run an example:

```bash
cargo run -p flui_engine --example gradient_test
cargo run -p flui_engine --example transform_test
cargo run -p flui_engine --example shapes_test
# etc.
```

## Test Pattern

All systematic tests follow this pattern:

1. **Clear Title** - Shows what is being tested
2. **Systematic Coverage** - Tests all variants of a feature
3. **Visual Labels** - Each test case is labeled
4. **Minimal Dependencies** - Tests focus on one feature
5. **Console Output** - Lists what is being tested

Example output:
```
=== Gradient Test ===
Testing Painter gradient primitives:
  1. horizontal_gradient() - red to blue
  2. vertical_gradient() - green to yellow
  3. radial_gradient_simple() - blue center to red edge
  4. radial_gradient_simple() - cyan to magenta (with inner radius)
```

## Adding New Tests

When adding a new systematic test:

1. Follow the naming pattern: `{feature}_test.rs`
2. Use the established structure (see `gradient_test.rs` as template)
3. Provide clear console output listing test cases
4. Label each test case in the visual output
5. Update this README

## Architecture

Tests use the `App` and `AppLogic` framework:

```rust
struct MyTestApp;

impl AppLogic for MyTestApp {
    fn on_event(&mut self, event: &Event) -> bool { /* handle events */ }
    fn update(&mut self, _delta_time: f32) { /* update state */ }
    fn render(&mut self, painter: &mut dyn Painter) { /* render */ }
}

fn main() {
    let app = App::with_config(AppConfig::new().backend(Backend::Egui))
        .title("My Test")
        .size(800, 600);
    app.run(MyTestApp).expect("Failed to run app");
}
```

This provides:
- Easy visual verification
- Interactive testing
- Consistent API across all tests
