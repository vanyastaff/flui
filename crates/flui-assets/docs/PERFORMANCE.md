# Performance Guide

This document explains the performance characteristics of `flui_assets` and how to optimize for your use case.

## Performance Overview

### Memory Efficiency

| Component | Size | Notes |
|-----------|------|-------|
| `AssetKey` | **4 bytes** | String interning via lasso |
| `AssetHandle<T, K>` | **12 bytes** | Arc (8) + Key (4) |
| Cache entry | **12 bytes + data** | Key + Arc pointer + actual data |
| `AssetRegistry` | **8 bytes + caches** | Arc to HashMap |

**Example**: Loading 1000 font files
```
Without flui_assets:
- 1000 × String (24 bytes) = 24KB in keys
- 1000 × Arc<FontData> (8 bytes) = 8KB in pointers
- Total overhead: ~32KB

With flui_assets:
- 1000 × AssetKey (4 bytes) = 4KB in keys
- 1000 × Arc<FontData> (8 bytes) = 8KB in pointers
- Total overhead: ~12KB (2.7x reduction)
```

### Time Complexity

| Operation | Complexity | Actual Time | Notes |
|-----------|-----------|-------------|-------|
| `load()` | O(1) + I/O | ~50-100μs + I/O | Cache miss |
| `get()` | O(1) | ~30ns | Cache hit |
| `insert()` | O(1) amortized | ~50ns | Lock-free |
| `evict()` | O(1) amortized | ~40ns | TinyLFU decision |
| Key creation | O(1) amortized | ~100ns | First intern slower |

**Benchmarks on M1 MacBook Pro (2021)**:
```
test cache_insert     ... bench:      48 ns/iter
test cache_get_hit    ... bench:      29 ns/iter
test cache_get_miss   ... bench:      42 ns/iter
test key_creation     ... bench:      97 ns/iter
test key_comparison   ... bench:       2 ns/iter
```

## Cache Performance

### TinyLFU Algorithm

flui_assets uses **TinyLFU** (Tiny Least Frequently Used) for cache eviction, which provides better hit rates than traditional LRU.

**How it works**:
1. **Frequency sketch**: Count-Min Sketch tracks access frequency
2. **Admission policy**: New items must be accessed more frequently than victim
3. **Recency component**: Recent items get slight boost

**Performance comparison** (typical workload):
```
LRU hit rate:      72%
LFU hit rate:      75%
TinyLFU hit rate:  82%  ← 10% improvement over LRU
```

**Why TinyLFU is better**:
- Resistant to cache pollution from one-time scans
- Balances frequency and recency
- O(1) admission decision (no sorting)

### Cache Configuration

```rust
use flui_assets::AssetRegistryBuilder;

// Small cache for mobile (50MB)
let registry = AssetRegistryBuilder::new()
    .with_capacity(50 * 1024 * 1024)
    .build();

// Large cache for desktop (500MB)
let registry = AssetRegistryBuilder::new()
    .with_capacity(500 * 1024 * 1024)
    .build();

// Unlimited cache (testing only)
let registry = AssetRegistryBuilder::new()
    .with_capacity(usize::MAX)
    .build();
```

**Capacity guidelines**:
- **Mobile**: 50-100 MB
- **Desktop**: 200-500 MB
- **Server**: 1-2 GB

### Cache Statistics

Monitor cache performance in production:

```rust
use flui_assets::{AssetCache, AssetCacheExt, FontAsset};

let cache: AssetCache<FontAsset> = registry.get_cache().unwrap();

// Get statistics
println!("Hit rate: {:.1}%", cache.hit_rate() * 100.0);
println!("Miss rate: {:.1}%", cache.miss_rate() * 100.0);
println!("Utilization: {:.1}%", cache.utilization() * 100.0);

// Check efficiency
if !cache.is_efficient() {
    println!("Warning: Cache hit rate below 70%");
    println!("Consider increasing cache size");
}
```

**Target metrics**:
- Hit rate: **> 70%** (good), **> 85%** (excellent)
- Utilization: **> 60%** (not wasting memory)
- Miss rate: **< 30%**

## String Interning Performance

### Why Interning Matters

String interning provides significant performance benefits for asset keys:

**Without interning**:
```rust
let key1 = "textures/grass.png".to_string();
let key2 = "textures/grass.png".to_string();

// Comparison: O(n) - must compare each character
key1 == key2; // ~20-30ns for short strings

// Hashing: O(n) - must hash entire string
let hash = calculate_hash(&key1); // ~40-60ns
```

**With interning**:
```rust
let key1 = AssetKey::new("textures/grass.png");
let key2 = AssetKey::new("textures/grass.png");

// Comparison: O(1) - single u32 comparison
key1 == key2; // ~2ns (10x faster)

// Hashing: O(1) - hash single u32
let hash = calculate_hash(&key1); // ~8ns (5x faster)
```

### Interning Overhead

**First use** (cold):
```rust
let key = AssetKey::new("new_texture.png"); // ~100ns
```

**Subsequent uses** (hot):
```rust
let key = AssetKey::new("new_texture.png"); // ~15ns
```

**Trade-off**:
- ✅ Much faster lookups (10x)
- ✅ Smaller memory footprint (6x)
- ⚠️ One-time interning cost (~100ns)
- ⚠️ Strings never deallocated

**Is it worth it?**

Yes, if:
- Same keys accessed multiple times (typical for assets)
- Large number of unique keys (thousands)
- HashMap lookups are hot path

No, if:
- Keys used exactly once
- Very few unique keys (< 100)

## Async Performance

### Non-Blocking I/O

All I/O operations are async to avoid blocking:

```rust
// ❌ BAD: Blocking I/O
let bytes = std::fs::read("texture.png")?; // Blocks thread

// ✅ GOOD: Async I/O
let bytes = tokio::fs::read("texture.png").await?; // Non-blocking
```

**Impact**:
- Without async: 1 thread per concurrent load
- With async: Thousands of concurrent loads on few threads

### Parallel Loading

Load multiple assets concurrently:

```rust
use futures::future::join_all;

let registry = AssetRegistry::global();

// Load 100 fonts concurrently
let handles = (0..100)
    .map(|i| {
        let registry = registry.clone();
        async move {
            registry.load(FontAsset::file(&format!("font{}.ttf", i))).await
        }
    })
    .collect::<Vec<_>>();

// Wait for all to complete
let results = join_all(handles).await;
```

**Performance**:
- Sequential: 100 × 10ms = 1000ms
- Parallel (10 threads): ~100-200ms (5-10x faster)

## Memory Management

### Weak References

Use weak references to avoid keeping assets alive unnecessarily:

```rust
use flui_assets::{AssetHandle, WeakAssetHandle};

// Strong reference keeps asset in memory
let handle: AssetHandle<FontData, AssetKey> = registry.load(font).await?;

// Convert to weak reference
let weak: WeakAssetHandle<_, _> = handle.downgrade();
drop(handle); // Asset can be evicted now

// Later, try to upgrade
if let Some(strong) = weak.upgrade() {
    // Asset still in cache
    use_font(&strong);
} else {
    // Asset was evicted, need to reload
    let strong = registry.load(font).await?;
}
```

**Pattern**: Store weak references in long-lived structures, upgrade when needed.

### Cache Size Tuning

**Too small cache**:
- High miss rate
- Frequent reloading
- Wasted I/O bandwidth

**Too large cache**:
- Wasted memory
- Slower GC (if applicable)
- May not fit in RAM

**Finding optimal size**:
1. Start with conservative estimate (100 MB)
2. Monitor hit rate in production
3. Increase if hit rate < 70%
4. Decrease if memory pressure

**Rule of thumb**:
```
Cache size = Working set × 1.5

Where working set = typical assets used in 5-minute period
```

## Optimization Techniques

### 1. Preloading

Load assets before they're needed:

```rust
// Preload critical assets at startup
let registry = AssetRegistry::global();

let assets = vec![
    FontAsset::file("ui_font.ttf"),
    ImageAsset::file("logo.png"),
    ImageAsset::file("background.jpg"),
];

for asset in assets {
    registry.preload(asset).await?;
}
```

**Benefit**: Assets available immediately when needed (zero latency).

### 2. Batch Loading

Load multiple assets with shared setup:

```rust
use flui_assets::loaders::BytesFileLoader;

let loader = BytesFileLoader::new("assets");

// Load multiple files with single loader instance
let handles = join_all(
    paths.iter().map(|path| loader.load_bytes(path))
).await;
```

**Benefit**: Amortize loader initialization cost.

### 3. Custom Cache per Asset Type

Different asset types may need different cache sizes:

```rust
// Large cache for images (200 MB)
let image_cache = AssetCache::<ImageAsset>::new(200 * 1024 * 1024);

// Small cache for configs (1 MB)
let config_cache = AssetCache::<ConfigAsset>::new(1024 * 1024);
```

### 4. Lazy Loading

Only load assets when actually used:

```rust
// ❌ BAD: Eager loading
let all_textures = load_all_textures().await?;

// ✅ GOOD: Lazy loading
let texture_keys = get_texture_keys();
// Load on-demand when rendering
```

## Benchmarking

### Using hyperfine

```bash
# Benchmark cold start (no cache)
hyperfine --warmup 0 --runs 10 \
  "cargo run --release -- load-assets"

# Benchmark hot start (cache populated)
hyperfine --warmup 5 --runs 20 \
  "cargo run --release -- load-assets"
```

### Using criterion

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_assets::{AssetRegistry, FontAsset};

fn bench_load(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let registry = AssetRegistry::global();

    c.bench_function("load_font", |b| {
        b.to_async(&runtime).iter(|| async {
            let font = FontAsset::file("font.ttf");
            black_box(registry.load(font).await)
        });
    });
}

criterion_group!(benches, bench_load);
criterion_main!(benches);
```

## Performance Checklist

### Development
- [ ] Profile hot paths with `cargo flamegraph`
- [ ] Check cache hit rates in tests
- [ ] Measure asset load times
- [ ] Monitor memory usage

### Production
- [ ] Set appropriate cache size for target hardware
- [ ] Monitor cache statistics
- [ ] Use weak references for long-lived structures
- [ ] Preload critical assets at startup
- [ ] Profile with production data

### Optimization
- [ ] Use batch loading where possible
- [ ] Implement lazy loading for rarely-used assets
- [ ] Consider custom caches per asset type
- [ ] Use async I/O throughout
- [ ] Profile and eliminate blocking operations

## Common Performance Issues

### Issue 1: Low Cache Hit Rate

**Symptoms**:
- High I/O wait times
- Frequent asset reloading
- Hit rate < 70%

**Solutions**:
1. Increase cache size
2. Preload frequently-used assets
3. Use weak references to avoid premature eviction
4. Profile access patterns

### Issue 2: Memory Pressure

**Symptoms**:
- High memory usage
- OOM crashes on low-end devices
- Slow GC pauses

**Solutions**:
1. Decrease cache size
2. Use streaming for large assets
3. Implement LRU eviction for specific types
4. Profile memory usage by asset type

### Issue 3: Slow Startup

**Symptoms**:
- Long initial load time
- UI frozen during startup
- Users waiting for assets

**Solutions**:
1. Lazy load non-critical assets
2. Show loading screen with progress
3. Preload in background thread
4. Cache assets to disk (future feature)

### Issue 4: Thread Contention

**Symptoms**:
- High CPU usage
- Lock contention in profiler
- Slower than expected performance

**Solutions**:
1. Use more granular caches (per asset type)
2. Reduce synchronous operations
3. Profile with `cargo flamegraph`
4. Consider sharding cache by key hash

## Future Optimizations

### Planned Features

1. **Memory-mapped fonts** (`mmap-fonts` feature)
   - Zero-copy loading
   - Shared across processes
   - Estimated 30-50% memory reduction

2. **Parallel decoding** (`parallel-decode` feature)
   - Decode images/videos in parallel
   - Use rayon thread pool
   - Estimated 2-4x faster loading

3. **Persistent cache**
   - Cache to disk between runs
   - Instant startup
   - Reduce repeated I/O

4. **Streaming assets**
   - Load large assets in chunks
   - Reduce memory spikes
   - Better for low-memory devices

## References

- [TinyLFU Paper](https://arxiv.org/abs/1512.00727)
- [Moka Performance](https://github.com/moka-rs/moka#performance)
- [String Interning Performance](https://matklad.github.io/2020/03/22/fast-simple-rust-interner.html)
- [Async Performance in Rust](https://rust-lang.github.io/async-book/04_pinning/01_chapter.html)
