# flui_painting Documentation

Welcome to the `flui_painting` documentation! This directory contains comprehensive guides and references for the crate.

## Quick Links

- **[README](../README.md)** - Quick start and API overview
- **[API Documentation](https://docs.rs/flui_painting)** - Full API reference
- **[Examples](../examples/)** - Code examples

## Guides

### For Users

- **[Architecture Guide](./ARCHITECTURE.md)** - Internal architecture and design patterns
- **[Performance Guide](./PERFORMANCE.md)** - Optimization techniques and benchmarking
- **[Migration Guide](./MIGRATION.md)** - Upgrading between versions

### For Contributors

- **[Contributing Guide](../CONTRIBUTING.md)** - How to contribute to the project

## Overview

`flui_painting` is a high-performance Canvas API for recording 2D drawing commands into optimized display lists for GPU rendering.

### Key Concepts

- **Canvas** - Mutable recording context for drawing operations
- **DisplayList** - Immutable sequence of commands ready for GPU execution
- **DrawCommand** - Individual drawing operation with all parameters
- **Command Pattern** - Separation of recording from execution

### Architecture

```text
┌──────────────┐
│ RenderObject │  calls paint()
└──────┬───────┘
       │
       ▼
┌──────────────┐
│   Canvas     │  records commands
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ DisplayList  │  sent to GPU thread
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ WgpuPainter  │  executes on GPU
└──────────────┘
```

## Getting Started

### Installation

```toml
[dependencies]
flui_painting = "0.1"
flui_types = "0.1"
```

### Basic Usage

```rust
use flui_painting::{Canvas, Paint};
use flui_types::{geometry::Rect, styling::Color};

let mut canvas = Canvas::new();
let rect = Rect::from_ltrb(10.0, 10.0, 100.0, 50.0);
let paint = Paint::fill(Color::BLUE);
canvas.draw_rect(rect, &paint);

let display_list = canvas.finish();
```

## Documentation Structure

### Architecture Guide

Learn about:
- Component architecture
- Data flow
- Design patterns
- Integration points
- Future enhancements

**Read if:** You want to understand how the crate works internally.

### Performance Guide

Learn about:
- Benchmarking
- Memory management
- Optimization techniques
- Common pitfalls
- Profiling

**Read if:** You need to optimize rendering performance.

### Migration Guide

Learn about:
- Version differences
- Breaking changes
- Migration steps
- API changes

**Read if:** You're upgrading from an older version.

### Contributing Guide

Learn about:
- Development setup
- Code style
- Testing requirements
- Pull request process
- Performance requirements

**Read if:** You want to contribute to the project.

## Additional Resources

### Examples

The [`examples/`](../examples/) directory contains practical usage examples:

- `basic_canvas.rs` - Simple canvas usage
- `transforms.rs` - Transform operations
- `composition.rs` - Canvas composition
- `caching.rs` - Performance optimization with caching

### Tests

The [`tests/`](../tests/) directory contains integration tests that serve as additional examples:

- `canvas_composition.rs` - Composition patterns
- `canvas_scoped.rs` - Scoped operations
- `canvas_transform.rs` - Transform API
- `thread_safety.rs` - Thread safety guarantees

### API Documentation

Full API documentation is available at:
- **[docs.rs/flui_painting](https://docs.rs/flui_painting)** - Online documentation
- `cargo doc --open` - Build and view locally

## FAQ

### General

**Q: What is flui_painting?**

A: A Canvas-based painting abstraction that records drawing commands into DisplayLists for GPU rendering.

**Q: How does it differ from immediate mode rendering?**

A: It uses deferred rendering - commands are recorded first, then executed later on the GPU. This enables caching, optimization, and thread safety.

**Q: Is it thread-safe?**

A: Canvas is `Send` (can move between threads), DisplayList is `Send + Clone` (can share across threads).

### Performance

**Q: How do I optimize rendering performance?**

A: See the [Performance Guide](./PERFORMANCE.md) for detailed optimization techniques.

**Q: Should I reuse Canvas instances?**

A: Yes! Use `canvas.reset()` to clear and reuse allocations across frames.

**Q: What's the fastest way to compose canvases?**

A: Append children before the parent draws anything for O(1) zero-copy composition.

### API

**Q: Why do I get "method not found" errors?**

A: Import the prelude: `use flui_painting::prelude::*;` to bring extension traits into scope.

**Q: Can I modify a DisplayList after creation?**

A: No, DisplayList is immutable. You can transform it using `map()` or `filter()` to create a new one.

**Q: Does restore() panic if called without save()?**

A: No (since 0.1.0). It's a safe no-op if there's no saved state.

### Integration

**Q: How does this integrate with flui_rendering?**

A: RenderObjects receive a `PaintingContext` with a Canvas. They use it to record drawing commands during `paint()`.

**Q: How does this integrate with flui_engine?**

A: The engine receives DisplayLists and executes them on the GPU using wgpu.

**Q: Can I use this standalone?**

A: Yes! You can use flui_painting independently to generate DisplayLists, though you'll need your own GPU backend to execute them.

## Version History

- **0.1.0** (current) - Initial release with core functionality
  - Extension traits pattern
  - Safe restore() behavior
  - Iterator methods
  - Comprehensive documentation

## Contributing

We welcome contributions! Please read the [Contributing Guide](./CONTRIBUTING.md) for details on:

- Code style
- Testing requirements
- Pull request process
- Performance expectations

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../../LICENSE-MIT))

at your option.

## Support

- **Issues:** [GitHub Issues](https://github.com/flui-org/flui/issues)
- **Discussions:** [GitHub Discussions](https://github.com/flui-org/flui/discussions)
- **Documentation:** [docs.rs/flui_painting](https://docs.rs/flui_painting)

---

**Happy painting!** 🎨
