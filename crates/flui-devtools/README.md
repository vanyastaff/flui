# FLUI DevTools 🛠️

Developer tools for FLUI framework - profiling, timeline, and inspection tools inspired by Flutter DevTools and React DevTools.

## Features

### 🎯 Performance Profiler (default)
Real-time frame performance analysis with phase-level breakdown:
- Frame timing with nanosecond precision
- Build/Layout/Paint phase profiling
- Jank detection and FPS tracking
- Frame history and statistics
- RAII guards for automatic timing

### 🔍 Widget Inspector (default)
Interactive widget tree inspection and debugging:
- Widget tree visualization
- Property inspection
- Size and position analysis
- Widget highlighting
- Type-based search
- Root-to-widget path tracing

### ⏱️ Timeline View
Event timeline with Chrome DevTools integration:
- Timeline event recording
- Category-based filtering
- Chrome Trace format export
- Thread-aware tracking
- Nested event support

### 🔥 Hot Reload
File watching with automatic rebuilds:
- Cross-platform file watching
- Configurable debounce
- Async and blocking modes
- Multiple path monitoring
- RAII watch handles

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_devtools = "0.1"

# Or with all features:
flui_devtools = { version = "0.1", features = ["full"] }
```

## Quick Start

### Performance Profiling

```rust
use flui_devtools::prelude::*;

let profiler = Profiler::new();

// Profile a frame
profiler.begin_frame();

{
    let _guard = profiler.profile_phase(FramePhase::Build);
    // Build widgets...
}

{
    let _guard = profiler.profile_phase(FramePhase::Layout);
    // Layout RenderObjects...
}

{
    let _guard = profiler.profile_phase(FramePhase::Paint);
    // Paint layers...
}

profiler.end_frame();

// Print frame summary
profiler.print_frame_summary();

// Get detailed stats
if let Some(stats) = profiler.frame_stats() {
    println!("Frame #{}: {:.2}ms", 
        stats.frame_number, 
        stats.total_time_ms()
    );
    println!("  Build: {:.1}%", stats.build_percent());
    println!("  Layout: {:.1}%", stats.layout_percent());
    println!("  Paint: {:.1}%", stats.paint_percent());
}

// Get performance metrics
println!("Avg FPS (last 60 frames): {:.1}", profiler.average_fps(60));
println!("Jank: {:.1}%", profiler.jank_percentage(100));
```

### Widget Inspector

```rust
use flui_devtools::inspector::Inspector;

let inspector = Inspector::new();
inspector.attach_to_tree(element_tree);

// Select and inspect a widget
let widget_info = inspector.select_widget(element_id);
println!("Widget type: {:?}", widget_info.widget_type);
println!("Size: {:?}", widget_info.size);
println!("Position: {:?}", widget_info.position);
println!("Children: {}", widget_info.children.len());

// Highlight widget for debugging
inspector.highlight_widget(element_id);

// Get full widget tree
let tree = inspector.get_widget_tree();
for node in tree.iter() {
    println!("{:indent$}{}", "", node.widget_type, 
        indent = node.depth * 2);
}

// Find all widgets of a type
let buttons = inspector.find_widgets_by_type("Button");
```

### Timeline Events

```rust
use flui_devtools::timeline::{Timeline, EventCategory};

let timeline = Timeline::new();

// Record frame event
{
    let _guard = timeline.record_event("Frame #42", EventCategory::Frame);
    // Frame work...
}

// Record custom events
{
    let _guard = timeline.record_event("LoadAssets", EventCategory::Custom);
    // Asset loading...
}

// Export to Chrome DevTools
let json = timeline.export_chrome_trace();
std::fs::write("trace.json", json).unwrap();

// Then open chrome://tracing and load trace.json
```

## Feature Flags

- `default`: Enables `profiling` and `inspector`
- `profiling`: Performance profiling tools
- `inspector`: Widget tree inspection
- `timeline`: Timeline event tracking
- `network-monitor`: HTTP request monitoring (TODO)
- `memory-profiler`: Memory usage tracking (TODO)
- `remote-debug`: WebSocket debugging server (TODO)
- `tracing-support`: Integration with `tracing` crate (TODO)
- `full`: All features enabled

## Examples

See `examples/` directory for complete examples:
- `profiler_demo.rs` - Frame profiling
- `inspector_demo.rs` - Widget inspection
- `timeline_demo.rs` - Timeline recording

## Architecture

```
┌─────────────────────────────────────────┐
│          FLUI Application               │
│   ┌─────────────────────────────┐      │
│   │     Widget Tree             │      │
│   │     Element Tree            │      │
│   │     Render Tree             │      │
│   └─────────────────────────────┘      │
└──────────────┬──────────────────────────┘
               │
    ┌──────────▼──────────┐
    │   FLUI DevTools     │
    ├─────────────────────┤
    │  • Profiler         │─── Frame timing
    │  • Inspector        │─── Widget tree
    │  • Timeline         │─── Events
    └─────────────────────┘
```

## Performance

DevTools is designed for minimal runtime overhead:
- **Profiler**: ~50ns per phase (RAII guard)
- **Inspector**: O(1) widget lookup with caching
- **Timeline**: Lock-free event recording
- **Hot Reload**: Debounced file events

## License

MIT OR Apache-2.0
