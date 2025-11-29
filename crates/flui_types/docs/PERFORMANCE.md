# Performance

Performance optimization guide for `flui_types`.

## Contents

- [Memory Layout](#memory-layout)
- [Copy vs Clone](#copy-vs-clone)
- [Allocation Strategies](#allocation-strategies)
- [SIMD Optimization](#simd-optimization)
- [Benchmarking](#benchmarking)
- [Common Bottlenecks](#common-bottlenecks)

---

## Memory Layout

### Type Sizes

All geometry types are optimized for minimal memory footprint:

| Type | Size (bytes) | Layout |
|------|--------------|--------|
| `Point` | 8 | 2 × f32 |
| `Size` | 8 | 2 × f32 |
| `Offset` | 8 | 2 × f32 |
| `Rect` | 16 | 4 × f32 |
| `Color` | 4 | RGBA packed u8 |
| `EdgeInsets` | 16 | 4 × f32 |
| `BoxConstraints` | 16 | 4 × f32 |
| `Alignment` | 8 | 2 × f32 |

### Cache-Friendly Design

```rust
// Good: Contiguous memory layout
struct LayoutData {
    rects: Vec<Rect>,      // 16 bytes each, contiguous
    colors: Vec<Color>,    // 4 bytes each, contiguous
}

// Better for iteration: Structure of Arrays
struct LayoutDataSoA {
    lefts: Vec<f32>,
    tops: Vec<f32>,
    widths: Vec<f32>,
    heights: Vec<f32>,
}
```

### Option Optimization

Some types benefit from niche optimization:

```rust
// Option<NonZeroUsize> = 8 bytes (same as usize)
// Used internally for ElementId

size_of::<Option<Point>>()  // 12 bytes (Point + discriminant + padding)
size_of::<Point>()          // 8 bytes
```

---

## Copy vs Clone

### Why Copy Matters

All geometry types implement `Copy`, enabling zero-cost passing:

```rust
// Copy: No heap allocation, just register/stack copy
fn process_point(p: Point) -> Point {
    Point::new(p.x * 2.0, p.y * 2.0)
}

// This is as fast as passing two f32s
let result = process_point(point);
```

### When Clone is Used

Some types require `Clone` due to heap allocations:

```rust
// Clone required for Vec contents
#[derive(Clone)]
pub struct BoxDecoration {
    pub box_shadow: Vec<BoxShadow>,  // Heap allocated
    // ...
}

// Minimize clones in hot paths
let decoration = &self.decoration;  // Borrow instead of clone
```

---

## Allocation Strategies

### Pre-allocation

```rust
// ❌ Bad: Growing vector during iteration
let mut points = Vec::new();
for i in 0..1000 {
    points.push(Point::new(i as f32, 0.0));
}

// ✅ Good: Pre-allocate capacity
let mut points = Vec::with_capacity(1000);
for i in 0..1000 {
    points.push(Point::new(i as f32, 0.0));
}
```

### Const Construction

Many types support `const` construction:

```rust
// Computed at compile time
const PADDING: EdgeInsets = EdgeInsets::symmetric_const(16.0, 8.0);
const CENTER: Alignment = Alignment::CENTER;
const WHITE: Color = Colors::WHITE;

// Zero runtime cost
fn get_padding() -> EdgeInsets {
    PADDING
}
```

### Stack vs Heap

```rust
// Stack allocation (fast)
let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 50.0);

// Heap allocation (avoid in hot paths)
let rect = Box::new(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0));
```

---

## SIMD Optimization

### Current Status

SIMD optimizations are planned but not yet implemented. The `simd` feature flag is reserved for future use.

### Planned Optimizations

```rust
#[cfg(feature = "simd")]
impl Rect {
    /// SIMD-accelerated intersection test
    pub fn intersect_simd(&self, other: &Rect) -> Option<Rect> {
        // Use SIMD for parallel min/max operations
        use std::simd::f32x4;
        
        let self_vec = f32x4::from_array([
            self.left, self.top, self.right, self.bottom
        ]);
        // ...
    }
}
```

### Manual SIMD with glam

`flui_types` uses `glam` which provides SIMD when available:

```rust
use glam::Vec2;

// glam automatically uses SIMD on supported platforms
let a = Vec2::new(1.0, 2.0);
let b = Vec2::new(3.0, 4.0);
let c = a + b;  // SIMD accelerated
```

---

## Benchmarking

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench -p flui_types

# Run specific benchmark
cargo bench -p flui_types -- rect_intersection
```

### Writing Benchmarks

```rust
use criterion::{criterion_group, criterion_main, Criterion};
use flui_types::geometry::Rect;

fn rect_intersection_benchmark(c: &mut Criterion) {
    let rect1 = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
    let rect2 = Rect::from_ltwh(50.0, 50.0, 100.0, 100.0);
    
    c.bench_function("rect_intersect", |b| {
        b.iter(|| rect1.intersect(&rect2))
    });
}

criterion_group!(benches, rect_intersection_benchmark);
criterion_main!(benches);
```

### Expected Performance

| Operation | Time (approx) |
|-----------|---------------|
| `Point::new()` | < 1 ns |
| `Rect::contains()` | < 2 ns |
| `Rect::intersect()` | < 5 ns |
| `Color::lerp()` | < 3 ns |
| `BoxConstraints::constrain()` | < 3 ns |

---

## Common Bottlenecks

### 1. Repeated Calculations

```rust
// ❌ Bad: Computing center multiple times
for child in children {
    let parent_center = parent.center();  // Computed every iteration
    // ...
}

// ✅ Good: Compute once
let parent_center = parent.center();
for child in children {
    // Use parent_center
}
```

### 2. Unnecessary Clones

```rust
// ❌ Bad: Cloning decoration for each widget
fn paint(&self, decoration: BoxDecoration) {
    // decoration is moved and possibly cloned by caller
}

// ✅ Good: Borrow instead
fn paint(&self, decoration: &BoxDecoration) {
    // No clone needed
}
```

### 3. Float Precision Issues

```rust
// ❌ Bad: Accumulating floating point errors
let mut pos = 0.0;
for _ in 0..1000 {
    pos += 0.1;  // Accumulates error
}

// ✅ Good: Compute directly
for i in 0..1000 {
    let pos = i as f32 * 0.1;  // No error accumulation
}
```

### 4. Bounds Checking in Loops

```rust
// ❌ Bad: Bounds check on each access
for i in 0..points.len() {
    let p = points[i];  // Bounds check
}

// ✅ Good: Iterator (no bounds checks)
for p in &points {
    // Direct access
}

// ✅ Also good: get_unchecked (unsafe but fast)
unsafe {
    for i in 0..points.len() {
        let p = points.get_unchecked(i);
    }
}
```

---

## Profile-Guided Optimization

### Profiling Tools

- **perf** (Linux): `perf record cargo run --release`
- **Instruments** (macOS): Xcode Instruments
- **Tracy** (cross-platform): Real-time profiling

### Inlining Hints

Critical methods are marked for inlining:

```rust
impl Point {
    #[inline]
    pub fn distance_to(&self, other: Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
    
    #[inline(always)]  // Force inline even in debug
    pub fn x(&self) -> f32 {
        self.x
    }
}
```

### Release Build Settings

Recommended `Cargo.toml` settings for maximum performance:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"

[profile.release.package.flui_types]
opt-level = 3
```

---

## Memory Pools

For high-frequency allocations, consider memory pools:

```rust
use std::cell::RefCell;

thread_local! {
    static RECT_POOL: RefCell<Vec<Rect>> = RefCell::new(Vec::with_capacity(1000));
}

// Reuse allocations
fn get_pooled_rect() -> Rect {
    RECT_POOL.with(|pool| {
        pool.borrow_mut().pop().unwrap_or_default()
    })
}

fn return_to_pool(rect: Rect) {
    RECT_POOL.with(|pool| {
        pool.borrow_mut().push(rect);
    });
}
```

---

## See Also

- [ARCHITECTURE.md](ARCHITECTURE.md) — Internal Architecture
- [PATTERNS.md](PATTERNS.md) — Usage Patterns
- [GUIDE.md](GUIDE.md) — User Guide
- [CHEATSHEET.md](CHEATSHEET.md) — Quick Reference
