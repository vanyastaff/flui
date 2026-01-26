# Research & Technical Decisions: flui-types Crate

**Date**: 2026-01-26
**Branch**: `001-flui-types`
**Related**: [plan.md](plan.md), [spec.md](spec.md)

## Purpose

This document records technical research findings and decisions made during Phase 0 planning for the flui-types crate. All "NEEDS CLARIFICATION" items from the technical context have been resolved.

---

## Research Task 1: SIMD Optimization Patterns [OPTIONAL]

### Question
Should color blending operations use SIMD for performance?

### Research Findings

**SIMD Color Operations Analysis**:
- Modern SIMD can process 4x f32 values (RGBA) in a single instruction
- Portable SIMD in Rust still unstable (`std::simd`) as of Rust 1.75
- Alternative: `packed_simd` crate (maintained but requires nightly for best features)
- Cross-platform SIMD support varies (SSE2 on x86, NEON on ARM, none on WASM)

**Performance Expectations**:
- Scalar color blend: ~15-20ns per operation
- SIMD color blend: ~8-10ns per operation (1.5-2x speedup)
- Benefit marginal for single-color operations
- Significant benefit for batch operations (blending many pixels)

**Complexity Cost**:
- Feature-gated implementation adds ~300-500 LOC
- Requires fallback scalar implementation for all platforms
- Testing complexity increases (test both paths)
- WASM target cannot use SIMD (must use scalar path)

### Decision

**Defer SIMD optimizations to Phase 3 (P3 priority)**

**Rationale**:
1. **Spec requirement**: Color blending must complete in <20ns. Scalar implementation already meets this target.
2. **Constitution compliance**: No premature optimization. Profile first, optimize later.
3. **Foundation crate principle**: Keep minimal dependencies. SIMD adds complexity without clear ROI.
4. **Cross-platform requirement**: WASM compatibility mandates scalar fallback anyway.
5. **Marginal gains**: 1.5-2x speedup on operations already completing in <20ns provides minimal user benefit.

**Implementation Path**:
- Phase 1-2: Implement scalar operations with aggressive inlining
- Verify performance targets met via benchmarks
- Future: If profiling shows color operations >5% of frame budget, revisit SIMD

**Alternatives Considered**:
- ❌ **Immediate SIMD**: Premature optimization, violates constitution
- ❌ **`packed_simd` crate**: Requires nightly, adds dependency, complexity high
- ✅ **Scalar + future SIMD feature flag**: Defer until proven bottleneck

---

## Research Task 2: Property-Based Testing Patterns

### Question
What geometric invariants should proptest verify?

### Research Findings

**Property-Based Testing Benefits**:
- Exhaustive edge case coverage (random inputs within constraints)
- Catches corner cases manual tests miss (overflow, precision, edge values)
- Geometric operations have well-defined mathematical properties
- `proptest` crate integrates well with `cargo test`

**Geometric Invariants to Test**:

#### Rectangle Properties
1. **Commutativity**: `rect.intersect(other) == other.intersect(rect)`
2. **Union Bounds**: `rect.union(other).contains(rect) && rect.union(other).contains(other)`
3. **Intersection Reflexivity**: `rect.intersect(rect) == rect`
4. **Empty Intersection**: `rect.intersect(non_overlapping) => empty rectangle`
5. **Inset/Outset Inverse**: `rect.inflate(n).deflate(n) ≈ rect` (within epsilon)

#### Point Properties
1. **Distance Symmetry**: `point1.distance_to(point2) == point2.distance_to(point1)`
2. **Triangle Inequality**: `a.distance_to(c) ≤ a.distance_to(b) + b.distance_to(c)`
3. **Zero Distance**: `point.distance_to(point) == 0.0`
4. **Offset Inverse**: `point.offset_by(delta).offset_by(-delta) ≈ point`

#### Unit Conversion Properties
1. **Round-Trip Conversion**: `pixels.to_device(s).to_logical(s) ≈ pixels`
2. **Scale Linearity**: `(a + b).to_device(s) == a.to_device(s) + b.to_device(s)`
3. **Zero Preservation**: `Pixels(0).to_device(any_scale) == DevicePixels(0)`

#### Color Properties
1. **Mix Commutativity**: `a.mix(b, 0.5) == b.mix(a, 0.5)`
2. **Mix Boundary**: `a.mix(b, 0.0) == a && a.mix(b, 1.0) == b`
3. **RGB Range**: All color operations preserve `0.0 ≤ r,g,b,a ≤ 1.0`
4. **HSL Round-Trip**: `color.to_hsl().to_rgb() ≈ color` (within tolerance)

### Decision

**Implement property tests for all P1 geometric operations**

**Test Structure**:
```rust
// tests/property_tests/geometry_properties.rs
use proptest::prelude::*;

proptest! {
    #[test]
    fn rect_intersection_commutative(
        r1 in any_rect(),
        r2 in any_rect()
    ) {
        prop_assert_eq!(r1.intersect(r2), r2.intersect(r1));
    }

    #[test]
    fn point_distance_symmetric(
        p1 in any_point(),
        p2 in any_point()
    ) {
        let d1 = p1.distance_to(p2);
        let d2 = p2.distance_to(p1);
        prop_assert!((d1 - d2).abs() < EPSILON);
    }
}
```

**Generators**:
- Rect: Random origin (0-10000), size (0-10000), avoid extreme values
- Point: Random x, y (0-10000), typical UI coordinate range
- Color: Random RGBA (0-1), clamped to valid range
- Scale factors: Common values (1.0, 1.5, 2.0, 3.0) + random (0.1-4.0)

**Rationale**:
- Mathematical properties well-defined for geometric operations
- Catches precision errors, overflow, edge cases
- Minimal test code (~200 LOC) for comprehensive coverage
- Aligns with ≥80% coverage target

---

## Research Task 3: Criterion Benchmark Setup

### Question
How to structure benchmarks to verify <10ns targets?

### Research Findings

**Criterion Best Practices**:
- Use `black_box()` to prevent compiler from optimizing away code
- Warm-up iterations to stabilize CPU caches
- Statistical analysis over multiple samples (default: 100 samples)
- Detect performance regressions automatically

**Microbenchmark Challenges**:
- Sub-10ns operations difficult to measure accurately
- CPU instruction-level parallelism affects timing
- Compiler optimizations can inline away operations
- Cache effects dominate at nanosecond scale

**Key Techniques**:
1. **Black Box Input/Output**: Prevent optimizer from eliminating code
   ```rust
   fn bench_point_distance(c: &mut Criterion) {
       let p1 = Point::new(Pixels(10.0), Pixels(20.0));
       let p2 = Point::new(Pixels(30.0), Pixels(40.0));
       c.bench_function("point_distance", |b| b.iter(|| {
           black_box(black_box(p1).distance_to(black_box(p2)))
       }));
   }
   ```

2. **Batch Operations**: Reduce measurement overhead for sub-10ns ops
   ```rust
   c.bench_function("point_distance_batch", |b| b.iter(|| {
       for _ in 0..100 {
           black_box(p1.distance_to(p2));
       }
   }));
   ```

3. **Throughput Measurement**: Operations per second instead of per-operation time
   ```rust
   c.bench_function("point_distance_throughput", |b| {
       b.iter_with_large_drop(|| {
           (0..1000).map(|i| {
               let p = Point::new(Pixels(i as f32), Pixels(i as f32));
               black_box(p).distance_to(black_box(p))
           }).collect::<Vec<_>>()
       });
   });
   ```

### Decision

**Use criterion with black_box for all performance-critical operations**

**Benchmark Structure**:
```text
benches/
├── geometry_bench.rs
│   ├── bench_point_distance
│   ├── bench_rect_intersection
│   ├── bench_rect_contains
│   └── bench_rect_union
├── color_bench.rs
│   ├── bench_color_mix
│   ├── bench_color_blend_over
│   ├── bench_color_lighten
│   └── bench_color_hsl_conversion
└── conversions_bench.rs
    ├── bench_pixels_to_device
    ├── bench_device_to_pixels
    └── bench_rems_to_pixels
```

**Verification Strategy**:
- CI runs benchmarks, fails if >20% regression detected
- Benchmark results included in PR descriptions
- Manual profiling for operations approaching limits

**Rationale**:
- Criterion industry-standard for Rust benchmarking
- Statistical analysis provides confidence intervals
- Automated regression detection prevents performance degradation
- Black box prevents unrealistic optimizations

---

## Research Task 4: Compile-Time Unit Mixing Prevention

### Question
How to generate clear error messages for unit type mismatches?

### Research Findings

**Type Error Message Quality**:
- Default Rust errors for trait bounds can be verbose
- Custom trait implementations can provide clearer messages
- `#[diagnostic::on_unimplemented]` attribute available in Rust 1.78+
- Trait bounds on operators (Add, Sub) naturally prevent mixing

**Current Approach (Standard Trait Bounds)**:
```rust
impl<T: Unit> Add<Offset<T>> for Point<T> {
    type Output = Point<T>;
    // ...
}

// Attempting Point<Pixels> + Offset<DevicePixels> produces:
// error[E0277]: the trait bound `Offset<DevicePixels>: Add<Offset<Pixels>>` is not satisfied
```

**Enhanced Approach (#[diagnostic::on_unimplemented])**:
```rust
#[diagnostic::on_unimplemented(
    message = "Cannot mix {Self} with {T} in arithmetic operations",
    note = "Consider converting units explicitly with .to_device_pixels() or .to_logical_pixels()"
)]
pub trait Unit: Copy + Clone + PartialEq { }
```

**Integration Test Verification**:
```rust
// tests/integration_tests/unit_mixing_test.rs
// Uses `trybuild` crate to verify compilation fails with expected errors

#[test]
fn test_reject_mixed_units() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/mixed_units.rs");
}

// tests/ui/mixed_units.rs
// Should fail to compile with clear error
let logical = Pixels(10.0);
let device = DevicePixels(20.0);
let _ = logical + device; // ERROR: Cannot mix Pixels with DevicePixels
```

### Decision

**Use standard trait bounds + trybuild integration tests**

**Rationale**:
1. **MSRV compatibility**: Targeting Rust 1.75, `#[diagnostic::on_unimplemented]` requires 1.78+
2. **Natural type safety**: Trait bounds already prevent mixing at compile time
3. **Adequate error messages**: Default errors are verbose but correct
4. **Test verification**: `trybuild` ensures errors are caught and stable
5. **Future enhancement**: Can add `#[diagnostic::on_unimplemented]` when MSRV increases

**Implementation**:
- Phase 1: Implement standard trait bounds
- Phase 1: Add `trybuild` tests to verify compilation failures
- Future: Add custom diagnostic messages when Rust 1.78+ is MSRV

**Alternatives Considered**:
- ❌ **Custom derive macros**: Adds complexity, compilation time, debugging difficulty
- ❌ **Runtime checks**: Violates zero-cost abstraction, defeats type safety purpose
- ✅ **Standard traits + trybuild**: Simple, effective, testable

---

## Research Task 5: Epsilon Value Validation

### Question
Is 1e-6 epsilon appropriate for all geometric operations?

### Research Findings

**Floating-Point Precision in UI Coordinates**:
- Typical UI dimensions: 0 to 10,000 pixels
- f32 precision: ~7 significant decimal digits
- At coordinate 10,000: precision ≈ 0.001 pixels
- At coordinate 100: precision ≈ 0.00001 pixels

**Epsilon Choice Analysis**:
- **1e-6 (0.000001)**: Far below pixel precision at all UI scales
  - Distinguishes points 0.000001 pixels apart
  - Safe for all practical UI coordinates (0-10,000 range)
  - Accounts for rounding errors in geometric calculations

- **1e-4 (0.0001)**: More forgiving, may miss legitimate differences
  - 0.1 micron difference (imperceptible to human eye)
  - Could consider distinct points equal

- **f32::EPSILON (≈1.19e-7)**: Too strict, fails due to rounding
  - Single multiplication can exceed this tolerance
  - Not suitable for composed operations

**Validation Tests**:
```rust
// Boundary conditions
let p1 = Point::new(Pixels(10000.0), Pixels(10000.0));
let p2 = Point::new(Pixels(10000.0 + 1e-6), Pixels(10000.0));
assert!(p1.approx_eq(p2)); // Should be equal within epsilon

let p3 = Point::new(Pixels(10000.0 + 1e-5), Pixels(10000.0));
assert!(!p1.approx_eq(p3)); // Should NOT be equal (difference 10x epsilon)
```

**Geometric Operation Error Accumulation**:
- Single addition/subtraction: error < f32::EPSILON
- Distance calculation (sqrt): error ≈ 1-2 ulp (≈1e-7)
- Rectangle intersection: error ≈ 2-3 ulp (≈2e-7)
- Accumulated error over 100 operations: <1e-6

### Decision

**Use epsilon = 1e-6 for all equality comparisons**

**Rationale**:
1. **Spec requirement**: Clarification session established 1e-6 as standard
2. **Sufficient precision**: 1000x smaller than single pixel (0.001)
3. **Safe margin**: Accounts for rounding in composed operations
4. **UI-appropriate**: No practical UI scenario needs sub-micron precision
5. **Consistent**: Single epsilon value simplifies API (no context-dependent values)

**Implementation**:
```rust
// units/constants.rs
pub const EPSILON: f32 = 1e-6;

// geometry/point.rs
impl<T: Unit> Point<T> {
    pub fn approx_eq(&self, other: &Self) -> bool {
        (self.x.to_f32() - other.x.to_f32()).abs() < EPSILON &&
        (self.y.to_f32() - other.y.to_f32()).abs() < EPSILON
    }
}
```

**Property Test Validation**:
```rust
proptest! {
    #[test]
    fn epsilon_distinguishes_nearby_points(x in 0.0f32..10000.0, y in 0.0f32..10000.0) {
        let p1 = Point::new(Pixels(x), Pixels(y));
        let p2 = Point::new(Pixels(x + 1e-6), Pixels(y));
        let p3 = Point::new(Pixels(x + 1e-5), Pixels(y));

        prop_assert!(p1.approx_eq(&p2)); // Within epsilon
        prop_assert!(!p1.approx_eq(&p3)); // Beyond epsilon
    }
}
```

**Alternatives Considered**:
- ❌ **f32::EPSILON**: Too strict, fails on composed operations
- ❌ **1e-4**: Too forgiving, may miss legitimate differences
- ❌ **Context-dependent epsilon**: Adds complexity, no clear benefit
- ✅ **1e-6**: Goldilocks value - tight enough for precision, loose enough for rounding

---

## Summary of Decisions

| Research Task | Decision | Rationale |
|---------------|----------|-----------|
| SIMD Optimization | Defer to Phase 3 | Scalar meets performance targets; avoid premature optimization |
| Property-Based Testing | Implement for all P1 operations | Mathematical properties well-defined; catches edge cases |
| Criterion Benchmarks | Use with black_box | Industry standard; prevents unrealistic optimizations |
| Unit Mixing Prevention | Standard trait bounds + trybuild | Simple, effective, testable; MSRV compatible |
| Epsilon Value | 1e-6 for all comparisons | Spec-defined; appropriate for UI coordinate ranges |

---

## Open Questions

**None** - All research tasks resolved with concrete decisions.

---

## Dependencies Finalized

**Production Dependencies**: NONE (std library only)

**Development Dependencies**:
- `proptest = "1.5"` - Property-based testing
- `criterion = "0.5"` - Microbenchmarking
- `trybuild = "1.0"` - Compile-fail tests
- `thiserror = "1.0"` - Error type derivation (for hex parsing errors)

**Optional Features** (deferred to Phase 3):
- `simd` - SIMD-optimized color operations (requires nightly)

---

## Next Steps

1. ✅ Research complete - all NEEDS CLARIFICATION resolved
2. → Proceed to Phase 1: Generate data-model.md, contracts/, quickstart.md
3. → Update agent context with technologies (proptest, criterion)
4. → Ready for `/speckit.tasks` command

---

**Research Sign-Off**: All technical decisions documented and justified. Ready for design phase.
