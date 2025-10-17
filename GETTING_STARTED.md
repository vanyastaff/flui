# Getting Started with Flui

> Quick start guide for developing Flui framework

## 🚀 Quick Start

### Prerequisites

```bash
# Rust 1.75 or later
rustc --version

# Cargo
cargo --version
```

### Project Structure

```
flui/
├── Cargo.toml          # Workspace configuration
├── ROADMAP.md          # Development roadmap
├── docs/               # Documentation
│   ├── architecture/   # Architecture docs
│   └── glossary/       # Flutter concepts
├── crates/             # Framework crates (to be created)
│   ├── flui_core/
│   ├── flui_foundation/
│   ├── flui_widgets/
│   ├── flui_rendering/
│   ├── flui_painting/
│   ├── flui_animation/
│   ├── flui_gestures/
│   ├── flui_scheduler/
│   ├── flui_platform/
│   └── flui_provider/
├── flui/               # Main re-export crate
├── examples/           # Example applications
└── tests/              # Integration tests
```

## 📋 Development Phases

### Current Phase: Phase 0 ✅

**Status:** Initial setup complete

**What's Done:**
- ✅ Project structure defined
- ✅ Cargo.toml workspace configured
- ✅ Documentation architecture reviewed
- ✅ Roadmap created with egui 0.33

**What's Next:** Phase 1 - Foundation Layer

---

## 🎯 Phase 1: Foundation Layer (Next)

**Timeline:** Weeks 2-3
**Goal:** Implement core types and utilities

### Tasks

#### 1. Create `flui_foundation` crate

```bash
cargo new --lib crates/flui_foundation
```

**Implement:**
- `src/key.rs` - Key system (ValueKey, UniqueKey, GlobalKey)
- `src/change_notifier.rs` - ChangeNotifier trait
- `src/observer_list.rs` - Observer pattern
- `src/diagnostics.rs` - Debug utilities
- `src/platform.rs` - Platform detection

#### 2. Create `flui_core` crate

```bash
cargo new --lib crates/flui_core
```

**Implement:**
- `src/widget.rs` - Widget trait
- `src/element.rs` - Element trait & tree
- `src/render_object.rs` - RenderObject trait
- `src/build_context.rs` - BuildContext
- `src/box_constraints.rs` - Layout constraints

### Testing Strategy

```rust
// Example test structure
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_equality() {
        let key1 = ValueKey::new("test");
        let key2 = ValueKey::new("test");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_change_notifier() {
        let mut notifier = ChangeNotifier::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        notifier.add_listener(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        }));

        notifier.notify_listeners();
        assert!(called.load(Ordering::SeqCst));
    }
}
```

---

## 🔧 Development Workflow

### 1. Start with Foundation

```bash
# Create foundation crate
cd crates
cargo new --lib flui_foundation

# Add dependencies to crates/flui_foundation/Cargo.toml
[dependencies]
parking_lot.workspace = true
once_cell.workspace = true
serde.workspace = true
thiserror.workspace = true
```

### 2. Implement Core Types

```rust
// crates/flui_foundation/src/key.rs
use std::any::Any;
use std::fmt::Debug;

pub trait Key: Any + Debug + Send + Sync {
    fn equals(&self, other: &dyn Key) -> bool;
    fn hash_code(&self) -> u64;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug, Clone)]
pub struct ValueKey<T: Hash + Eq + Clone + Send + Sync + 'static> {
    value: T,
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> ValueKey<T> {
    pub fn new(value: T) -> Self {
        Self { value }
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
```

### 3. Write Tests

```rust
// crates/flui_foundation/src/key.rs
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
    fn value_key_hash_consistent() {
        let key = ValueKey::new("test");
        let hash1 = key.hash_code();
        let hash2 = key.hash_code();
        assert_eq!(hash1, hash2);
    }
}
```

### 4. Build & Test

```bash
# Build specific crate
cargo build -p flui_foundation

# Test specific crate
cargo test -p flui_foundation

# Test all
cargo test --workspace

# Check format
cargo fmt --check

# Lint
cargo clippy -- -D warnings
```

---

## 📚 Architecture Overview

### Three-Tree Pattern

```
Widget Tree              Element Tree            Render Tree
(Configuration)          (State)                 (Layout & Paint)
┌─────────────┐         ┌──────────────┐        ┌───────────────┐
│   MyApp     │────────>│ ComponentElem│        │               │
└─────────────┘         └──────────────┘        │               │
      │                        │                 │               │
      ▼                        ▼                 ▼               │
┌─────────────┐         ┌──────────────┐        ┌───────────────┐
│  Container  │────────>│ RenderObjElem│───────>│ RenderBox     │
└─────────────┘         └──────────────┘        └───────────────┘
      │                        │                        │
      ▼                        ▼                        ▼
┌─────────────┐         ┌──────────────┐        ┌───────────────┐
│    Text     │────────>│  LeafElement │───────>│ RenderPara    │
└─────────────┘         └──────────────┘        └───────────────┘
```

### Core Concepts

**Widget:**
- Immutable configuration
- Describes what to show
- Cheap to create/destroy

**Element:**
- Mutable state holder
- Manages lifecycle
- Preserves state across rebuilds

**RenderObject:**
- Layout computation
- Paint to screen
- Hit testing

---

## 🎓 Learning Resources

### Documentation

1. **Architecture Docs** (in `docs/architecture/`)
   - `nebula_arch_p1.txt` - Foundation layer
   - `nebula_arch_p2.txt` - Core traits
   - `nebula_arch_p3.txt` - Widget framework
   - `nebula_arch_p4.txt` - Rendering & animation
   - `nebula_arch_p5.txt` - Controllers & providers
   - `nebula_arch_p6.txt` - Performance optimization

2. **Glossary** (in `docs/glossary/`)
   - `foundation.md` - Foundation concepts
   - `widgets.md` - Widget system
   - `animation.md` - Animation system
   - `rendering.md` - Rendering concepts
   - `gestures.md` - Gesture handling

### External Resources

- [egui docs](https://docs.rs/egui/0.33/)
- [Flutter architecture](https://docs.flutter.dev/resources/architectural-overview)
- [Rust async book](https://rust-lang.github.io/async-book/)

---

## 🐛 Debugging & Tools

### Logging

```rust
use tracing::{info, debug, warn, error};

// Initialize logging
tracing_subscriber::fmt::init();

// Use in code
info!("Building widget tree");
debug!(count = 42, "Rebuilt {} elements", count);
```

### Profiling

```rust
// Enable profiling feature
#[cfg(feature = "profiling")]
puffin::profile_scope!("expensive_function");

fn expensive_function() {
    // ...
}
```

### Testing

```bash
# Unit tests
cargo test --lib

# Integration tests
cargo test --test integration_tests

# Doc tests
cargo test --doc

# Benchmarks
cargo bench
```

---

## 🤝 Contributing

### Code Style

```rust
// Use clippy
cargo clippy -- -D warnings

// Format
cargo fmt

// Check
cargo check --all-features
```

### Commit Messages

```
feat: Add ValueKey implementation
fix: Correct ChangeNotifier listener removal
docs: Update architecture documentation
test: Add tests for GlobalKey
refactor: Simplify Element tree structure
```

### Pull Requests

1. Create feature branch: `git checkout -b feat/my-feature`
2. Implement with tests
3. Run checks: `cargo test && cargo clippy && cargo fmt`
4. Commit and push
5. Open PR with description

---

## 📝 Next Steps

### For Contributors

1. ✅ Read ROADMAP.md
2. ✅ Review architecture docs
3. ⏳ Start with Phase 1: Foundation
4. ⏳ Implement flui_foundation crate
5. ⏳ Write comprehensive tests
6. ⏳ Document APIs

### For Users (Post-1.0)

1. Install: `cargo add flui`
2. Create app: `flui::FluiApp::new(MyApp).run()`
3. Build UI with widgets
4. Enjoy declarative Rust UI! 🎉

---

## 🎯 Success Criteria

### Phase 1 Complete When:

- ✅ `flui_foundation` compiles
- ✅ `flui_core` compiles
- ✅ All tests pass (>80% coverage)
- ✅ Documentation complete
- ✅ Clippy warnings = 0
- ✅ Simple "Hello World" example works

---

**Happy coding! 🚀**

For questions, see [ROADMAP.md](ROADMAP.md) or open an issue.
