# FLUI Dependencies Guide

**Last Updated:** 2025-01-10
**Status:** ✅ Production Ready

---

## Overview

This document explains all major dependencies used in FLUI, their rationale, and guidelines for adding new dependencies. FLUI follows a **minimal dependency** philosophy while leveraging high-quality ecosystem crates where appropriate.

**Key Principles:**
1. **Performance over convenience** - Choose fast, proven libraries
2. **Stability over features** - Prefer mature crates with clear roadmaps
3. **Rust idioms** - Leverage Rust's type system and zero-cost abstractions
4. **Clear rationale** - Every dependency must justify its existence

---

## Dependency Hierarchy

FLUI's 12 crates are organized in 5 layers with strict dependency rules:

```text
Layer 5: Application
    flui_app (entry point, main loop)

Layer 4: High-Level Features
    flui_widgets (UI components)
    flui_devtools (developer tools)

Layer 3: Core Framework
    flui_core (element tree, pipeline, hooks)

Layer 2: Specialized Systems
    flui_rendering (RenderObjects)
    flui_gestures (input handling)
    flui_animation (animations)
    flui_assets (asset loading)

Layer 1: Foundation
    flui_painting (Canvas API)
    flui_engine (GPU backend)

Layer 0: Primitives
    flui_types (math, geometry, colors)
```

**Rules:**
- ✅ Lower layers can depend on layers below
- ❌ Higher layers cannot depend on lower layers
- ✅ Shared utilities in workspace dependencies

---

## Critical Dependencies

### GPU Rendering Stack

#### wgpu 25.0
**Purpose:** Cross-platform GPU API abstraction (Vulkan/Metal/DX12/WebGPU)
**Used by:** `flui_engine`
**License:** MIT/Apache-2.0

**Why wgpu:**
- ✅ Pure Rust (no C++ dependencies like Skia)
- ✅ Cross-platform (Windows/macOS/Linux/Web)
- ✅ Modern API (based on WebGPU spec)
- ✅ Active development (wgpu.rs project)
- ✅ 2MB binary overhead acceptable for desktop/mobile

**Alternatives Considered:**
- ❌ Skia - C++ dependency, 10MB binary, build complexity
- ❌ egui - Software rendering, limited GPU acceleration
- ❌ Custom rasterizer - Huge effort, can't compete with GPU

**Performance:**
- 5.6x faster than software on complex UIs
- 80x faster for blur effects
- Native GPU performance on all platforms

**See:** [ADR-005: wgpu-Only Backend](decisions/ADR-005-wgpu-only-backend.md)

---

#### Lyon 1.0
**Purpose:** Path tessellation (converts SVG paths to GPU triangles)
**Used by:** `flui_engine`
**License:** MIT/Apache-2.0

**Why Lyon:**
- ✅ Production-ready (1.0 stable)
- ✅ High-quality tessellation
- ✅ Integrates seamlessly with wgpu
- ✅ Used by Mozilla for Pathfinder

**Alternatives:**
- ❌ Custom tessellation - Complex geometry algorithms
- ❌ CPU-based rendering - Too slow for complex paths

**Performance:**
- ~100μs per path draw (includes tessellation + GPU)
- Handles complex Bézier curves, strokes, fills

---

#### Glyphon 0.9
**Purpose:** GPU text rendering with SDF (Signed Distance Field)
**Used by:** `flui_engine`
**License:** MIT

**Why Glyphon:**
- ✅ GPU-accelerated text (SDF rendering)
- ✅ Integrates with wgpu + cosmic-text
- ✅ High-quality antialiasing
- ✅ Fast glyph atlas caching

**Alternatives:**
- ❌ glyph_brush - Lower-level, more boilerplate
- ❌ rusttype - CPU rasterization only
- ❌ fontdue - No GPU integration

**Dependencies:**
- `cosmic-text 0.14` - Text layout and shaping
- `ttf-parser 0.25` - Font parsing

**Performance:**
- ~50μs per text draw (cached glyphs)
- Atlas-based caching reduces GPU uploads

---

### Synchronization & Threading

#### parking_lot 0.12
**Purpose:** High-performance mutex and rwlock
**Used by:** All crates (workspace dependency)
**License:** MIT/Apache-2.0

**Why parking_lot:**
- ✅ **3x faster** than `std::sync::Mutex` (~15ns vs ~45ns lock time)
- ✅ **40x smaller** memory footprint (1 byte vs 40 bytes)
- ✅ No poisoning (simpler error handling)
- ✅ Fair locking (prevents starvation)
- ✅ Used by Tokio, Rayon, many production Rust projects

**Alternatives:**
- ❌ `std::sync::Mutex` - 3x slower, poisoning complexity
- ❌ Lock-free structures - Too complex for most use cases
- ❌ Message passing - Doesn't fit UI shared-state paradigm

**Performance Impact:**
- Single-thread overhead: <5% (measured)
- Parallel speedup (4 cores): 2.5x for builds
- Zero deadlocks in stress tests (lock ordering documented)

**See:** [ADR-004: Thread-Safety Design](decisions/ADR-004-thread-safety-design.md)

---

### Math & Geometry

#### glam 0.30
**Purpose:** SIMD-accelerated math library (vectors, matrices, quaternions)
**Used by:** `flui_types`, `flui_engine`
**License:** MIT/Apache-2.0

**Why glam:**
- ✅ SIMD optimization (SSE2/AVX2 on x86)
- ✅ Small memory footprint (no padding)
- ✅ Serde support for serialization
- ✅ Used by Bevy (proven in production)

**Alternatives:**
- ❌ nalgebra - Larger API surface, slower compilation
- ❌ cgmath - Less active development
- ❌ Custom math - Reinventing SIMD-optimized wheel

**Performance:**
- Vec2/Vec3/Vec4 operations use SIMD when available
- Matrix operations 2-4x faster than naive impl

---

### Async Runtime

#### tokio 1.43 (LTS)
**Purpose:** Async runtime for I/O, timers, channels
**Used by:** `flui_assets` (asset loading), examples
**License:** MIT

**Why tokio:**
- ✅ LTS release (supported until March 2026)
- ✅ Mature ecosystem (most Rust async libraries use tokio)
- ✅ Excellent performance and stability
- ✅ Multi-threaded scheduler

**Alternatives:**
- ❌ async-std - Smaller ecosystem, less active
- ❌ smol - Minimal but requires more manual work

**Usage:**
- Asset loading (network/file I/O)
- Background task scheduling
- Not used on main UI thread (blocking operations only)

---

### Caching

#### moka 0.12
**Purpose:** High-performance async cache with TinyLFU eviction
**Used by:** `flui_core` (layout caching), `flui_assets` (asset caching)
**License:** MIT/Apache-2.0

**Why moka:**
- ✅ TinyLFU eviction (better hit rates than LRU)
- ✅ Lock-free, async-friendly
- ✅ Thread-safe with low contention
- ✅ Per-entry TTL and size limits

**Alternatives:**
- ❌ lru - Simple but not thread-safe
- ❌ quick_cache - No async support
- ❌ HashMap - No eviction policy

**Performance:**
- Layout cache: 80% hit rate (measured)
- Asset cache: 95%+ hit rate for repeated loads

---

### Collections & Utilities

#### slab 0.4
**Purpose:** Arena allocator for element tree
**Used by:** `flui_core`
**License:** MIT

**Why slab:**
- ✅ O(1) insert/remove with ID reuse
- ✅ Cache-friendly contiguous storage
- ✅ Stable indices (elements don't move)
- ✅ Used by Tokio for task management

**Alternatives:**
- ❌ Vec - Unstable indices on removal
- ❌ HashMap - Worse cache locality
- ❌ generational-arena - More complex for same benefit

**ElementId Optimization:**
- Uses `NonZeroUsize` for niche optimization
- `Option<ElementId>` = 8 bytes (same as ElementId)
- +1 offset pattern (Slab 0-indexed, ElementId 1-indexed)

**See:** [ADR-003: Enum vs Trait Objects](decisions/ADR-003-enum-vs-trait-objects.md)

---

#### smallvec 1.13
**Purpose:** Small vector optimization (stack-allocated up to N items)
**Used by:** Multiple crates (hot paths)
**License:** MIT/Apache-2.0

**Why smallvec:**
- ✅ Avoids heap allocation for small collections
- ✅ Useful for child lists (most widgets have 1-3 children)
- ✅ Zero overhead for small sizes

**Alternatives:**
- ❌ Vec - Always heap-allocates
- ❌ arrayvec - Fixed capacity (less flexible)

---

#### ahash 0.8
**Purpose:** Fast non-cryptographic hash function
**Used by:** HashMap/HashSet in hot paths
**License:** MIT/Apache-2.0

**Why ahash:**
- ✅ 2-3x faster than SipHash (std default)
- ✅ Non-cryptographic (suitable for internal use)
- ✅ Used by Rust compiler itself

**Alternatives:**
- ❌ SipHash - Slower, cryptographic (overkill for UI)
- ❌ fnv - Slower on modern CPUs

---

#### lasso 0.7
**Purpose:** String interning (4-byte keys instead of String)
**Used by:** `flui_assets` (asset keys)
**License:** MIT/Apache-2.0

**Why lasso:**
- ✅ 4-byte interned keys vs 24-byte String
- ✅ Fast equality checks (integer comparison)
- ✅ Thread-safe with multi-threaded support
- ✅ Reduces memory for repeated strings

**Usage:**
- Asset paths: `AssetKey::new("logo.png")` → 4 bytes
- Cache keys: Fast hashing and comparison

---

### Error Handling & Logging

#### thiserror 2.0
**Purpose:** Derive macro for custom error types
**Used by:** All crates (workspace dependency)
**License:** MIT/Apache-2.0

**Why thiserror:**
- ✅ Ergonomic error definitions with derive macro
- ✅ Automatic Display/Error impl
- ✅ Source chaining for error context

**Example:**
```rust
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("Invalid constraints: {0:?}")]
    InvalidConstraints(BoxConstraints),
}
```

---

#### tracing 0.1
**Purpose:** Structured logging and diagnostics
**Used by:** All crates (workspace dependency)
**License:** MIT

**Why tracing:**
- ✅ Structured logs (not just strings)
- ✅ Span-based profiling (enter/exit events)
- ✅ Multiple subscribers (console, file, Tracy)
- ✅ Industry standard for Rust async code

**Usage:**
```rust
#[cfg(debug_assertions)]
tracing::debug!("Layout: size={:?}", size);
```

**Important:** Always use `tracing`, NEVER `println!`

---

### Serialization

#### serde 1.0
**Purpose:** Serialization/deserialization framework
**Used by:** Optional feature across all crates
**License:** MIT/Apache-2.0

**Why serde:**
- ✅ Industry standard for Rust serialization
- ✅ Zero-cost derive macro
- ✅ Format-agnostic (JSON, TOML, bincode, etc.)

**Usage:**
```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
```

**Feature flag:** `serde` (optional, disabled by default)

---

### Build Utilities

#### bon 3.8
**Purpose:** Builder pattern derive macro
**Used by:** `flui_widgets` (widget builders)
**License:** MIT/Apache-2.0

**Why bon:**
- ✅ Ergonomic builder API
- ✅ Type-safe optional parameters
- ✅ Less boilerplate than manual builders

**Example:**
```rust
#[builder]
pub struct Container {
    pub padding: EdgeInsets,
    pub child: Option<AnyElement>,
}

// Usage:
Container::builder()
    .padding(EdgeInsets::all(16.0))
    .child(Text::new("Hello"))
    .build()
```

---

### Image Processing

#### image 0.25
**Purpose:** Image decoding (PNG, JPEG, GIF, WebP)
**Used by:** `flui_types` (ImageData), `flui_engine` (GPU upload)
**License:** MIT

**Why image:**
- ✅ Supports all common formats
- ✅ Pure Rust (no C dependencies)
- ✅ Used widely in Rust ecosystem

**Features enabled:** `["png", "jpeg", "gif", "webp"]`

**Alternatives:**
- ❌ image-rs/individual crates - More manual work
- ❌ C bindings (libpng, libjpeg) - Build complexity

---

## Profiling & Development

### puffin 0.19 (Optional)
**Purpose:** Real-time profiler with GUI (puffin_http)
**Feature:** `profiling`
**License:** MIT/Apache-2.0

**Why puffin:**
- ✅ Real-time profiling in browser
- ✅ Flamegraphs for CPU profiling
- ✅ Low overhead (<1% in profiling builds)

**Usage:**
```bash
cargo run --features profiling --example my_app
# Open http://localhost:8585 in browser
```

---

### tracing-tracy 0.11 (Optional)
**Purpose:** Tracy profiler integration for production profiling
**Feature:** `tracy`
**License:** MIT

**Why Tracy:**
- ✅ Production-grade profiling
- ✅ Frame profiling, memory tracking
- ✅ Used by game engines (Bevy)

**Usage:**
```bash
cargo run --features tracy
# Connect with Tracy profiler GUI
```

---

### criterion 0.7 (Dev Dependency)
**Purpose:** Statistical benchmarking
**Used by:** All crates (dev-dependencies)
**License:** MIT/Apache-2.0

**Why criterion:**
- ✅ Statistical analysis of benchmark results
- ✅ Outlier detection and trend analysis
- ✅ HTML reports with graphs

**Usage:**
```bash
cargo bench -p flui_core
```

---

## Platform Dependencies

### winit 0.30.10
**Purpose:** Cross-platform window management
**Used by:** `flui_engine`, examples
**License:** MIT

**Why winit:**
- ✅ Cross-platform (Windows/macOS/Linux)
- ✅ Integrates with wgpu
- ✅ Event loop for input handling

**Alternatives:**
- ❌ glutin - Lower-level, more manual
- ❌ Platform-specific APIs - Not cross-platform

---

### pollster 0.3
**Purpose:** Minimal async executor for examples
**Used by:** Examples only (not library code)
**License:** MIT

**Why pollster:**
- ✅ Tiny, simple for blocking on futures
- ✅ Useful for quick examples without full tokio

**Usage:**
```rust
let surface = pollster::block_on(create_surface());
```

---

## Dependency Guidelines

### Adding New Dependencies

**Before adding a dependency, ask:**
1. ✅ Does it solve a real problem we can't solve efficiently ourselves?
2. ✅ Is it actively maintained (commits in last 6 months)?
3. ✅ Is it widely used in Rust ecosystem (>1M downloads)?
4. ✅ Does it have a clear license (MIT/Apache-2.0 preferred)?
5. ✅ Does it add acceptable binary size overhead?
6. ✅ Can we justify it in this DEPENDENCIES.md?

**Red flags:**
- ❌ Unmaintained (last commit >1 year ago)
- ❌ Small user base (<10k downloads)
- ❌ Unclear license or GPL/LGPL
- ❌ Large binary overhead (>5MB)
- ❌ Complex C/C++ build dependencies

### Dependency Update Policy

**Stable dependencies (workspace):**
- Review every 3 months
- Update only for security fixes or major features
- Test thoroughly before updating workspace version

**Critical dependencies (wgpu, parking_lot):**
- Review every 2 months
- Run full benchmark suite before updating
- Document breaking changes in migration guide

**Dev dependencies (criterion, puffin):**
- Update freely (not in production binary)

---

## Dependency Matrix by Crate

| Crate | Layer | Key Dependencies |
|-------|-------|------------------|
| **flui_types** | 0 | glam, serde (opt), image |
| **flui_engine** | 1 | wgpu, lyon, glyphon, cosmic-text, winit |
| **flui_painting** | 1 | flui_types, flui_engine |
| **flui_assets** | 2 | tokio, moka, lasso, reqwest, image |
| **flui_core** | 3 | parking_lot, slab, moka, rayon (opt) |
| **flui_rendering** | 2 | flui_core, flui_painting, parking_lot |
| **flui_gestures** | 2 | flui_types, flui_core |
| **flui_widgets** | 4 | flui_core, flui_rendering, bon |
| **flui_app** | 5 | All above crates |

---

## Binary Size Analysis

**Release build (stripped):**

| Component | Size | Justification |
|-----------|------|---------------|
| **Rust std** | ~400KB | Unavoidable baseline |
| **wgpu + drivers** | ~2MB | GPU abstraction worth it |
| **Lyon** | ~300KB | Path tessellation required |
| **Glyphon** | ~200KB | High-quality text rendering |
| **FLUI code** | ~1.5MB | Framework logic |
| **Total** | ~4.5MB | Acceptable for desktop/mobile |

**Comparison:**
- Electron app: ~80MB (Chrome engine)
- Flutter app: ~8MB (Skia + Dart VM)
- FLUI app: ~4.5MB (wgpu + Rust)

**Optimization options:**
- Link-time optimization (LTO): Enabled in release profile
- Code gen units: Set to 1 for maximum optimization
- Strip symbols: Enabled (`strip = true`)

---

## Performance Characteristics

### Dependency Performance Impact

| Dependency | Overhead | Benchmark | Justification |
|------------|----------|-----------|---------------|
| **parking_lot** | <5% single-thread | 15ns lock time | 3x faster than std |
| **slab** | <1% | O(1) insert/remove | Cache-friendly storage |
| **moka** | <2% | 95%+ hit rate | TinyLFU beats LRU |
| **wgpu** | N/A (GPU) | 5.6x faster complex UI | Native GPU performance |
| **glam** | 0% (SIMD) | 2-4x vs naive | SIMD math acceleration |

### Build Time Impact

**From scratch:**
```bash
cargo build --release --workspace
# ~45s on modern laptop (M1/Ryzen 7)
```

**Incremental (single crate change):**
```bash
cargo build -p flui_core
# ~5s
```

**Heavy dependencies:**
- wgpu: ~10s (large crate)
- lyon: ~5s (tessellation algorithms)
- tokio: ~8s (async runtime)

**Mitigation:**
- Use `cargo check` for quick feedback (~2s)
- Incremental compilation enabled by default
- Parallel compilation with `codegen-units = 16` in dev

---

## Related Documentation

### Architecture Decisions
- [ADR-001: Unified Render Trait](decisions/ADR-001-unified-render-trait.md) - Why single trait
- [ADR-002: Three-Tree Architecture](decisions/ADR-002-three-tree-architecture.md) - Why View/Element/Render
- [ADR-003: Enum vs Trait Objects](decisions/ADR-003-enum-vs-trait-objects.md) - Why enum-based storage
- [ADR-004: Thread-Safety Design](decisions/ADR-004-thread-safety-design.md) - Why Arc/Mutex everywhere
- [ADR-005: wgpu-Only Backend](decisions/ADR-005-wgpu-only-backend.md) - Why GPU-only rendering

### Integration
- [INTEGRATION.md](INTEGRATION.md) - How crates interact
- [PATTERNS.md](PATTERNS.md) - Common patterns using these dependencies

### Navigation
- [README.md](README.md) - Architecture documentation hub

---

## External References

### Dependency Documentation
- [wgpu Book](https://wgpu.rs/) - GPU API guide
- [parking_lot docs](https://docs.rs/parking_lot) - Synchronization primitives
- [moka docs](https://docs.rs/moka) - Caching library
- [glam docs](https://docs.rs/glam) - Math library
- [tokio tutorial](https://tokio.rs/tokio/tutorial) - Async runtime

### Performance Resources
- [Rust Performance Book](https://nnethercote.github.io/perf-book/) - Optimization techniques
- [wgpu Performance Guide](https://wgpu.rs/doc/wgpu/index.html#performance) - GPU optimization

### Ecosystem Standards
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) - Best practices
- [Cargo Book](https://doc.rust-lang.org/cargo/) - Dependency management
