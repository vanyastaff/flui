# Implementation Plan: flui-types Crate

**Branch**: `001-flui-types` | **Date**: 2026-01-26 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-flui-types/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

The flui-types crate is the foundational measurement and geometry library for the Flui framework. It provides type-safe unit wrappers (Pixels, DevicePixels, Rems, ScaledPixels) that prevent cross-platform rendering bugs by catching unit mixing at compile time. The crate also provides core geometric primitives (Point, Size, Rectangle) with zero-cost abstractions, and a complete color system supporting RGB, HSL, and multiple blending modes. As a foundation crate with no internal Flui dependencies, it establishes the type-safety patterns used throughout the framework.

**Technical Approach**: Leverage Rust's generic system and newtype pattern for zero-cost unit safety. Use const generics where beneficial (e.g., fixed-size arrays for corners). Implement core traits (Add, Sub, Mul, PartialEq) with appropriate unit constraints. Optimize for stack allocation and compiler inlining. Provide both generic foundation types and concrete application types following constitution guidelines.

## Technical Context

**Language/Version**: Rust 1.75+ (MSRV for const generic features)
**Primary Dependencies**:
- **Core**: std library only (no external deps for core functionality)
- **Optional**: SIMD optimizations (feature-gated, e.g., `packed_simd` or `std::simd`)
- **Testing**: proptest 1.5+ (property-based tests), criterion 0.5+ (benchmarks)

**Storage**: N/A (pure computational library, no persistence)

**Testing**:
- `cargo test --workspace` for unit and integration tests
- `cargo test --all-features` for SIMD feature validation
- Property-based tests via `proptest` for geometric invariants
- Microbenchmarks via `criterion` for performance validation

**Target Platform**: All platforms (Windows, macOS, Linux, WASM)
- No platform-specific code or FFI
- Must compile and pass tests on all targets
- WASM compatibility verified in CI

**Project Type**: Foundation library (Rust crate)

**Performance Goals**:
- Point distance calculation: <10 nanoseconds
- Rectangle intersection: <20 nanoseconds
- Color blending (mix, blend_over): <20 nanoseconds
- Unit conversions: optimized away by compiler (zero cost)
- Clean build: <5 seconds
- All operations inline-friendly (<50 LOC per hot path method)

**Constraints**:
- **Memory**: Point ≤8 bytes, Size ≤8 bytes, Rect ≤20 bytes, Color ≤16 bytes
- **Zero allocations**: No heap allocations in any public API methods
- **Stack-only**: All types must be `Copy` or small enough to clone cheaply
- **Compile-time safety**: Unit mixing must be impossible at compile time
- **Error tolerance**: Floating-point epsilon = 1e-6 (0.000001) for equality comparisons

**Scale/Scope**:
- ~10-15 public types (unit types, geometric primitives, color types)
- ~200-300 public methods across all types
- Target: ≥80% code coverage (constitution requirement for foundation crates)
- Estimated: ~3000-4000 lines of production code, ~2000-3000 lines of tests

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

### ✅ I. Flutter-Inspired Architecture
**Status**: NOT APPLICABLE (foundation crate)
- This is a foundation crate providing primitive types, not UI components
- No three-tree architecture concerns at this level
- Public API follows Rust idioms (new(), from_*(), to_*()) rather than Flutter patterns

### ✅ II. Type Safety First
**Status**: PASS - Core mission of this crate
- **Generic units**: `Point<T: Unit>`, `Rect<T: Unit>` prevent unit mixing at compile time
- **Foundation layer**: Generics allowed per constitution ("Foundation crates MAY use generics")
- **Zero-cost abstractions**: Newtype pattern compiles to raw f32 operations
- **Typestate pattern**: Not applicable (no stateful transitions in primitive types)
- **Typed IDs**: Not applicable (no ID types in this crate - defined in flui-tree)

### ✅ III. Modular Architecture
**Status**: PASS
- Foundation layer crate with no Flui internal dependencies
- External dependencies: minimal (testing/benchmarking only)
- Clear single responsibility: measurement units and geometry primitives
- Can be tested independently
- Can be published as standalone crate if needed

### ✅ IV. Test-First for Public APIs
**Status**: MUST IMPLEMENT
- All public trait methods (Add, Sub, Mul, PartialEq, etc.) must have tests before implementation
- All public APIs (new(), from_*(), to_*(), contains(), intersects(), etc.) must have tests first
- Verify RED state before GREEN state for all P1 user stories
- Integration tests for unit conversions (verify compile errors for invalid operations)
- **Coverage target**: ≥80% per constitution

### ✅ V. Explicit Over Implicit
**Status**: PASS
- No hidden conversions between unit types (all conversions via explicit `to_*()` methods)
- No RefCell or interior mutability (all types are Copy or immutable)
- Error handling explicit: invalid hex codes panic in debug, log warnings in release
- Edge cases documented: negative rectangles normalize with explicit origin adjustment

### ✅ VI. Code Quality Standards
**Status**: MUST IMPLEMENT
- **Logging**: NOT APPLICABLE (pure computational library, no logging points)
  - Exception: Invalid hex color parsing in release mode must log warning
  - Use `tracing::warn!` for hex parse fallback only
- **Error Handling**: Use `thiserror` for any Error types (e.g., hex color parsing)
- **Thread Safety**: All types are Copy/Clone with no shared state - inherently thread-safe
- **Frame Scheduling**: NOT APPLICABLE (no event loop in this crate)

**Note**: This crate is performance-critical foundation code. No logging in hot paths (distance(), contains(), etc.). Only log on invalid input (hex parsing fallback).

### ✅ VII. Incremental Development
**Status**: PASS
- Spec defines 10 user stories with clear priorities (P1, P2, P3)
- P1 stories (Device-Independent Layout, Unit Mixing Prevention, Geometric Calculations) deliverable independently
- P2 stories (Rems, Conversions, Padding/Margins, Colors) can be developed in parallel [P]
- P3 stories (Precise Rendering, Corner Radii, RTL Support) optional enhancements

### ✅ VIII. User Experience Consistency
**Status**: PASS
- API consistency: `Point::new()`, `Size::new()`, `Rect::from_ltwh()` follow Rust conventions
- Edge cases documented: negative rectangles, invalid colors, division by zero
- Error messages: Compile-time for unit mixing ("Cannot add Rect<Pixels> and Offset<DevicePixels>")
- Documentation: Visual examples for color blending modes, rectangle normalization

### ✅ IX. Performance Requirements
**Status**: MUST VALIDATE
- **Frame Budget**: NOT APPLICABLE (this crate used in layout/paint but has no frame concept)
- **Phase Budgets**: Operations must complete in <10-20ns per spec requirements
- **Memory Targets**: Explicit byte limits per type (Point≤8, Rect≤20, Color≤16)
- **Hot Path Requirements**:
  - ✅ Zero allocations (all stack-based Copy types)
  - ✅ Inline small functions (mark all public methods with #[inline])
  - ✅ No allocations (no Vec, no Box, no Arc in public types)
- **Benchmarks**: criterion benchmarks required for all performance claims

**Verification Method**:
```rust
// Compile-time size checks
const _: () = assert!(std::mem::size_of::<Point<Pixels>>() <= 8);
const _: () = assert!(std::mem::size_of::<Rect<Pixels>>() <= 20);
const _: () = assert!(std::mem::size_of::<Color>() <= 16);
```

### Overall Gate Status: ✅ PASS (with implementation requirements)

**Action Items Before Phase 1**:
1. Research SIMD optimization patterns for color blending (optional feature)
2. Research property-based testing patterns for geometric invariants
3. Research criterion benchmark setup for <10ns target validation

---

## Project Structure

### Documentation (this feature)

```text
specs/001-flui-types/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
│   └── README.md        # API contracts for unit types, geometric primitives, colors
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/flui_types/
├── src/
│   ├── lib.rs                    # Crate root with module exports and prelude
│   ├── units/
│   │   ├── mod.rs                # Unit trait and base implementations
│   │   ├── pixels.rs             # Pixels, DevicePixels, Rems, ScaledPixels
│   │   ├── conversions.rs        # Unit conversion methods
│   │   └── constants.rs          # Common constants (EPSILON = 1e-6)
│   ├── geometry/
│   │   ├── mod.rs                # Geometric types module
│   │   ├── point.rs              # Point<T: Unit>
│   │   ├── size.rs               # Size<T: Unit>
│   │   ├── rect.rs               # Rect<T: Unit>
│   │   ├── offset.rs             # Offset<T: Unit> (delta between points)
│   │   ├── edges.rs              # EdgeInsets<T: Unit> (top, right, bottom, left)
│   │   └── corners.rs            # Corners<T> (generic over value type)
│   ├── styling/
│   │   ├── mod.rs                # Styling types module
│   │   ├── color.rs              # Color (RGBA), HSL conversions
│   │   ├── color_blend.rs        # Blending modes (mix, blend_over, scale)
│   │   └── color_names.rs        # Named color constants
│   └── prelude.rs                # Common imports for consumers
│
├── tests/
│   ├── unit_tests/
│   │   ├── units_test.rs         # Unit type tests (compile-time + runtime)
│   │   ├── geometry_test.rs      # Point, Size, Rect tests
│   │   └── color_test.rs         # Color system tests
│   ├── integration_tests/
│   │   ├── unit_mixing_test.rs   # Verify compile errors (trybuild)
│   │   └── conversions_test.rs   # Cross-unit conversion tests
│   └── property_tests/
│       ├── geometry_properties.rs # Proptest for geometric invariants
│       └── color_properties.rs    # Proptest for color operations
│
├── benches/
│   ├── geometry_bench.rs         # Criterion benchmarks for Point, Rect
│   ├── color_bench.rs            # Criterion benchmarks for Color ops
│   └── conversions_bench.rs      # Benchmark unit conversions
│
├── examples/
│   ├── basic_usage.rs            # Simple demonstration of core types
│   ├── unit_conversions.rs       # Device-independent to device pixel examples
│   └── color_blending.rs         # Color mixing and blending examples
│
├── Cargo.toml                    # Dependencies: proptest, criterion (dev-deps only)
└── README.md                     # Crate overview, features, usage
```

**Structure Decision**:
This is a **foundation library crate** (Option 1: Single project). The flui-types crate is organized into three main modules:

1. **units/**: Core unit types and trait definitions. Provides the Unit trait and concrete implementations (Pixels, DevicePixels, Rems, ScaledPixels). Handles unit conversions with explicit scale factors.

2. **geometry/**: Geometric primitives parameterized by unit types. Point, Size, Rect, Offset, EdgeInsets, and Corners are all generic over Unit types, enabling type-safe calculations.

3. **styling/**: Color system with RGB/HSL support. Independent of unit types. Provides multiple blending modes (mix for lerp, blend_over for alpha compositing, scale for RGB multiplication).

The structure follows Rust library conventions with `lib.rs` as the entry point, a `prelude` module for common imports, and comprehensive test coverage across `tests/`, `benches/`, and `examples/`.

---

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| N/A | No constitution violations | All requirements met within constitution |

**Justification Summary**: This crate fully complies with the constitution. Foundation crates are explicitly allowed to use generics (Principle II). All other requirements (test-first, performance targets, documentation) are followed.

---

## Phase 0: Research & Technical Decisions

**Goal**: Resolve all NEEDS CLARIFICATION items and document technical decisions.

### Research Tasks

1. **SIMD Optimization Patterns** [OPTIONAL]
   - **Question**: Should color blending operations use SIMD for performance?
   - **Research**: Compare scalar vs SIMD implementations for `mix()`, `blend_over()`, `scale()`
   - **Decision Criteria**: Performance gain >2x AND complexity acceptable
   - **Risk**: SIMD may not compile on all targets (WASM, ARM)
   - **Recommendation**: Implement scalar first, add SIMD as feature-gated optimization in Phase 3

2. **Property-Based Testing Patterns**
   - **Question**: What geometric invariants should proptest verify?
   - **Research**: Common geometric property patterns (intersection commutative, union associative, etc.)
   - **Deliverable**: List of properties for `geometry_properties.rs`
   - **Example Properties**:
     - `rect.intersect(other) == other.intersect(rect)` (commutative)
     - `rect.union(other).contains(rect)` (union bounds both)
     - `point.distance_to(other) == other.distance_to(point)` (symmetric)

3. **Criterion Benchmark Setup**
   - **Question**: How to structure benchmarks to verify <10ns targets?
   - **Research**: Criterion best practices for microbenchmarks (avoid optimizer tricks)
   - **Deliverable**: Benchmark harness setup in `benches/`
   - **Key Techniques**: Use `black_box()` to prevent compiler from optimizing away code

4. **Compile-Time Unit Mixing Prevention**
   - **Question**: How to generate clear error messages for unit type mismatches?
   - **Research**: Custom trait bounds and derive macros for better error messages
   - **Deliverable**: Error message examples and trait design
   - **Goal**: Error message like "Cannot add Rect<Pixels> and Offset<DevicePixels>"

5. **Epsilon Value Validation**
   - **Question**: Is 1e-6 epsilon appropriate for all geometric operations?
   - **Research**: Typical UI coordinate ranges (0-10000), precision requirements
   - **Deliverable**: Validation that 1e-6 works for expected coordinate ranges
   - **Risk**: Too large epsilon may consider distinct points equal
   - **Validation**: Test at boundary conditions (0.0001 pixel differences)

### Output

**File**: `research.md`

**Contents**:
- Decision log for each research task
- Rationale for technical choices
- Benchmarks and measurements supporting decisions
- Links to relevant resources (Rust docs, similar libraries)
- Open questions requiring user input

---

## Phase 1: Design & API Contracts

**Prerequisites**: `research.md` complete

### 1. Data Model

**File**: `data-model.md`

**Contents**:

#### Unit Types (Foundation Layer - Generics Allowed)

**Unit Trait**
- Purpose: Marker trait for all measurement unit types
- Methods: `ZERO: Self`, `to_f32(&self) -> f32`, `from_f32(f32) -> Self`
- Constraints: Must be Copy + Clone + PartialEq
- Implementors: Pixels, DevicePixels, Rems, ScaledPixels

**Pixels (Logical Pixels)**
- Purpose: Device-independent layout units
- Storage: Newtype wrapper around f32
- Operations: Add, Sub, Mul<f32>, Div<f32>
- Conversions: `to_device_pixels(scale: f32)`, `to_rems(base_font_size: f32)`

**DevicePixels (Screen Pixels)**
- Purpose: Physical screen pixels for GPU rendering
- Storage: Newtype wrapper around f32
- Operations: Add, Sub, Mul<f32>, Div<f32>
- Conversions: `to_logical_pixels(scale: f32)`

**Rems (Font-Relative Units)**
- Purpose: Typography-based spacing (accessible layouts)
- Storage: Newtype wrapper around f32
- Operations: Add, Sub, Mul<f32>, Div<f32>
- Conversions: `to_pixels(base_font_size: f32)`

**ScaledPixels (Internal Framework Use)**
- Purpose: Pre-scaling calculations
- Storage: Newtype wrapper around f32
- Operations: Add, Sub, Mul<f32>, Div<f32>
- Conversions: `to_pixels(scale: f32)`

#### Geometric Primitives (Generic over Unit)

**Point\<T: Unit\>**
- Fields: `x: T`, `y: T`
- Methods: `new()`, `distance_to()`, `offset_by()`, `approx_eq(epsilon)`
- Operators: `Point + Offset = Point`, `Point - Point = Offset`

**Size\<T: Unit\>**
- Fields: `width: T`, `height: T`
- Methods: `new()`, `is_empty()`, `area()`, `scale()`
- Constraints: width ≥ 0, height ≥ 0 (normalized in constructor)

**Rect\<T: Unit\>**
- Fields: `origin: Point<T>`, `size: Size<T>`
- Methods: `from_ltwh()`, `contains()`, `intersects()`, `intersect()`, `union()`, `inflate()`, `deflate()`, `inset_by()`
- Properties: `left()`, `top()`, `right()`, `bottom()`, `center()`
- Normalization: Negative dimensions adjust origin to preserve visual bounds

**Offset\<T: Unit\>**
- Fields: `dx: T`, `dy: T`
- Purpose: Delta between two points or displacement vector
- Methods: `new()`, `magnitude()`, `normalized()`

**EdgeInsets\<T: Unit\>**
- Fields: `top: T`, `right: T`, `bottom: T`, `left: T`
- Methods: `all()`, `symmetric()`, `only()`, `horizontal()`, `vertical()`
- Purpose: Padding, margins, safe areas

**Corners\<T\>**
- Fields: `top_left: T`, `top_right: T`, `bottom_right: T`, `bottom_left: T`
- Methods: `all()`, `top()`, `bottom()`, `only()`
- Purpose: Corner radii, per-corner values

#### Color System (Unit-Independent)

**Color**
- Storage: `r: f32`, `g: f32`, `b: f32`, `a: f32` (normalized 0.0-1.0)
- Methods:
  - Constructors: `from_rgb()`, `from_rgba()`, `from_hex()`, `from_hsl()`
  - Operations: `with_opacity()`, `mix()`, `blend_over()`, `scale()`, `lighten()`, `darken()`
  - Conversions: `to_rgb()`, `to_rgba()`, `to_hsl()`
- Validation: RGB values clamped to [0, 255], alpha clamped to [0.0, 1.0]
- Edge Cases: Invalid hex codes panic in debug, fall back to transparent black with warning log in release

**HSL (Utility Struct)**
- Fields: `h: f32` (0-360), `s: f32` (0-1), `l: f32` (0-1)
- Purpose: Intermediate representation for color adjustments
- Not a public type - used internally for lighten/darken operations

### 2. API Contracts

**Directory**: `contracts/`

**File**: `contracts/README.md`

**Contents**:

#### Unit Type Contracts

**Contract 1: Type Safety**
- MUST: Prevent mixing incompatible units at compile time
- MUST: Provide explicit conversion methods only
- MUST: Self-document conversions (method names show direction)
- Verification: Compile-time tests using `trybuild` crate

**Contract 2: Zero-Cost Abstractions**
- MUST: Compile to raw f32 operations (no runtime overhead)
- MUST: Optimize conversions away when scale factor is constant
- Verification: Assembly inspection (`cargo asm`), criterion benchmarks

**Contract 3: Numeric Stability**
- MUST: Use epsilon = 1e-6 for equality comparisons
- MUST: Handle edge cases (0.0, infinity, NaN) per spec
- Verification: Property tests with extreme values

#### Geometric Primitive Contracts

**Contract 4: Geometric Invariants**
- MUST: Rectangle intersection is commutative
- MUST: Rectangle union contains both inputs
- MUST: Point distance is symmetric
- MUST: Empty rectangles are clearly identifiable via `is_empty()`
- Verification: Property-based tests

**Contract 5: Memory Layout**
- MUST: Point ≤ 8 bytes
- MUST: Size ≤ 8 bytes
- MUST: Rect ≤ 20 bytes
- MUST: Color ≤ 16 bytes
- Verification: Compile-time assertions

**Contract 6: Performance Targets**
- MUST: Point distance < 10ns (measured via criterion)
- MUST: Rectangle intersection < 20ns (measured via criterion)
- MUST: Color blending < 20ns (measured via criterion)
- Verification: Continuous benchmarking in CI

#### Color System Contracts

**Contract 7: Color Blending Modes**
- MUST: Provide `mix()` for linear interpolation (lerp)
- MUST: Provide `blend_over()` for alpha compositing
- MUST: Provide `scale()` for RGB value multiplication
- MUST: Provide `lighten()`/`darken()` using HSL lightness
- Verification: Unit tests with known color values

**Contract 8: Color Parsing**
- MUST: Parse hex codes: "#RRGGBB", "#RRGGBBAA"
- MUST: Panic in debug for invalid hex with clear message
- MUST: Fall back to transparent black in release with warning log
- Verification: Test with invalid inputs

### 3. Quickstart Guide

**File**: `quickstart.md`

**Contents**:

```markdown
# flui-types Quickstart

## Basic Usage

### Unit Types

```rust
use flui_types::prelude::*;

// Device-independent layout
let button_width = Pixels(100.0);
let button_height = Pixels(50.0);

// Rendering (scale factor 2.0 for Retina)
let device_width = button_width.to_device_pixels(2.0); // DevicePixels(200.0)

// Accessible spacing
let padding = Rems(2.0);
let pixel_padding = padding.to_pixels(16.0); // Pixels(32.0)
```

### Geometric Primitives

```rust
use flui_types::geometry::*;

// Create a button rectangle
let button = Rect::from_ltwh(
    Pixels(10.0),  // left
    Pixels(10.0),  // top
    Pixels(100.0), // width
    Pixels(50.0),  // height
);

// Hit testing
let tap_position = Point::new(Pixels(50.0), Pixels(30.0));
if button.contains(tap_position) {
    println!("Button tapped!");
}

// Padding
let padding = EdgeInsets::all(Pixels(8.0));
let content_area = button.inset_by(padding);
```

### Colors

```rust
use flui_types::styling::*;

// From design specs
let brand_color = Color::from_hex("#FF5733").unwrap();

// Hover state (80% opacity)
let hover_color = brand_color.with_opacity(0.8);

// Mix two colors
let mixed = Color::RED.mix(&Color::BLUE, 0.5); // Linear interpolation

// Lighten for highlight
let highlight = brand_color.lighten(0.2); // 20% lighter via HSL
```

## Testing Your Code

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_hit_area() {
        let button = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(50.0));

        // Inside button
        assert!(button.contains(Point::new(Pixels(50.0), Pixels(25.0))));

        // Outside button
        assert!(!button.contains(Point::new(Pixels(150.0), Pixels(25.0))));
    }
}
```

## Common Patterns

### Pattern 1: Layout to Rendering Pipeline

```rust
// Layout phase (device-independent)
let layout_rect = Rect::from_ltwh(/* ... */);

// Convert for GPU rendering
let scale_factor = window.scale_factor();
let device_rect = Rect::new(
    layout_rect.origin.to_device_pixels(scale_factor),
    layout_rect.size.to_device_pixels(scale_factor),
);
```

### Pattern 2: Responsive Spacing

```rust
// Base font size from user preferences
let base_font_size = user_settings.font_size(); // Pixels(16.0)

// Spacing scales with font size
let padding = Rems(1.5).to_pixels(base_font_size.0); // Pixels(24.0)
```

### Pattern 3: Color Theming

```rust
struct Theme {
    primary: Color,
    background: Color,
}

impl Theme {
    fn hover_color(&self) -> Color {
        self.primary.lighten(0.1)
    }

    fn disabled_color(&self) -> Color {
        self.primary.with_opacity(0.5)
    }
}
```

## Performance Tips

1. **Use Copy semantics**: All types are Copy - pass by value
2. **Avoid allocations**: All operations are stack-based
3. **Trust the optimizer**: Unit conversions with const scale factors are free
4. **Batch conversions**: Convert once, use many times

## Next Steps

- Read the [API documentation](https://docs.rs/flui_types)
- See [examples/](../examples/) for complete demonstrations
- Check [tests/](../tests/) for comprehensive usage patterns
```

### 4. Agent Context Update

**Script**: `.specify/scripts/powershell/update-agent-context.ps1 -AgentType claude`

**Expected Changes**:
- Add `flui_types` crate to technology list
- Add property-based testing (proptest) if not present
- Add microbenchmarking (criterion) if not present
- Preserve manual additions between markers

---

## Phase 2: Task Generation

**Command**: `/speckit.tasks` (separate command, NOT part of /speckit.plan)

**Deliverable**: `tasks.md` with detailed implementation tasks organized by priority and phase.

---

## Completion Checklist

- [x] Technical Context filled (no NEEDS CLARIFICATION)
- [x] Constitution Check passed
- [x] Project Structure documented (source layout defined)
- [x] Complexity Tracking completed (no violations)
- [x] Phase 0 Research Tasks defined
- [x] Phase 1 Data Model specified
- [x] Phase 1 API Contracts documented
- [x] Phase 1 Quickstart Guide drafted
- [ ] Phase 0 research.md generated (next step)
- [ ] Phase 1 artifacts generated (after research)
- [ ] Phase 2 tasks.md generated (separate command)

---

## Next Actions

1. **Execute Phase 0**: Generate `research.md` by dispatching research agents
2. **Review Research**: Validate technical decisions before proceeding
3. **Execute Phase 1**: Generate data-model.md, contracts/, quickstart.md
4. **Update Agent Context**: Run update-agent-context script
5. **Ready for Tasks**: Run `/speckit.tasks` to generate implementation tasks
