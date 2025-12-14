# Design: Geometry Types Refactoring

## Context

FLUI's geometry types are foundational to the entire framework. They are used in:
- Layout calculations (constraints, sizing)
- Hit testing (point containment)
- Rendering (path construction, transformations)
- Animation (interpolation)

Current implementation has good coverage but violates mathematical semantics that industry libraries follow. This creates confusion and prevents seamless interoperability.

### Stakeholders
- **Framework users:** Need intuitive, mathematically correct APIs
- **Integration developers:** Need mint/glam interop for ecosystem compatibility
- **Rendering pipeline:** Needs efficient f32 for GPU, but f64 for precision in calculations

## Goals / Non-Goals

### Goals
1. **Semantic correctness:** Point represents position, Vec2 represents direction/magnitude
2. **Industry compatibility:** Follow kurbo/euclid naming and API conventions
3. **Interoperability:** Enable mint-based conversions with zero overhead
4. **Type safety:** Unit types on all geometry primitives prevent coordinate system bugs
5. **Precision:** f64 default for calculations, f32 via explicit generic parameter

### Non-Goals
- Full kurbo API compatibility (only adopt proven patterns)
- Support for 3D geometry (separate concern)
- SIMD optimization (defer to glam via mint)
- Breaking the existing public API unnecessarily (minimize migration burden)
- Type aliases like `Point32` (not Rust-idiomatic)

## Decisions

### Decision 1: Add Vec2 Type, Keep Offset

**What:** Add `Vec2<T, U>` as mathematical vector type. Keep `Offset` as Flutter-compatible UI displacement type.

**Why:**
- `Vec2` follows kurbo/euclid convention for mathematical operations
- `Offset` is Flutter terminology familiar to target users
- Both can coexist: `Vec2` for math, `Offset` for UI-specific APIs

**Alternatives considered:**
- Replace `Offset` with `Vec2` entirely - Rejected: breaks Flutter API familiarity
- Make `Offset` an alias for `Vec2` - Rejected: `Offset` has `dx/dy` fields, `Vec2` has `x/y`

### Decision 2: f64 Default, Pure Generics (No Type Aliases)

**What:** All geometry types are fully generic with `f64` as default. No `*32` type aliases — use Rust generics idiomatically.

```rust
// Generic types with f64 default:
pub struct Point<T = f64, U = UnknownUnit> { ... }
pub struct Vec2<T = f64, U = UnknownUnit> { ... }
pub struct Rect<T = f64, U = UnknownUnit> { ... }
pub struct Size<T = f64, U = UnknownUnit> { ... }

// Usage - Rust type inference handles precision:
let p1 = Point::new(1.0, 2.0);             // Point<f64> (default)
let p2: Point<f32> = Point::new(1.0, 2.0); // Point<f32> (explicit)
let p3 = Point::<f32>::new(1.0, 2.0);      // Point<f32> (turbofish)

// GPU boundary - explicit f32:
fn upload_to_gpu(vertices: &[Point<f32>]) { ... }
```

**Why:**
- **Rust-idiomatic:** Generics with defaults, not type aliases for variants
- kurbo uses f64 for precision in curve calculations
- Type inference makes explicit `<f32>` only needed at GPU boundaries
- Cleaner API — one type name, parameterized by precision

**Alternatives considered:**
- Keep f32 default - Rejected: precision issues in complex layouts
- Add `Point32`, `Vec32` aliases - Rejected: not Rust-idiomatic, clutters API

### Decision 3: Correct Operator Semantics

**What:** Implement mathematically correct operator overloads.

```rust
// Point operations
impl Sub<Point<T,U>> for Point<T,U> {
    type Output = Vec2<T,U>;  // Point - Point = Vector
}

impl Add<Vec2<T,U>> for Point<T,U> {
    type Output = Point<T,U>;  // Point + Vector = Point
}

// Vector operations
impl Add<Vec2<T,U>> for Vec2<T,U> {
    type Output = Vec2<T,U>;  // Vector + Vector = Vector
}

impl Mul<T> for Vec2<T,U> {
    type Output = Vec2<T,U>;  // Vector * Scalar = Vector
}
```

**Why:**
- Mathematical correctness enables intuitive usage
- Prevents semantic errors at compile time
- Matches kurbo, euclid, and academic literature

**Alternatives considered:**
- Keep current (Point + Point = Point) - Rejected: mathematically incorrect

### Decision 4: Unit Types on All Primitives

**What:** Extend `PhantomData<U>` unit marker to all geometry types.

```rust
pub struct Point<T = f64, U = UnknownUnit> { x: T, y: T, _unit: PhantomData<U> }
pub struct Vec2<T = f64, U = UnknownUnit> { x: T, y: T, _unit: PhantomData<U> }
pub struct Size<T = f64, U = UnknownUnit> { width: T, height: T, _unit: PhantomData<U> }
pub struct Rect<T = f64, U = UnknownUnit> { min: Point<T,U>, max: Point<T,U> }
pub struct RRect<T = f64, U = UnknownUnit> { rect: Rect<T,U>, ... }
```

**Why:**
- Prevents mixing screen/world/local coordinates
- Zero runtime cost (PhantomData is ZST)
- euclid proves this pattern at scale (WebRender/Servo)

**Alternatives considered:**
- Unit types only on Point - Rejected: incomplete protection

### Decision 5: mint Feature Flag for Interop

**What:** Add `mint` optional dependency with From/Into implementations.

```rust
#[cfg(feature = "mint")]
impl<T: Copy> From<mint::Point2<T>> for Point<T> { ... }

#[cfg(feature = "mint")]
impl<T: Copy> From<Point<T>> for mint::Point2<T> { ... }
```

**Why:**
- mint is the standard interop crate (used by glam, nalgebra, cgmath)
- Zero-cost conversions via From/Into
- Optional - no dependency for users who don't need interop

**Alternatives considered:**
- Direct glam dependency - Rejected: mint provides broader compatibility
- No interop - Rejected: limits ecosystem integration

### Decision 6: Naming Convention Alignment

**What:** Align method names with kurbo conventions where applicable.

| Current | New (kurbo-style) | Reason |
|---------|-------------------|--------|
| `Vec2::distance()` | `Vec2::hypot()` | kurbo convention |
| `Vec2::length()` | `Vec2::hypot()` | Consistency |
| `Vec2::length_squared()` | `Vec2::hypot2()` | kurbo convention |
| `Rect::expand_by()` | `Rect::inflate()` | Industry standard |
| `Point::lerp()` | Keep as `lerp()` | Universal name |

**Why:**
- Familiar to kurbo/euclid users
- `hypot` is mathematically precise term (hypotenuse)
- Reduces cognitive load when switching between libraries

**Alternatives considered:**
- Keep all current names - Rejected: misses alignment opportunity
- Full kurbo naming - Rejected: some names less intuitive

### Decision 7: Optional Library Features

**What:** Add optional feature flags for external library integrations.

```toml
[features]
default = []
mint = ["dep:mint"]        # Interop with glam, nalgebra, cgmath
kurbo = ["dep:kurbo"]      # Precise path operations
palette = ["dep:palette"]  # Advanced color science

[dependencies]
mint = { version = "0.5", optional = true }
kurbo = { version = "0.11", optional = true }
palette = { version = "0.7", optional = true }
```

**Why each library:**

| Library | Purpose | When to use |
|---------|---------|-------------|
| **mint** | Interop types | Always recommended for ecosystem compat |
| **kurbo** | BezPath, arc length, precise bounds | When need accurate path math |
| **palette** | Lab, Oklab, wide gamut, Delta E | When need color science |

**What each feature provides:**

**mint feature:**
```rust
impl From<mint::Point2<T>> for Point<T> { ... }
impl From<Point<T>> for mint::Point2<T> { ... }
impl From<mint::Vector2<T>> for Vec2<T> { ... }
impl From<mint::ColumnMatrix4<f32>> for Matrix4 { ... }
// Enables seamless: let p: Point = glam_vec.into();
```

**kurbo feature:**
```rust
impl Path {
    /// Uses kurbo's adaptive subdivision for precise bounds
    pub fn precise_bounds(&self) -> Rect { ... }
    
    /// Arc length using kurbo's algorithms
    pub fn arc_length(&self, accuracy: f64) -> f64 { ... }
    
    /// Convert to kurbo BezPath for advanced operations
    pub fn to_kurbo(&self) -> kurbo::BezPath { ... }
}
```

**palette feature:**
```rust
impl Color {
    /// Convert to Lab color space (perceptual)
    pub fn to_lab(&self) -> palette::Lab { ... }
    
    /// Convert to Oklab (improved perceptual)
    pub fn to_oklab(&self) -> palette::Oklab { ... }
    
    /// Perceptual color difference (Delta E 2000)
    pub fn delta_e(&self, other: Color) -> f32 { ... }
}
```

**Why:**
- **Custom implementations remain lean** - No forced dependencies
- **Opt-in power features** - Users choose complexity level
- **Industry-standard interop** - mint is the ecosystem bridge
- **Advanced operations available** - kurbo/palette for precision needs

**Alternatives considered:**
- Make kurbo/palette required - Rejected: bloats core crate
- Re-implement kurbo algorithms - Rejected: proven, maintained code exists
- No interop - Rejected: limits ecosystem integration

## Type Hierarchy

```
Geometry Types (all generic over T and U):
├── Point<T, U>      - Position in 2D space
├── Vec2<T, U>       - Direction + magnitude (new)
├── Size<T, U>       - Width/height dimensions
├── Rect<T, U>       - Axis-aligned bounding box
├── RRect<T, U>      - Rounded rectangle
└── Offset           - UI displacement (Flutter compat, keeps f32)

Defaults:
├── T defaults to f64 (precision for calculations)
└── U defaults to UnknownUnit (no coordinate space enforcement)

Unit Types:
├── UnknownUnit        - Default, no type safety
├── ScreenSpace        - Screen coordinates (pixels)
├── WorldSpace         - World/scene coordinates
└── LocalSpace         - Widget-local coordinates
```

## Risks / Trade-offs

### Risk 1: Breaking Change Migration Burden
- **Impact:** All code using `Point` vector methods must change
- **Mitigation:** 
  - Provide migration guide with search/replace patterns
  - Deprecate old methods for one release before removal
  - Compiler errors will guide users to correct types

### Risk 2: f64 Performance on GPU Path
- **Impact:** Extra conversion when uploading to GPU buffers
- **Mitigation:**
  - Use `Point<f32>` explicitly at GPU boundaries
  - Conversion happens once at GPU boundary, not per-frame
  - f64 calculations are worth precision in layout

### Risk 3: Increased Compile Time from Generics
- **Impact:** More generic code may increase compile times
- **Mitigation:**
  - Generic internals monomorphize to same code
  - Minimal impact expected (geometry is small crate)

### Risk 4: mint Dependency Concerns
- **Impact:** Additional optional dependency
- **Mitigation:**
  - Feature-flagged, not default
  - mint is tiny (types only, no code)
  - Widely trusted in ecosystem

## Migration Plan

### Phase 1: Add New Types (Non-Breaking)
1. Add `Vec2<T, U>` type with full API
2. Add unit types (`ScreenSpace`, `WorldSpace`, `LocalSpace`)
3. Add `mint` feature flag with conversions
4. Migrate types to f64 default

### Phase 2: Deprecate Old APIs
1. Mark `Point::normalize()`, `Point::dot()`, etc. as `#[deprecated]`
2. Add deprecation messages pointing to `Vec2` equivalents
3. Update documentation with migration examples

### Phase 3: Remove Deprecated APIs (Next Major)
1. Remove deprecated methods from `Point`
2. Update all internal usages to new types
3. Publish migration guide

### Rollback Strategy
- Each phase is independently deployable
- Phase 1 is purely additive (zero risk)
- Phase 2 deprecations can be reverted
- Phase 3 is major version, expected breaking

## Open Questions

1. **Should `Offset` also get unit types?**
   - Current thinking: No, keep it simple for Flutter API compatibility
   - `Offset` is specifically for UI displacement, less need for unit safety

2. **Should we add `Box2D` (euclid) as alias for `Rect`?**
   - Current thinking: No, `Rect` is more intuitive
   - Can add later if there's demand

3. **Should `Transform`/`Matrix4` also migrate to f64?**
   - Current thinking: Yes, for consistency
   - GPU transform upload is infrequent, precision matters
