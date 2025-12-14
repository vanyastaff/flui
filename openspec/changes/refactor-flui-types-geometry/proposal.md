# Change: Refactor flui_types Geometry to Industry Standards

## Why

The current `flui_types` geometry module has semantic inconsistencies that violate mathematical conventions used by industry-standard libraries (kurbo, euclid, glam). Specifically:

1. **Point contains vector operations** - `Point::normalize()`, `dot()`, `cross()`, `length()` are mathematically vector operations, not point operations
2. **No dedicated Vec2 type** - Missing fundamental 2D vector type for direction/magnitude representation
3. **Inconsistent precision** - Uses `f32` while industry standard (kurbo) uses `f64` for better precision
4. **No interoperability** - Missing `mint` integration prevents seamless library interop
5. **Partial unit type support** - Only `Point` has unit types, but `Rect`, `Size`, `RRect` do not

## Library Integration Analysis

We analyzed all flui_types modules to determine which should use external libraries vs custom implementations:

| Category | Decision | External Library | Notes |
|----------|----------|------------------|-------|
| **Geometry (Point, Vec2, Rect, Size, Offset, RRect)** | Custom + mint | mint (feature) | f64 default, unit types, mint interop |
| **Matrix4** | Custom | mint (feature) | Keep f32, SIMD, add mint conversion |
| **Path/Bezier** | Custom + kurbo | kurbo (optional) | kurbo for precise path operations |
| **Color** | Custom + palette | palette (optional) | palette for advanced color science |
| **Animation curves** | Custom | — | Flutter-compatible API essential |
| **Physics simulations** | Custom | — | Domain-specific to UI animation |
| **Layout/Constraints** | Custom | — | f64 default |

### Rationale for Custom Implementations

**Geometry (custom + mint):**
- kurbo has different API style (not Flutter-compatible)
- euclid is inspiration but too complex for our needs
- glam is game-focused without unit types
- **mint provides interop layer** without forcing dependency

**Matrix4 (custom):**
- Our implementation already has SIMD (SSE/NEON)
- f32 is correct for GPU operations
- glam would add unnecessary dependency

**Color (custom + optional palette):**
- u8 RGBA is GPU-native (our focus)
- Custom SIMD lerp/blend
- **palette feature** for advanced color science (Lab, wide gamut) when needed

**Animation curves (custom):**
- Flutter-compatible Curve trait system is essential
- No standard Rust library provides these exact APIs

## What Changes

### **BREAKING** - Precision Change
- All geometry types migrate from `f32` to `f64` as default precision
- Use pure Rust generics: `Point<T = f64>`, `Rect<T = f64>`, etc.
- Explicit `<f32>` at GPU boundaries only

### **BREAKING** - Semantic Correction
- Remove vector operations from `Point` type
- Add new `Vec2<T, U>` type for vector operations
- Fix operator semantics: `Point - Point = Vec2`, `Point + Vec2 = Point`

### New Features - Core
- Add `mint` feature flag for library interoperability
- Extend unit types to `Rect`, `Size`, `RRect`, `Vec2`
- Add kurbo-compatible naming conventions (`hypot()` vs `length()`)

### New Features - Optional Dependencies
- `mint` feature: Zero-cost conversions with glam, nalgebra, cgmath
- `kurbo` feature: Precise BezPath, arc length, path operations
- `palette` feature: Advanced color science (Lab, Delta E, wide gamut)

### Improved Algorithms
- Adopt kurbo-style adaptive subdivision for Bezier curves
- Add proper floating-point comparison with configurable epsilon

## Impact

- **Affected code:** `crates/flui_types/src/geometry/` (all files)
- **Affected code:** `crates/flui_types/src/painting/path.rs`
- **Affected crates:** All crates depending on `flui_types` geometry (flui_core, flui_rendering, flui_widgets, flui_app)
- **Migration required:** Update all usages of `Point` vector methods to use new `Vec2` type
- **Performance:** `f64` may have minor impact on GPU upload, mitigated by explicit `<f32>`

## Success Criteria

1. All geometry types follow mathematical semantics (Point vs Vector distinction)
2. Full unit type support across all geometry primitives
3. `mint` interoperability enables zero-cost conversions with glam, nalgebra, kurbo
4. All existing tests pass after migration
5. Documentation includes migration guide for breaking changes
6. Optional features (`kurbo`, `palette`) work correctly when enabled
