# Flui - Next Steps (Phase 1)

> Detailed action plan for starting Phase 1: Foundation Layer

## 🎯 Immediate Actions (This Week)

### 1. Create Foundation Crate Structure

```bash
# Create flui_foundation crate
mkdir -p crates/flui_foundation/src
cd crates/flui_foundation

# Create Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "flui_foundation"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
parking_lot.workspace = true
once_cell.workspace = true
serde.workspace = true
thiserror.workspace = true
EOF

# Create lib.rs
cat > src/lib.rs << 'EOF'
//! Foundation layer for Flui framework
//!
//! This crate provides core utilities and types that are used throughout
//! the Flui framework, including keys, change notification, diagnostics,
//! and platform detection.

pub mod key;
pub mod change_notifier;
pub mod observer_list;
pub mod diagnostics;
pub mod platform;

// Re-exports
pub use key::{Key, ValueKey, UniqueKey, GlobalKey};
pub use change_notifier::{ChangeNotifier, ValueNotifier};
pub use observer_list::ObserverList;

// Type aliases
pub type VoidCallback = Box<dyn Fn() + Send + Sync>;
pub type ListenerId = u64;
EOF
```

### 2. Implement Key System (`key.rs`)

**Priority: CRITICAL** | **Time: 1-2 days**

```rust
// crates/flui_foundation/src/key.rs

use std::any::Any;
use std::fmt::Debug;
use std::hash::{Hash, Hasher, DefaultHasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Trait for widget identity keys
pub trait Key: Any + Debug + Send + Sync {
    /// Check if two keys are equal
    fn equals(&self, other: &dyn Key) -> bool;

    /// Get hash code for this key
    fn hash_code(&self) -> u64;

    /// Cast to Any for downcasting
    fn as_any(&self) -> &dyn Any;
}

/// LocalKey - key scoped to parent
pub trait LocalKey: Key {}

/// ValueKey - identified by value
#[derive(Debug, Clone)]
pub struct ValueKey<T: Hash + Eq + Clone + Send + Sync + 'static> {
    value: T,
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> ValueKey<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> Key for ValueKey<T> {
    fn equals(&self, other: &dyn Key) -> bool {
        other.as_any()
            .downcast_ref::<Self>()
            .map(|other| self.value == other.value)
            .unwrap_or(false)
    }

    fn hash_code(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// UniqueKey - always unique
#[derive(Debug, Clone, Copy)]
pub struct UniqueKey {
    id: u64,
}

impl UniqueKey {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self {
            id: COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl Key for UniqueKey {
    fn equals(&self, other: &dyn Key) -> bool {
        other.as_any()
            .downcast_ref::<Self>()
            .map(|other| self.id == other.id)
            .unwrap_or(false)
    }

    fn hash_code(&self) -> u64 {
        self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// GlobalKey - can access state from anywhere
pub struct GlobalKey<T: 'static> {
    id: GlobalKeyId,
    _phantom: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalKeyId(u64);

impl GlobalKeyId {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl<T: 'static> GlobalKey<T> {
    pub fn new() -> Self {
        Self {
            id: GlobalKeyId::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn id(&self) -> GlobalKeyId {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_key_equality() {
        let key1 = ValueKey::new(42);
        let key2 = ValueKey::new(42);
        assert!(key1.equals(&key2 as &dyn Key));
    }

    #[test]
    fn value_key_inequality() {
        let key1 = ValueKey::new(42);
        let key2 = ValueKey::new(43);
        assert!(!key1.equals(&key2 as &dyn Key));
    }

    #[test]
    fn unique_key_uniqueness() {
        let key1 = UniqueKey::new();
        let key2 = UniqueKey::new();
        assert!(!key1.equals(&key2 as &dyn Key));
    }

    #[test]
    fn value_key_hash_consistent() {
        let key = ValueKey::new("test");
        let hash1 = key.hash_code();
        let hash2 = key.hash_code();
        assert_eq!(hash1, hash2);
    }
}
```

**Tests to Write:**
- [x] ValueKey equality with same values
- [x] ValueKey inequality with different values
- [x] UniqueKey uniqueness
- [x] Hash code consistency
- [ ] GlobalKey ID generation
- [ ] Type-safe downcasting

---

### 3. Implement ChangeNotifier (`change_notifier.rs`)

**Priority: CRITICAL** | **Time: 1 day**

```rust
// crates/flui_foundation/src/change_notifier.rs

use crate::{ObserverList, VoidCallback, ListenerId};
use parking_lot::RwLock;

/// Trait for observable objects
pub trait Listenable {
    fn add_listener(&mut self, listener: VoidCallback) -> ListenerId;
    fn remove_listener(&mut self, id: ListenerId);
    fn notify_listeners(&self);
}

/// Base class for objects that notify listeners
pub struct ChangeNotifier {
    listeners: ObserverList<VoidCallback>,
    disposed: bool,
}

impl ChangeNotifier {
    pub fn new() -> Self {
        Self {
            listeners: ObserverList::new(),
            disposed: false,
        }
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    pub fn dispose(&mut self) {
        assert!(!self.disposed, "ChangeNotifier disposed twice");
        self.disposed = true;
        self.listeners = ObserverList::new();
    }

    fn assert_not_disposed(&self) {
        assert!(!self.disposed, "ChangeNotifier was used after being disposed");
    }
}

impl Listenable for ChangeNotifier {
    fn add_listener(&mut self, listener: VoidCallback) -> ListenerId {
        self.assert_not_disposed();
        self.listeners.add(listener)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        self.listeners.remove(id);
    }

    fn notify_listeners(&self) {
        self.assert_not_disposed();
        for listener in self.listeners.iter() {
            listener();
        }
    }
}

/// ValueNotifier - notifies when value changes
pub struct ValueNotifier<T: Clone> {
    value: T,
    notifier: ChangeNotifier,
}

impl<T: Clone> ValueNotifier<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            notifier: ChangeNotifier::new(),
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn set_value(&mut self, value: T) {
        self.value = value;
        self.notifier.notify_listeners();
    }
}

impl<T: Clone> Listenable for ValueNotifier<T> {
    fn add_listener(&mut self, listener: VoidCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn notify_listeners(&self) {
        self.notifier.notify_listeners();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[test]
    fn change_notifier_basic() {
        let mut notifier = ChangeNotifier::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        notifier.add_listener(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        }));

        notifier.notify_listeners();
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn value_notifier_updates() {
        let mut notifier = ValueNotifier::new(42);
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        notifier.add_listener(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        }));

        notifier.set_value(43);
        assert!(called.load(Ordering::SeqCst));
        assert_eq!(*notifier.value(), 43);
    }
}
```

---

### 4. Implement ObserverList (`observer_list.rs`)

**Priority: CRITICAL** | **Time: 0.5 days**

```rust
// crates/flui_foundation/src/observer_list.rs

use crate::ListenerId;
use std::sync::atomic::{AtomicU64, Ordering};

/// List of observers with stable IDs
pub struct ObserverList<T> {
    observers: Vec<(ListenerId, T)>,
}

impl<T> ObserverList<T> {
    pub fn new() -> Self {
        Self {
            observers: Vec::new(),
        }
    }

    pub fn add(&mut self, observer: T) -> ListenerId {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        self.observers.push((id, observer));
        id
    }

    pub fn remove(&mut self, id: ListenerId) {
        self.observers.retain(|(observer_id, _)| *observer_id != id);
    }

    pub fn is_empty(&self) -> bool {
        self.observers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.observers.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.observers.iter().map(|(_, observer)| observer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_list_add_remove() {
        let mut list = ObserverList::new();
        let id1 = list.add(42);
        let id2 = list.add(43);

        assert_eq!(list.len(), 2);

        list.remove(id1);
        assert_eq!(list.len(), 1);

        let values: Vec<_> = list.iter().copied().collect();
        assert_eq!(values, vec![43]);
    }
}
```

---

### 5. Build & Test Foundation

```bash
# Build
cargo build -p flui_foundation

# Test
cargo test -p flui_foundation

# Check for warnings
cargo clippy -p flui_foundation -- -D warnings

# Format
cargo fmt -p flui_foundation

# Documentation
cargo doc -p flui_foundation --open
```

---

## 📊 Week 1 Progress Tracker

### Day 1-2: Key System
- [ ] Create `key.rs`
- [ ] Implement `Key` trait
- [ ] Implement `ValueKey<T>`
- [ ] Implement `UniqueKey`
- [ ] Implement `GlobalKey<T>`
- [ ] Write tests (6+ tests)
- [ ] Document APIs

### Day 3: ChangeNotifier
- [ ] Create `change_notifier.rs`
- [ ] Implement `Listenable` trait
- [ ] Implement `ChangeNotifier`
- [ ] Implement `ValueNotifier<T>`
- [ ] Write tests (4+ tests)
- [ ] Document APIs

### Day 4: ObserverList & Platform
- [ ] Create `observer_list.rs`
- [ ] Implement `ObserverList<T>`
- [ ] Create `platform.rs`
- [ ] Implement platform detection
- [ ] Write tests
- [ ] Document APIs

### Day 5: Core Crate Setup
- [ ] Create `flui_core` crate structure
- [ ] Define `Widget` trait
- [ ] Define `Element` trait
- [ ] Define `RenderObject` trait
- [ ] Write initial tests

---

## 🎯 Success Criteria (Week 1)

### Must Have
- ✅ `flui_foundation` compiles without errors
- ✅ All tests pass (>10 tests total)
- ✅ Zero clippy warnings
- ✅ Documentation for public APIs
- ✅ Key system fully functional
- ✅ ChangeNotifier pattern working

### Nice to Have
- Diagnostics module started
- Platform detection implemented
- CI/CD setup
- Code coverage > 80%

---

## 🔜 Next Week (Week 2)

### flui_core Implementation

1. **Widget Trait**
   - Define trait
   - Implement `IntoWidget`
   - Write tests

2. **Element Trait**
   - Define trait
   - Implement `ElementId`
   - Implement `ElementTree`
   - Write tests

3. **RenderObject Trait**
   - Define trait
   - Implement `BoxConstraints`
   - Implement basic render objects
   - Write tests

4. **BuildContext**
   - Define struct
   - Implement tree traversal
   - Implement ancestor lookup
   - Write tests

---

## 💻 Development Tips

### Code Quality
```bash
# Before committing
cargo fmt
cargo clippy -- -D warnings
cargo test --all
```

### Documentation
```rust
/// Brief description
///
/// # Examples
///
/// ```
/// use flui_foundation::ValueKey;
///
/// let key = ValueKey::new(42);
/// assert_eq!(key.value(), &42);
/// ```
pub struct ValueKey<T> { /* ... */ }
```

### Testing
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        // Arrange
        let key = ValueKey::new(42);

        // Act
        let hash = key.hash_code();

        // Assert
        assert!(hash > 0);
    }
}
```

---

## 🐛 Common Issues

### Issue: Trait object safety
**Solution:** Use `Box<dyn Trait>` or `Arc<dyn Trait>`

### Issue: Lifetime errors
**Solution:** Use `'static` for callbacks, `Arc` for shared state

### Issue: Mutable borrow conflicts
**Solution:** Use `parking_lot::Mutex` or `RefCell`

---

## 📚 Resources

### References
- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [egui docs](https://docs.rs/egui/0.33/)
- Flutter architecture (in `docs/architecture/`)

### Tools
- `cargo-watch` - Auto-recompile on save
- `cargo-expand` - View macro expansions
- `cargo-flamegraph` - Performance profiling

---

## 🤝 Questions?

If stuck, refer to:
1. Architecture docs in `docs/architecture/`
2. ROADMAP.md for overall plan
3. Open an issue on GitHub

---

**Let's build something amazing! 🚀**

Start with: `cargo new --lib crates/flui_foundation`
