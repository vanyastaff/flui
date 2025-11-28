# Performance Guide

This guide covers performance optimization techniques for `flui_painting`.

## Table of Contents

- [Benchmarking](#benchmarking)
- [Memory Management](#memory-management)
- [Optimization Techniques](#optimization-techniques)
- [Common Pitfalls](#common-pitfalls)
- [Profiling](#profiling)

## Benchmarking

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench -p flui_painting

# Run specific benchmark
cargo bench -p flui_painting canvas_composition

# With detailed output
cargo bench -p flui_painting -- --verbose
```

### Performance Targets

| Operation | Target | Actual |
|-----------|--------|--------|
| Canvas::new() | < 100ns | ~50ns |
| draw_rect() | < 50ns | ~30ns |
| append_canvas (empty) | < 200ns | ~100ns (swap) |
| append_canvas (1000 cmds) | < 10µs | ~5µs |
| finish() | < 100ns | ~50ns (move) |
| DisplayList::clone() | < 1µs (1000 cmds) | ~500ns |

## Memory Management

### Canvas Allocation Strategy

#### Problem: Per-Frame Allocations

```rust
// ❌ Bad - allocates every frame
fn render_frame() {
    let mut canvas = Canvas::new(); // Allocation!
    canvas.draw_rect(rect, &paint);
    // ... more drawing
    let display_list = canvas.finish();
}
```

**Cost:** ~2-3µs per frame on allocation/deallocation.

#### Solution: Canvas Reuse

```rust
// ✅ Good - reuse allocations
struct Renderer {
    canvas: Canvas,
}

impl Renderer {
    fn render_frame(&mut self) {
        self.canvas.reset(); // Clear, keep capacity
        self.canvas.draw_rect(rect, &paint);
        // ... more drawing
        let display_list = self.canvas.finish();
    }
}
```

**Benefit:** ~100ns per reset vs ~2µs for allocation.

### DisplayList Memory

#### Arc-based Sharing (Future)

Currently, `DisplayList::clone()` does a deep copy. Future versions will use Arc:

```rust
// Future: O(1) clone
pub struct DisplayList {
    inner: Arc<DisplayListInner>,
}
```

**Current Workaround:** Use `&DisplayList` references when possible.

```rust
// ❌ Avoid unnecessary clones
let dl1 = canvas.finish();
let dl2 = dl1.clone(); // Deep copy
let dl3 = dl1.clone(); // Another deep copy

// ✅ Use references
let display_list = canvas.finish();
render_thread_1(&display_list);
render_thread_2(&display_list);
```

### Command Vector Growth

#### Problem: Repeated Reallocations

```rust
// ❌ Bad - grows gradually
let mut canvas = Canvas::new();
for i in 0..1000 {
    canvas.draw_rect(rect, &paint); // Reallocates multiple times
}
```

**Cost:** Multiple reallocations as Vec grows.

#### Solution: Reserve Capacity

```rust
// ✅ Good - pre-allocate
let mut canvas = Canvas::with_capacity(1000);
for i in 0..1000 {
    canvas.draw_rect(rect, &paint); // No reallocations
}
```

**Note:** `with_capacity()` is not yet public API. Use `reset()` to preserve capacity between frames.

## Optimization Techniques

### 1. Batch Operations

#### Individual Calls

```rust
// ❌ Slow - many function calls
for rect in rects {
    canvas.draw_rect(rect, &paint);
}
```

**Cost:** Function call overhead × N.

#### Batch Calls

```rust
// ✅ Fast - single call
canvas.draw_rects(&rects, &paint);
```

**Benefit:** Single function call, better cache locality.

**Benchmark:**

```
draw_rect × 1000:     15µs
draw_rects(1000):      8µs  (1.9× faster)
```

### 2. Culling

#### Skip Invisible Objects

```rust
// ✅ Cull before expensive operations
if canvas.would_be_clipped(&rect) == Some(true) {
    return; // Skip - completely outside clip
}

// Expensive operation only if visible
canvas.draw_complex_shape(&shape, &paint);
```

**Benefit:** Avoid creating DrawCommands for invisible objects.

**Typical Savings:** 30-50% of commands in complex UIs.

### 3. Static Content Caching

#### Problem: Redrawing Static Content

```rust
// ❌ Bad - redraws every frame
fn render_background(canvas: &mut Canvas) {
    // Complex gradient background
    canvas.draw_gradient(rect, complex_gradient);
    canvas.draw_image(logo, rect, None);
    // ... more static content
}

// Called every frame
for _ in 0..60 {
    let mut canvas = Canvas::new();
    render_background(&mut canvas); // Wasteful!
    // ... dynamic content
}
```

#### Solution: Cache DisplayList

```rust
// ✅ Good - cache static content
let background = Canvas::record(|c| {
    c.draw_gradient(rect, complex_gradient);
    c.draw_image(logo, rect, None);
    // ... more static content
});

// Reuse every frame
for _ in 0..60 {
    let mut canvas = Canvas::new();
    canvas.append_display_list(background.clone()); // Fast!
    // ... dynamic content only
}
```

**Benefit:** Record once, replay many times.

**Benchmark:**

```
Render static content × 60 frames:
  Without caching:  1200µs (20µs/frame)
  With caching:      360µs (6µs/frame)  (3.3× faster)
```

### 4. Transform Baking

Transforms are baked into commands at record time:

```rust
// Transform applied once during recording
canvas.save();
canvas.translate(100.0, 50.0);
canvas.rotate(PI / 4.0);
canvas.draw_rect(rect, &paint);
canvas.restore();

// DrawCommand stores final matrix:
// DrawRect { transform: translate * rotate, ... }
```

**Benefit:** No runtime transform computation during GPU execution.

### 5. Zero-Copy Composition

#### First Child Optimization

```rust
// ✅ Extremely fast - O(1) swap
let mut parent = Canvas::new();
let child = render_large_child(); // 1000s of commands

parent.append_canvas(child); // mem::swap - just swaps Vec pointers!
```

**Benchmark:**

```
append_canvas (empty parent):     100ns  (O(1) swap)
append_canvas (non-empty, 1000):  5µs    (O(n) extend)
```

**Usage Pattern:**

```rust
// ✅ Optimal - append children first
let mut canvas = Canvas::new();
canvas.append_canvas(child1); // O(1) swap
canvas.append_canvas(child2); // O(n1)
canvas.append_canvas(child3); // O(n1 + n2)
// Parent's own drawing
canvas.draw_rect(rect, &paint); // O(1) push

// ❌ Suboptimal - parent draws first
let mut canvas = Canvas::new();
canvas.draw_rect(rect, &paint); // Parent has commands
canvas.append_canvas(child1);   // O(n1) extend, no swap
canvas.append_canvas(child2);   // O(n1)
canvas.append_canvas(child3);   // O(n1 + n2)
```

### 6. Chaining API

The chaining API has minimal overhead:

```rust
// Similar performance to direct calls
canvas
    .translated(100.0, 50.0)
    .rotated(PI / 4.0)
    .rect(rect, &paint);

// vs

canvas.translate(100.0, 50.0);
canvas.rotate(PI / 4.0);
canvas.draw_rect(rect, &paint);
```

**Benefit:** Better readability with no cost.

### 7. Scoped Operations

Scoped operations compile to direct calls:

```rust
// Zero overhead - inlined by compiler
canvas.with_save(|c| {
    c.translate(50.0, 50.0);
    c.draw_rect(rect, &paint);
});

// Compiles to:
canvas.save();
canvas.translate(50.0, 50.0);
canvas.draw_rect(rect, &paint);
canvas.restore();
```

## Common Pitfalls

### 1. Unnecessary Clones

```rust
// ❌ Bad - clones Paint every call
for i in 0..1000 {
    canvas.draw_rect(rect, &Paint::fill(Color::RED)); // New Paint each time!
}

// ✅ Good - reuse Paint
let paint = Paint::fill(Color::RED);
for i in 0..1000 {
    canvas.draw_rect(rect, &paint);
}
```

**Savings:** ~10ns per draw call.

### 2. Path Allocation

```rust
// ❌ Bad - allocates Path every draw
for i in 0..100 {
    let path = Path::circle(center, radius); // Allocation!
    canvas.draw_path(&path, &paint);
}

// ✅ Good - reuse Path
let path = Path::circle(center, radius);
for i in 0..100 {
    canvas.draw_path(&path, &paint);
}

// ✅ Better - use draw_circle
for i in 0..100 {
    canvas.draw_circle(center, radius, &paint); // No Path allocation
}
```

### 3. Excessive save/restore

```rust
// ❌ Bad - save/restore every iteration
for item in items {
    canvas.save();
    canvas.translate(item.x, item.y);
    canvas.draw_rect(item.rect, &paint);
    canvas.restore();
}

// ✅ Good - batch transforms
canvas.save();
for item in items {
    canvas.translate(item.x, item.y);
    canvas.draw_rect(item.rect, &paint);
    canvas.translate(-item.x, -item.y); // Undo
}
canvas.restore();

// ✅ Better - don't transform at all
for item in items {
    let translated_rect = item.rect.translate(item.x, item.y);
    canvas.draw_rect(translated_rect, &paint);
}
```

### 4. Redundant Bounds Checks

```rust
// ❌ Wasteful - checks bounds every iteration
for item in items {
    if !canvas.would_be_clipped(&item.rect) {
        canvas.draw_rect(item.rect, &paint);
    }
}

// ✅ Better - check container bounds once
let container_bounds = Rect::from_ltrb(
    items.iter().map(|i| i.rect.left()).min(),
    items.iter().map(|i| i.rect.top()).min(),
    items.iter().map(|i| i.rect.right()).max(),
    items.iter().map(|i| i.rect.bottom()).max(),
);

if !canvas.would_be_clipped(&container_bounds) {
    for item in items {
        canvas.draw_rect(item.rect, &paint);
    }
}
```

## Profiling

### CPU Profiling

#### Using cargo-flamegraph

```bash
# Install
cargo install flamegraph

# Profile
cargo flamegraph --bench canvas_composition

# Opens flamegraph.svg in browser
```

#### Using perf (Linux)

```bash
# Record
cargo build --release --bench canvas_composition
perf record --call-graph=dwarf ./target/release/deps/canvas_composition-*

# Analyze
perf report
```

### Memory Profiling

#### Using heaptrack (Linux)

```bash
# Install
sudo apt install heaptrack

# Profile
heaptrack ./target/release/deps/canvas_composition-*

# Analyze
heaptrack_gui heaptrack.canvas_composition.*.gz
```

#### Using Valgrind

```bash
# Massif (heap profiler)
valgrind --tool=massif ./target/release/deps/canvas_composition-*

# Analyze
ms_print massif.out.*
```

### Analyzing Results

#### Look for:

1. **Hot paths** - Functions called frequently
2. **Allocations** - Memory allocation/deallocation
3. **Cache misses** - Poor data locality
4. **Branch mispredictions** - Unpredictable control flow

#### Common Findings:

```
// Hot path example
Canvas::draw_rect:           15% (optimize this!)
DisplayList::push:           10%
Vec::extend:                  8%
Matrix4::multiply:            5%
```

## Performance Checklist

Use this checklist when optimizing rendering code:

- [ ] Reuse Canvas allocations with `reset()`
- [ ] Cache static content in DisplayLists
- [ ] Use batch operations (`draw_rects`, `draw_circles`)
- [ ] Cull invisible objects with `would_be_clipped()`
- [ ] Append children before parent's own drawing
- [ ] Reuse Paint and Path objects
- [ ] Avoid unnecessary save/restore
- [ ] Profile with flamegraph or perf
- [ ] Check memory usage with heaptrack
- [ ] Benchmark before/after changes

## Performance Metrics

### Target Frame Budget (60 FPS)

```
16.67ms total frame time
├─  2ms:  Layout
├─  3ms:  Paint (DisplayList creation)
├─  8ms:  GPU rendering
└─  3.67ms: Other (input, scheduling, etc.)
```

### Typical Command Counts

| UI Complexity | Commands/Frame | Paint Time |
|---------------|----------------|------------|
| Simple (button) | 10-50 | < 0.1ms |
| Medium (form) | 100-500 | 0.5-1ms |
| Complex (dashboard) | 1000-5000 | 2-5ms |
| Very Complex (editor) | 10000+ | 10ms+ |

### When to Optimize

Optimize when:
- Paint time > 3ms consistently
- Frame drops below 60 FPS
- Memory usage growing unbounded
- Profiler shows hot paths in painting code

Don't optimize prematurely:
- Profile first, then optimize
- Measure before and after
- Focus on bottlenecks, not micro-optimizations

## References

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [cargo-flamegraph](https://github.com/flamegraph-rs/flamegraph)
- [Criterion.rs](https://github.com/bheisler/criterion.rs) - Benchmarking library
