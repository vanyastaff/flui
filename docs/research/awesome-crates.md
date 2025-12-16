# Awesome Crates for FLUI

Research from analyzing dependencies of iced, xilem, slint, egui, dioxus, leptos.

## Hidden Gems (not in awesome-rust lists)

### Animation & Interpolation

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **lilt** | Interruptable transition-based animations, zero deps | iced | HIGH |

```rust
// Example usage
let anim = Animated::new(0.0f32)
    .transition(Transition::ease_in_out(Duration::from_millis(300)));
anim.set(1.0); // smooth transition
```

### Memory & Data Structures

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **smol_str** | Small-string optimized, O(1) clone | iced, slint | HIGH |
| **nohash-hasher** | "Hasher" without hashing for int keys | egui | MEDIUM |
| **self_cell** | Self-referential structs without macros | egui | MEDIUM |
| **generational-box** | Box with generational runtime | dioxus | LOW |
| **tree_arena** | Arena-allocated tree | xilem | LOW |
| **anymap3** | Type-safe heterogeneous storage | xilem | LOW |
| **clru** | LRU cache with O(1) ops and weights | slint | HIGH |

```rust
// smol_str - inline storage for small strings
let s: SmolStr = "Button".into(); // no allocation
s.clone(); // O(1)

// clru - weighted LRU cache
let mut cache = CLruCache::new(NonZeroUsize::new(100).unwrap());
```

### Graphics & Rendering

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **guillotiere** | Dynamic 2D texture atlas allocator | iced | HIGH |
| **cryoglyph** | Fast 2D text rendering for wgpu (glyphon fork) | iced | MEDIUM |
| **half** | 16-bit floats (f16) | iced | LOW |

```rust
// guillotiere - texture atlas
let mut atlas = AtlasAllocator::new(size2(1024, 1024));
let alloc = atlas.allocate(size2(64, 64));
```

### Concurrency & Threading

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **send_wrapper** | Move non-Send types between threads | leptos, slint | MEDIUM |
| **guardian** | Owned mutex guards for Arc<Mutex> | leptos | LOW |
| **pin-weak** | Pin<Weak<T>> wrapper | slint | LOW |
| **async-lock** | Async-aware sync primitives | leptos | LOW |

### Input & Events

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **ui-events** | Cross-platform UI events abstraction | xilem | MEDIUM |
| **cursor-icon** | Cross-platform cursor types | xilem | MEDIUM |

### Utilities

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **scopeguard** | RAII scope guards | slint | LOW |
| **const-field-offset** | Compile-time field offsets | slint | LOW |
| **paste** | Macro token pasting | leptos | LOW |

---

## Well-Known Crates (from awesome-rust)

### Accessibility

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **accesskit** 0.21 | Cross-platform accessibility | xilem, egui | HIGH |
| **accesskit_winit** | winit integration | xilem, egui | HIGH |

### Layout

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **taffy** | Flexbox/Grid layout engine | many | EVALUATE |

### Collections

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **smallvec** | Stack-allocated Vec | xilem, egui | HIGH |
| **hashbrown** | Fast HashMap | xilem | MEDIUM |
| **indexmap** | Ordered HashMap | leptos | MEDIUM |
| **slotmap** | Generational arena | leptos | EVALUATE |

### Error Handling

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **thiserror** 2.x | Typed errors derive | iced, egui, leptos | HIGH |

### Hashing

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **rustc-hash** 2.x | Fast hasher | iced, leptos | HIGH |
| **ahash** | Fast hasher | egui | HIGH |

### Testing & Benchmarking

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **criterion** | Benchmarking | iced, egui | HIGH |
| **proptest** | Property-based testing | - | HIGH |
| **insta** | Snapshot testing | xilem | MEDIUM |

### Parallelism

| Crate | Description | Used by | Priority |
|-------|-------------|---------|----------|
| **rayon** | Data parallelism | slint, egui | MEDIUM |

---

## Framework Dependency Comparison

### Rendering Backend

| Crate | iced | xilem | slint | egui | dioxus |
|-------|------|-------|-------|------|--------|
| wgpu | 27.0 | - | 26-27 | 27.0 | 26.0 |
| vello | - | 0.6.0 | - | 0.0.4 | 0.6 |
| tiny-skia | 0.11 | - | - | - | - |
| glow | - | - | 0.16 | 0.16 | - |

### Text Rendering

| Crate | iced | xilem | slint | egui |
|-------|------|-------|-------|------|
| cosmic-text | git | - | - | - |
| parley | - | 0.7.0 | 0.7.0 | - |
| rustybuzz | - | - | 0.20 | - |
| skrifa | - | - | 0.37 | 0.37 |

### Windowing

| Crate | iced | xilem | slint | egui | dioxus |
|-------|------|-------|-------|------|--------|
| winit | git | 0.30.12 | - | 0.30.12 | 0.30.11 |

---

## Recommendations for FLUI

### Immediate (HIGH priority)

1. **lilt** - animations (zero deps, perfect for UI)
2. **guillotiere** - texture atlas for glyphs/icons
3. **smol_str** - for widget text (labels, buttons)
4. **clru** - LRU cache for layout/glyph caching
5. **accesskit** - accessibility (required for production)
6. **smallvec** - for children lists
7. **thiserror** - typed errors for public API
8. **rustc-hash** or **ahash** - faster hasher
9. **criterion** - benchmarking

### Evaluate

- **taffy** - consider for layout engine (Flexbox/Grid)
- **ui-events** - standardized UI events
- **slotmap** - alternative to slab (generational)

### For Reactivity Improvements

- **generational-box** - like dioxus signals
- **guardian** - owned mutex guards
- **async-lock** - async Mutex/RwLock

---

## Current FLUI vs Recommendations

| Area | FLUI now | Recommendation |
|------|----------|----------------|
| Tree storage | slab | keep slab or + tree_arena |
| String | String | + smol_str for labels |
| Animation | ? | + **lilt** |
| Texture atlas | ? | + **guillotiere** |
| Glyph cache | ? | + **clru** |
| HashMap hasher | std | + nohash-hasher/rustc-hash |
| Events | custom | evaluate ui-events |
| Accessibility | - | + **accesskit** |

---

## Links

- [awesome-rust (unofficial)](https://github.com/rust-unofficial/awesome-rust)
- [awesome-rust-com](https://github.com/awesome-rust-com/awesome-rust)
- [iced](https://github.com/iced-rs/iced)
- [xilem](https://github.com/linebender/xilem)
- [slint](https://github.com/slint-ui/slint)
- [egui](https://github.com/emilk/egui)
- [dioxus](https://github.com/DioxusLabs/dioxus)
- [leptos](https://github.com/leptos-rs/leptos)
