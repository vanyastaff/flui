# Design Patterns in flui_assets

This document explains the design patterns used in `flui_assets` and their rationale.

## 1. Extension Trait Pattern

### Problem
You want to add convenience methods without bloating the core API or breaking compatibility.

### Solution
Use sealed core traits with blanket extension trait implementations.

### Example

```rust
// Step 1: Create sealed trait module
mod sealed {
    pub trait Sealed {}
    impl<T, K> Sealed for AssetHandle<T, K> {}
}

// Step 2: Core trait (minimal, stable API)
pub trait AssetHandleCore<T, K>: sealed::Sealed {
    fn get(&self) -> &T;
    fn key(&self) -> &K;
    fn strong_count(&self) -> usize;
    fn weak_count(&self) -> usize;
}

// Step 3: Extension trait (convenience methods)
pub trait AssetHandleExt<T, K>: AssetHandleCore<T, K> {
    fn is_unique(&self) -> bool {
        self.strong_count() == 1
    }

    fn has_weak_refs(&self) -> bool {
        self.weak_count() > 0
    }

    fn map<U, F>(&self, f: F) -> U
    where F: FnOnce(&T) -> U
    {
        f(self.get())
    }

    fn total_ref_count(&self) -> usize {
        self.strong_count() + self.weak_count()
    }
}

// Step 4: Blanket implementation
impl<H, T, K> AssetHandleExt<T, K> for H
where H: AssetHandleCore<T, K> + ?Sized
{}
```

### Benefits
- ✅ **Backward compatible** - Can add new methods to extension trait
- ✅ **Clean separation** - Core API stays minimal and stable
- ✅ **Zero cost** - Extension methods inline completely
- ✅ **Sealed** - Users can't implement core trait, preventing breakage

### When to Use
- Adding convenience methods to existing types
- Building on a stable core API
- Library development where API stability matters

### Implementation in flui_assets
- `AssetHandleCore` + `AssetHandleExt` (6 convenience methods)
- `AssetCacheCore` + `AssetCacheExt` (5 convenience methods)

## 2. Type State Builder Pattern

### Problem
Want compile-time validation that builder configuration is valid.

### Solution
Use marker types to represent builder states, making `build()` only available in valid states.

### Example

```rust
// Step 1: Define state markers
#[derive(Debug, Clone, Copy)]
pub struct NoCapacity;

#[derive(Debug, Clone, Copy)]
pub struct HasCapacity(pub(crate) usize);

// Step 2: Generic builder with state parameter
pub struct AssetRegistryBuilder<C = NoCapacity> {
    capacity: C,
}

// Step 3: Initial state methods
impl AssetRegistryBuilder<NoCapacity> {
    pub fn new() -> Self {
        Self { capacity: NoCapacity }
    }

    // Transition to HasCapacity state
    pub fn with_capacity(self, capacity: usize) -> AssetRegistryBuilder<HasCapacity> {
        assert!(capacity > 0, "Capacity must be greater than 0");
        AssetRegistryBuilder {
            capacity: HasCapacity(capacity),
        }
    }
}

// Step 4: Final state methods
impl AssetRegistryBuilder<HasCapacity> {
    // build() only available in HasCapacity state
    pub fn build(self) -> AssetRegistry {
        AssetRegistry::new(self.capacity.0)
    }
}
```

### Usage

```rust
// ✅ This compiles
let registry = AssetRegistryBuilder::new()
    .with_capacity(1024)
    .build();

// ❌ This doesn't compile - no build() method on NoCapacity
let registry = AssetRegistryBuilder::new()
    .build(); // ERROR: no method `build` found
```

### Benefits
- ✅ **Compile-time safety** - Invalid states cannot compile
- ✅ **Clear API progression** - Type system guides usage
- ✅ **Zero runtime overhead** - States are marker types
- ✅ **Self-documenting** - Type signatures show requirements

### When to Use
- Builders with required configuration
- APIs with sequential steps
- Preventing misuse at compile time

## 3. Sealed Trait Pattern

### Problem
You want to provide a trait for users to use, but not implement.

### Solution
Use a private `Sealed` super-trait that users cannot implement.

### Example

```rust
// Private sealed trait
mod sealed {
    pub trait Sealed {}

    // Only implement for types in your crate
    impl Sealed for OurType1 {}
    impl Sealed for OurType2 {}
}

// Public trait with sealed super-trait
pub trait PublicTrait: sealed::Sealed {
    fn method(&self);
}

// Users can use the trait
fn use_trait<T: PublicTrait>(value: &T) {
    value.method();
}

// But cannot implement it
// impl PublicTrait for MyType {} // ERROR: sealed::Sealed is private
```

### Benefits
- ✅ **API evolution** - Can add trait methods without breaking users
- ✅ **Internal guarantees** - Only your types implement the trait
- ✅ **Clear intent** - Users know they shouldn't implement it

### When to Use
- Extension trait core traits
- Traits that may evolve over time
- Internal traits with public visibility

### Implementation in flui_assets
- `AssetHandleCore` is sealed
- `AssetCacheCore` is sealed

## 4. Smart Handle Pattern (Arc + Key)

### Problem
Need efficient shared ownership of cached data with identity.

### Solution
Combine `Arc<T>` for sharing with a key for identity and cache operations.

### Example

```rust
pub struct AssetHandle<T, K> {
    data: Arc<T>,   // Shared ownership
    key: K,         // Identity
}

impl<T, K> AssetHandle<T, K> {
    pub fn new(data: Arc<T>, key: K) -> Self {
        Self { data, key }
    }

    pub fn get(&self) -> &T {
        &self.data
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    // Clone is cheap - just clones Arc
    pub fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            key: self.key.clone(),
        }
    }

    // Create weak reference
    pub fn downgrade(&self) -> WeakAssetHandle<T, K> {
        WeakAssetHandle {
            data: Arc::downgrade(&self.data),
            key: self.key.clone(),
        }
    }
}
```

### Benefits
- ✅ **Efficient cloning** - O(1) atomic increment
- ✅ **Cache-aware** - Key enables cache invalidation
- ✅ **Memory management** - Arc handles cleanup
- ✅ **Weak references** - Prevent cache bloat

### When to Use
- Cached resources with identity
- Shared ownership with many clones
- Cache invalidation needs

## 5. String Interning Pattern

### Problem
String keys waste memory and are slow to compare/hash.

### Solution
Intern strings to unique integers, storing string once.

### Example

```rust
use lasso::{Rodeo, Spur};
use parking_lot::RwLock;
use once_cell::sync::Lazy;

// Global interner
static INTERNER: Lazy<RwLock<Rodeo>> = Lazy::new(|| {
    RwLock::new(Rodeo::new())
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetKey(Spur); // 4 bytes

impl AssetKey {
    pub fn new(s: &str) -> Self {
        let mut interner = INTERNER.write();
        Self(interner.get_or_intern(s))
    }

    pub fn as_str(&self) -> String {
        let interner = INTERNER.read();
        interner.resolve(&self.0).to_string()
    }
}

impl Hash for AssetKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.into_inner().get().hash(state); // Hash u32
    }
}
```

### Performance Impact

```rust
// Without interning
let key1 = "texture.png".to_string(); // 24+ bytes, heap allocation
let key2 = "texture.png".to_string(); // 24+ bytes, heap allocation
key1 == key2; // O(n) string comparison

// With interning
let key1 = AssetKey::new("texture.png"); // 4 bytes, stack
let key2 = AssetKey::new("texture.png"); // 4 bytes, stack
key1 == key2; // O(1) u32 comparison
```

### Benefits
- ✅ **6x memory reduction** - 4 bytes vs 24+ bytes
- ✅ **10x faster comparison** - Single u32 comparison
- ✅ **2-3x faster hashing** - Hash u32 instead of string
- ✅ **Cache-friendly** - Keys fit in CPU cache lines

### Trade-offs
- ⚠️ Global state (interner)
- ⚠️ ~100ns overhead on first use
- ⚠️ Strings never deallocated (acceptable for finite key space)

### When to Use
- Identifiers with many duplicates
- Frequent comparison/hashing operations
- Limited key space (thousands, not millions)

## 6. Type Erasure with TypeId

### Problem
Need to store different cache types in a single collection.

### Solution
Use `TypeId` as key with `Box<dyn Any>` as value.

### Example

```rust
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub struct AssetRegistry {
    caches: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl AssetRegistry {
    pub fn get_cache<T: Asset>(&self) -> Option<Arc<AssetCache<T>>> {
        let caches = self.caches.read();
        let type_id = TypeId::of::<T>();

        caches.get(&type_id)
            .and_then(|cache| cache.downcast_ref::<AssetCache<T>>())
            .map(|cache| cache.clone())
    }

    pub fn create_cache<T: Asset>(&self) {
        let mut caches = self.caches.write();
        let type_id = TypeId::of::<T>();

        caches.insert(
            type_id,
            Box::new(AssetCache::<T>::new(self.default_capacity)),
        );
    }
}
```

### Benefits
- ✅ **Type-safe** - Downcasting is checked
- ✅ **Zero-cost abstraction** - TypeId is compile-time
- ✅ **Flexible** - Can store any type
- ✅ **Thread-safe** - Works with Send + Sync

### Trade-offs
- ⚠️ Runtime downcasting cost (but amortized)
- ⚠️ Less obvious than enums
- ⚠️ Requires `'static` types

### When to Use
- Heterogeneous collections of typed data
- Plugin systems
- Dynamic type dispatch

## 7. Extension Method Pattern (Convenience)

### Problem
Want to provide helper methods without modifying original type.

### Solution
Implement methods on existing types via extension traits.

### Example

```rust
pub trait AssetCacheExt<T: Asset>: AssetCacheCore<T> {
    /// Get cache hit rate (0.0 - 1.0)
    fn hit_rate(&self) -> f64 {
        let stats = self.stats();
        stats.hit_rate()
    }

    /// Get cache miss rate (0.0 - 1.0)
    fn miss_rate(&self) -> f64 {
        1.0 - self.hit_rate()
    }

    /// Check if cache is efficient (>70% hit rate)
    fn is_efficient(&self) -> bool {
        self.hit_rate() > 0.7
    }

    /// Async check if key exists
    fn contains(&self, key: &T::Key) -> impl Future<Output = bool> + Send
    where Self: Sync
    {
        async move {
            self.get(key).await.is_some()
        }
    }
}
```

### Benefits
- ✅ **Non-invasive** - Doesn't modify original type
- ✅ **Composable** - Can build on other traits
- ✅ **Default implementations** - Users get them for free

### When to Use
- Adding utility methods to library types
- Building higher-level APIs
- Providing optional functionality

## Pattern Combinations

### Extension Trait + Sealed + Type State

The three patterns work together in `AssetRegistryBuilder`:

```rust
// 1. Sealed trait (prevents external implementation)
mod sealed {
    pub trait Sealed {}
    impl Sealed for NoCapacity {}
    impl Sealed for HasCapacity {}
}

// 2. Type state markers
pub struct NoCapacity;
pub struct HasCapacity(usize);

// 3. Core trait with type state
pub trait BuilderCore<C>: sealed::Sealed {
    fn capacity(&self) -> Option<usize>;
}

// 4. Extension trait with convenience methods
pub trait BuilderExt<C>: BuilderCore<C> {
    fn is_configured(&self) -> bool {
        self.capacity().is_some()
    }
}

// 5. Blanket implementation
impl<B, C> BuilderExt<C> for B where B: BuilderCore<C> {}
```

## Anti-Patterns to Avoid

### ❌ Overly Generic APIs

```rust
// BAD: Too generic, hard to understand
pub trait GenericAsset<T, K, E, M> {
    fn load(&self) -> Result<T, E>;
}

// GOOD: Clear associated types
pub trait Asset {
    type Data;
    type Key;
    type Error;

    fn load(&self) -> Result<Self::Data, Self::Error>;
}
```

### ❌ Leaky Abstractions

```rust
// BAD: Exposes internal cache implementation
pub fn get_moka_cache<T: Asset>(&self) -> &Cache<T::Key, T::Data>;

// GOOD: Abstract cache operations
pub async fn get<T: Asset>(&self, key: &T::Key) -> Option<Arc<T::Data>>;
```

### ❌ Unnecessary Cloning

```rust
// BAD: Clones heavy data
pub fn get_data(&self) -> T {
    self.data.clone()
}

// GOOD: Returns reference or Arc
pub fn get_data(&self) -> &T {
    &self.data
}
```

## Summary

| Pattern | Use Case | Benefit |
|---------|----------|---------|
| Extension Trait | Add methods | Backward compatibility |
| Type State | Compile-time validation | Safety |
| Sealed Trait | Prevent implementation | API evolution |
| Smart Handle | Shared ownership | Efficiency |
| String Interning | Efficient keys | Performance |
| Type Erasure | Heterogeneous storage | Flexibility |

These patterns combine to create a high-performance, type-safe, and maintainable asset system.
