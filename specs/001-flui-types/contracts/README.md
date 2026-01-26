# API Contracts: flui-types Crate

**Date**: 2026-01-26
**Branch**: `001-flui-types`
**Related**: [spec.md](../spec.md), [plan.md](../plan.md), [data-model.md](../data-model.md)

## Purpose

This document defines the API contracts for the flui-types crate. These are binding guarantees that the implementation must uphold. Each contract is testable and measurable.

---

## Contract Categories

1. **Type Safety Contracts** - Compile-time guarantees
2. **Performance Contracts** - Timing and efficiency guarantees
3. **Memory Contracts** - Size and allocation guarantees
4. **Behavioral Contracts** - Functional correctness guarantees
5. **Error Handling Contracts** - Edge case behavior guarantees

---

## 1. Type Safety Contracts

### Contract 1.1: Unit Type Isolation

**Guarantee**: It MUST be impossible to mix incompatible unit types in arithmetic operations.

**Verification Method**: Compile-time tests using `trybuild` crate

**Test Cases**:
```rust
// tests/integration_tests/compile_fail/mixed_units.rs

use flui_types::prelude::*;

fn main() {
    let logical = Pixels(10.0);
    let device = DevicePixels(20.0);

    // This MUST NOT compile
    let _ = logical + device;
    //~^ ERROR: mismatched types
    //~| expected `Pixels`
    //~| found `DevicePixels`
}
```

**Expected Compiler Error**:
```
error[E0308]: mismatched types
  --> tests/integration_tests/compile_fail/mixed_units.rs:X:Y
   |
   | let _ = logical + device;
   |                   ^^^^^^ expected `Pixels`, found `DevicePixels`
```

**Success Criteria**: All `tests/ui/*.rs` files with `//~ ERROR` annotations must fail compilation with expected error messages

---

### Contract 1.2: Explicit Conversions Only

**Guarantee**: There MUST be no implicit conversions between unit types. All conversions MUST be explicit via named methods.

**Verification Method**: API audit + property tests

**Valid Patterns**:
```rust
// ✅ Explicit conversions with clear method names
let logical = Pixels(100.0);
let device = logical.to_device_pixels(2.0);  // Explicit
let rems = logical.to_rems(16.0);            // Explicit
```

**Invalid Patterns**:
```rust
// ❌ These patterns MUST NOT be possible
let logical = Pixels(100.0);
let device: DevicePixels = logical.into();   // NO implicit Into trait
let device = logical as DevicePixels;        // NO type coercion
```

**Success Criteria**:
- No `From<T>` or `Into<T>` implementations between unit types
- All conversion methods follow naming pattern: `to_{target_unit}(scale_params...)`

---

### Contract 1.3: Self-Documenting Conversions

**Guarantee**: Conversion method names MUST clearly indicate direction and requirements.

**Verification Method**: API documentation review

**Naming Patterns**:
```rust
// Direction: to_{target_unit}
Pixels::to_device_pixels(scale_factor)      // Pixels → DevicePixels
DevicePixels::to_logical_pixels(scale_factor) // DevicePixels → Pixels
Pixels::to_rems(base_font_size)             // Pixels → Rems
Rems::to_pixels(base_font_size)             // Rems → Pixels

// Parameters communicate what's needed for conversion
.to_device_pixels(scale_factor: f32)        // "scale_factor" = DPI scale
.to_rems(base_font_size: f32)               // "base_font_size" = font size in pixels
```

**Success Criteria**: All conversion methods pass this checklist:
- [ ] Method name follows `to_{target}` pattern
- [ ] Parameter name documents what it represents
- [ ] Docstring explains when to use this conversion
- [ ] Docstring provides example with realistic values

---

## 2. Performance Contracts

### Contract 2.1: Zero-Cost Abstractions

**Guarantee**: Unit types MUST compile to raw f32 operations with zero runtime overhead.

**Verification Method**: Assembly inspection (`cargo asm`) + criterion benchmarks

**Test Procedure**:
```bash
# Generate assembly for hot path methods
cargo asm flui_types::geometry::point::Point::distance_to --rust

# Verify: Should compile to raw floating-point instructions
# Expected: movss, mulss, addss, sqrtss (x86-64)
# No function calls, no allocations, no indirection
```

**Benchmark Verification**:
```rust
// benches/zero_cost_bench.rs

#[bench]
fn bench_raw_f32_distance(b: &mut Bencher) {
    let x1 = 10.0f32;
    let y1 = 20.0f32;
    let x2 = 30.0f32;
    let y2 = 40.0f32;

    b.iter(|| {
        let dx = x2 - x1;
        let dy = y2 - y1;
        black_box((dx * dx + dy * dy).sqrt())
    });
}

#[bench]
fn bench_point_distance(b: &mut Bencher) {
    let p1 = Point::new(Pixels(10.0), Pixels(20.0));
    let p2 = Point::new(Pixels(30.0), Pixels(40.0));

    b.iter(|| {
        black_box(p1.distance_to(p2))
    });
}
```

**Success Criteria**: Point distance time ≤ Raw f32 time + 5% margin

---

### Contract 2.2: Constant Propagation for Conversions

**Guarantee**: Unit conversions with constant scale factors MUST be optimized away by the compiler.

**Verification Method**: Assembly inspection

**Test Case**:
```rust
// Constant scale factor (known at compile time)
const SCALE: f32 = 2.0;

fn layout_to_render(layout_pos: Point<Pixels>) -> Point<DevicePixels> {
    layout_pos.to_device_pixels(SCALE)
}
```

**Expected Assembly** (release mode):
```asm
; Should reduce to: multiply by 2.0 (constant folded)
mulss   xmm0, xmm1, 2.0   ; No function call overhead
```

**Success Criteria**: No function call overhead in release builds with const scale factors

---

### Contract 2.3: Performance Targets

**Guarantee**: Critical operations MUST meet specified timing targets.

**Verification Method**: Criterion benchmarks with statistical analysis

**Targets**:
| Operation | Maximum Time | Measured With |
|-----------|--------------|---------------|
| Point distance calculation | 10 nanoseconds | criterion |
| Rectangle intersection | 20 nanoseconds | criterion |
| Rectangle union | 20 nanoseconds | criterion |
| Color `mix()` | 20 nanoseconds | criterion |
| Color `blend_over()` | 20 nanoseconds | criterion |
| Rectangle `contains(point)` | 5 nanoseconds | criterion |

**Benchmark Template**:
```rust
// benches/performance_gates.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_point_distance(c: &mut Criterion) {
    let p1 = Point::new(Pixels(10.0), Pixels(20.0));
    let p2 = Point::new(Pixels(30.0), Pixels(40.0));

    c.bench_function("point_distance", |b| {
        b.iter(|| {
            black_box(black_box(p1).distance_to(black_box(p2)))
        })
    });
}

criterion_group!(benches, bench_point_distance);
criterion_main!(benches);
```

**Success Criteria**:
- Mean time < specified target
- Regression detection: ±5% tolerance in CI
- Warm-up iterations to stabilize CPU state

---

### Contract 2.4: Numeric Stability

**Guarantee**: Floating-point operations MUST use epsilon tolerance for equality comparisons.

**Verification Method**: Property tests with extreme values

**Epsilon Value**: 1e-6 (0.000001) for all equality comparisons

**Implementation**:
```rust
pub const EPSILON: f32 = 1e-6;

impl<T: Unit> Point<T> {
    pub fn approx_eq(self, other: Self) -> bool {
        (self.x.to_f32() - other.x.to_f32()).abs() < EPSILON &&
        (self.y.to_f32() - other.y.to_f32()).abs() < EPSILON
    }
}
```

**Property Tests**:
```rust
// tests/property_tests/numeric_stability.rs
use proptest::prelude::*;

proptest! {
    #[test]
    fn epsilon_distinguishes_nearby_points(
        x in 0.0f32..10000.0,
        y in 0.0f32..10000.0
    ) {
        let p1 = Point::new(Pixels(x), Pixels(y));
        let p2 = Point::new(Pixels(x + 1e-6), Pixels(y));
        let p3 = Point::new(Pixels(x + 1e-5), Pixels(y));

        // Within epsilon
        prop_assert!(p1.approx_eq(p2));

        // Beyond epsilon (10x threshold)
        prop_assert!(!p1.approx_eq(p3));
    }
}
```

**Success Criteria**: Property tests pass across coordinate range 0-10000

---

## 3. Memory Contracts

### Contract 3.1: Memory Layout Limits

**Guarantee**: Types MUST NOT exceed specified memory sizes.

**Verification Method**: Compile-time assertions

**Size Limits**:
```rust
// src/lib.rs or tests/memory_layout_test.rs

const _: () = assert!(std::mem::size_of::<Point<Pixels>>() <= 8);
const _: () = assert!(std::mem::size_of::<Size<Pixels>>() <= 8);
const _: () = assert!(std::mem::size_of::<Rect<Pixels>>() <= 20);
const _: () = assert!(std::mem::size_of::<Color>() <= 16);
const _: () = assert!(std::mem::size_of::<EdgeInsets<Pixels>>() <= 16);
const _: () = assert!(std::mem::size_of::<Offset<Pixels>>() <= 8);
```

**Rationale**:
- 8 bytes = 2× f32 (Point, Size, Offset)
- 16 bytes = 4× f32 (Color, EdgeInsets)
- 20 bytes = Point + Size (16) + padding allowance

**Success Criteria**: Compilation fails if size limits exceeded

---

### Contract 3.2: Zero Heap Allocations

**Guarantee**: All public API methods MUST NOT perform heap allocations.

**Verification Method**: Manual code review + allocation testing

**Stack-Only Design**:
```rust
// ✅ All types are Copy (stack-allocated)
#[derive(Copy, Clone)]
pub struct Point<T: Unit> { x: T, y: T }

// ✅ All operations return by value (no Box, no Vec, no String)
impl<T: Unit> Point<T> {
    pub fn distance_to(self, other: Self) -> f32 { /* ... */ }
}
```

**Forbidden Patterns**:
```rust
// ❌ NO heap allocations
Box::new(...)       // Heap allocation
Vec::new()          // Heap allocation
String::from(...)   // Heap allocation
Arc::new(...)       // Heap allocation
```

**Testing**:
```rust
// Use global allocator tracking in tests
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn test_no_allocations_in_hot_path() {
    let _profiler = dhat::Profiler::new_heap();

    let p1 = Point::new(Pixels(10.0), Pixels(20.0));
    let p2 = Point::new(Pixels(30.0), Pixels(40.0));
    let _ = p1.distance_to(p2);

    // Verify zero allocations
    let stats = dhat::HeapStats::get();
    assert_eq!(stats.total_blocks, 0);
}
```

**Success Criteria**: Zero allocations detected in all hot path operations

---

### Contract 3.3: Copy Semantics

**Guarantee**: All types MUST implement `Copy` for efficient pass-by-value.

**Verification Method**: Trait bound checks

**Implementation**:
```rust
// All unit types
#[derive(Copy, Clone)]
pub struct Pixels(pub f32);

// All geometric types
#[derive(Copy, Clone)]
pub struct Point<T: Unit> { pub x: T, pub y: T }

// Color type
#[derive(Copy, Clone)]
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }
```

**Success Criteria**: All public types implement `Copy + Clone`

---

## 4. Behavioral Contracts

### Contract 4.1: Geometric Invariants

**Guarantee**: Geometric operations MUST preserve mathematical properties.

**Verification Method**: Property-based tests using `proptest`

**Invariant Properties**:

#### Rectangle Intersection is Commutative
```rust
proptest! {
    #[test]
    fn rect_intersection_commutative(
        r1 in arbitrary_rect(),
        r2 in arbitrary_rect()
    ) {
        let i1 = r1.intersect(r2);
        let i2 = r2.intersect(r1);
        prop_assert!(i1.approx_eq(i2));
    }
}
```

#### Rectangle Union Contains Both Inputs
```rust
proptest! {
    #[test]
    fn rect_union_contains_both(
        r1 in arbitrary_rect(),
        r2 in arbitrary_rect()
    ) {
        let union = r1.union(r2);

        // Union must contain all points from r1 and r2
        prop_assert!(union.contains(r1.origin));
        prop_assert!(union.contains(r2.origin));

        // Union dimensions >= both inputs
        prop_assert!(union.size.width.to_f32() >= r1.size.width.to_f32());
        prop_assert!(union.size.height.to_f32() >= r1.size.height.to_f32());
    }
}
```

#### Point Distance is Symmetric
```rust
proptest! {
    #[test]
    fn point_distance_symmetric(
        p1 in arbitrary_point(),
        p2 in arbitrary_point()
    ) {
        let d1 = p1.distance_to(p2);
        let d2 = p2.distance_to(p1);
        prop_assert!((d1 - d2).abs() < EPSILON);
    }
}
```

#### Triangle Inequality
```rust
proptest! {
    #[test]
    fn triangle_inequality(
        p1 in arbitrary_point(),
        p2 in arbitrary_point(),
        p3 in arbitrary_point()
    ) {
        let d12 = p1.distance_to(p2);
        let d23 = p2.distance_to(p3);
        let d13 = p1.distance_to(p3);

        // d(p1, p3) ≤ d(p1, p2) + d(p2, p3)
        prop_assert!(d13 <= d12 + d23 + EPSILON);
    }
}
```

**Success Criteria**: All property tests pass with 1000+ random inputs

---

### Contract 4.2: Empty Rectangle Identification

**Guarantee**: Empty rectangles MUST be clearly identifiable via `is_empty()` method.

**Verification Method**: Unit tests with edge cases

**Test Cases**:
```rust
#[test]
fn test_empty_rectangles() {
    // Zero width
    let r1 = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(0.0), Pixels(10.0));
    assert!(r1.is_empty());

    // Zero height
    let r2 = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(10.0), Pixels(0.0));
    assert!(r2.is_empty());

    // Both zero
    let r3 = Rect::zero();
    assert!(r3.is_empty());

    // Non-empty
    let r4 = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(10.0), Pixels(10.0));
    assert!(!r4.is_empty());
}
```

**Success Criteria**: All edge cases handled correctly

---

### Contract 4.3: Rectangle Normalization

**Guarantee**: Negative rectangle dimensions MUST adjust origin to preserve visual bounds.

**Verification Method**: Unit tests with negative dimensions

**Behavior Specification**:
```rust
#[test]
fn test_negative_width_normalization() {
    // Negative width: adjust left edge
    let r = Rect::from_ltwh(Pixels(100.0), Pixels(50.0), Pixels(-20.0), Pixels(30.0));

    // Origin adjusted: left = 100 - 20 = 80
    assert_eq!(r.left().to_f32(), 80.0);
    assert_eq!(r.top().to_f32(), 50.0);

    // Dimensions clamped positive
    assert_eq!(r.size.width.to_f32(), 20.0);
    assert_eq!(r.size.height.to_f32(), 30.0);

    // Visual bounds preserved: right edge still at 100
    assert_eq!(r.right().to_f32(), 100.0);
}

#[test]
fn test_negative_height_normalization() {
    // Negative height: adjust top edge
    let r = Rect::from_ltwh(Pixels(100.0), Pixels(50.0), Pixels(20.0), Pixels(-30.0));

    // Origin adjusted: top = 50 - 30 = 20
    assert_eq!(r.left().to_f32(), 100.0);
    assert_eq!(r.top().to_f32(), 20.0);

    // Dimensions clamped positive
    assert_eq!(r.size.width.to_f32(), 20.0);
    assert_eq!(r.size.height.to_f32(), 30.0);

    // Visual bounds preserved: bottom edge still at 50
    assert_eq!(r.bottom().to_f32(), 50.0);
}
```

**Success Criteria**: Visual bounds preserved for all negative dimension cases

---

## 5. Color System Contracts

### Contract 5.1: Blending Mode Availability

**Guarantee**: Color system MUST provide three distinct blending modes.

**Verification Method**: API documentation + unit tests

**Required Methods**:

#### Linear Interpolation (`mix`)
```rust
impl Color {
    /// Linear interpolation between two colors
    /// ratio = 0.0 returns self, ratio = 1.0 returns other
    pub fn mix(&self, other: &Color, ratio: f32) -> Color { /* ... */ }
}

#[test]
fn test_color_mix_boundaries() {
    let red = Color::RED;
    let blue = Color::BLUE;

    // Boundary: ratio = 0.0
    assert_eq!(red.mix(&blue, 0.0), red);

    // Boundary: ratio = 1.0
    assert_eq!(red.mix(&blue, 1.0), blue);

    // Midpoint: ratio = 0.5
    let purple = red.mix(&blue, 0.5);
    assert_eq!(purple.r, 0.5);
    assert_eq!(purple.b, 0.5);
}
```

#### Alpha Compositing (`blend_over`)
```rust
impl Color {
    /// Porter-Duff Source Over compositing
    /// Composite this color over background
    pub fn blend_over(&self, background: &Color) -> Color { /* ... */ }
}

#[test]
fn test_blend_over_alpha_compositing() {
    // Semi-transparent red over opaque white
    let red = Color::from_rgba(255, 0, 0, 0.5);
    let white = Color::WHITE;
    let result = red.blend_over(&white);

    // Result should be pink (red + white mixed)
    assert!(result.r > 0.5); // More red than white
    assert!(result.g > 0.0); // Some white contribution
    assert_eq!(result.a, 1.0); // Fully opaque
}
```

#### RGB Scaling (`scale`)
```rust
impl Color {
    /// Multiply RGB values by factor
    pub fn scale(&self, factor: f32) -> Color { /* ... */ }
}

#[test]
fn test_color_scale() {
    let red = Color::RED;

    // Darken by 50%
    let dark_red = red.scale(0.5);
    assert_eq!(dark_red.r, 0.5);
    assert_eq!(dark_red.a, 1.0); // Alpha unchanged
}
```

**Success Criteria**: All three methods available with correct semantics

---

### Contract 5.2: Color Adjustment Modes

**Guarantee**: Color system MUST provide both HSL-based and RGB-based adjustments.

**Verification Method**: Unit tests with known color values

**HSL-based Methods**:
```rust
#[test]
fn test_hsl_lighten() {
    let red = Color::RED;
    let lighter = red.lighten(0.2); // 20% lighter via HSL

    // Lightness increased (via HSL conversion)
    let hsl = lighter.to_hsl();
    assert!(hsl.l > 0.5); // Brighter
}

#[test]
fn test_hsl_darken() {
    let red = Color::RED;
    let darker = red.darken(0.2); // 20% darker via HSL

    // Lightness decreased
    let hsl = darker.to_hsl();
    assert!(hsl.l < 0.5); // Dimmer
}
```

**RGB-based Methods**:
```rust
#[test]
fn test_rgb_scale() {
    let red = Color::RED;
    let darker = red.scale(0.5); // 50% RGB scale

    // Direct RGB multiplication
    assert_eq!(darker.r, 0.5);
    assert_eq!(darker.g, 0.0);
    assert_eq!(darker.b, 0.0);
}
```

**Success Criteria**: Both HSL and RGB adjustment methods available

---

### Contract 5.3: Hex Color Parsing

**Guarantee**: Hex color parsing MUST support two formats and handle errors appropriately.

**Verification Method**: Unit tests with valid and invalid inputs

**Valid Formats**:
```rust
#[test]
fn test_hex_parsing_6_digit() {
    let color = Color::from_hex("#FF5733").unwrap();
    assert_eq!(color.r, 1.0); // 255/255
    assert_eq!(color.g, 0x57 as f32 / 255.0);
    assert_eq!(color.b, 0x33 as f32 / 255.0);
    assert_eq!(color.a, 1.0); // Default full opacity
}

#[test]
fn test_hex_parsing_8_digit() {
    let color = Color::from_hex("#FF573380").unwrap();
    assert_eq!(color.r, 1.0);
    assert_eq!(color.a, 0x80 as f32 / 255.0); // Alpha from hex
}
```

**Error Handling**:
```rust
#[test]
#[should_panic(expected = "Invalid hex color")]
fn test_invalid_hex_debug_panics() {
    // In debug builds: panic with clear message
    let _ = Color::from_hex("#GGHHII");
}

#[test]
#[cfg(not(debug_assertions))]
fn test_invalid_hex_release_fallback() {
    // In release builds: fallback to transparent black with warning
    let color = Color::from_hex("#GGHHII");
    assert_eq!(color, Color::TRANSPARENT);

    // Warning log emitted (verify in log output)
    // tracing::warn!("Invalid hex color '#GGHHII', using transparent black");
}
```

**Success Criteria**:
- Both 6-digit and 8-digit formats parsed correctly
- Debug builds panic with actionable message
- Release builds fall back to transparent black with warning log

---

## Contract Compliance Summary

### Automated Verification

| Contract Category | Verification Tool | Run With |
|-------------------|-------------------|----------|
| Type Safety | `trybuild` | `cargo test --test compile_fail` |
| Performance | `criterion` | `cargo bench` |
| Memory Layout | Compile-time asserts | `cargo build` |
| Geometric Invariants | `proptest` | `cargo test --test property_tests` |
| Color Blending | Unit tests | `cargo test --test color_tests` |
| Error Handling | Unit tests | `cargo test --test edge_cases` |

### Continuous Integration Checks

```yaml
# .github/workflows/contracts.yml

- name: Type Safety Contracts
  run: cargo test --test compile_fail

- name: Performance Contracts
  run: cargo bench --no-fail-fast

- name: Memory Layout Contracts
  run: cargo build --release

- name: Behavioral Contracts
  run: cargo test --test property_tests -- --test-threads=1

- name: Color System Contracts
  run: cargo test --lib color
```

---

## Regression Prevention

### Performance Regression Detection

```toml
# benches/criterion_config.toml

[default]
significance_level = 0.05
noise_threshold = 0.05  # ±5% tolerance
```

**CI Integration**:
```bash
# Compare against baseline
cargo bench --save-baseline main
git checkout feature-branch
cargo bench --baseline main

# Fail if regression > 5%
```

### Contract Violation Alerts

**Compile-Time Violations**: Build failure
**Runtime Violations**: Test failure
**Performance Violations**: Benchmark failure in CI

---

## Contract Evolution

### Adding New Contracts

1. Document new contract in this file
2. Create verification test
3. Update CI pipeline
4. Tag with version number

### Deprecating Contracts

Contracts MUST NOT be removed. They can only be:
- **Expanded**: Add new guarantees (backwards compatible)
- **Tightened**: Strengthen existing guarantees (breaking change - MAJOR version bump)
- **Deprecated**: Mark as legacy (MINOR version bump with migration guide)

---

## Next Steps

1. ✅ API contracts documented
2. → Create quickstart guide (quickstart.md)
3. → Run agent context update script
4. → Ready for `/speckit.tasks` command
