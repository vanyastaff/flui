# Thread-Safe Hooks System Refactoring

## Overview

This document describes the refactoring of the FLUI hooks system to be thread-safe by replacing `Rc`/`RefCell` with `Arc`/`Mutex` (using parking_lot).

## Motivation

The user requirement was: **"не забывай что у меня должно быть multi-thread ui"** (don't forget that I need to have multi-thread ui)

The original hooks system used:
- `Rc<T>` - reference-counted pointers (NOT thread-safe)
- `RefCell<T>` - interior mutability (NOT thread-safe)

These types cannot be sent between threads, which prevented the UI from being multi-threaded.

## Changes Made

### 1. Signal Hook (`crates/flui_core/src/hooks/signal.rs`)

**Replaced:**
- `Rc<RefCell<T>>` → `Arc<Mutex<T>>`
- `Rc<SignalInner<T>>` → `Arc<SignalInner<T>>`
- `Rc<dyn Fn()>` → `Arc<dyn Fn() + Send + Sync>`
- `.borrow()` → `.lock()`
- `.borrow_mut()` → `.lock()`

**Key Changes:**
- Updated `SignalInner` to use `Arc<Mutex<T>>` for value storage
- Updated subscribers HashMap to use `Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>`
- Changed all `Rc::clone()` calls to `Arc::clone()` for clarity
- Updated trait bounds: `T: Clone + Send + 'static`
- Updated callback bounds: `F: Fn() + Send + Sync + 'static`
- Updated documentation to reflect thread-safety

**Test Updates:**
- All test code using `Rc<RefCell<_>>` updated to `Arc<Mutex<_>>`
- All `.borrow()` calls changed to `.lock()`
- All `.borrow_mut()` calls changed to `.lock()`

### 2. Effect Hook (`crates/flui_core/src/hooks/effect.rs`)

**Replaced:**
- `Rc<EffectInner>` → `Arc<EffectInner>`
- `Rc<dyn Fn() -> Option<CleanupFn>>` → `Arc<dyn Fn() -> Option<CleanupFn> + Send + Sync>`
- `RefCell<Option<CleanupFn>>` → `Mutex<Option<CleanupFn>>`
- `RefCell<Vec<DependencyId>>` → `Mutex<Vec<DependencyId>>`
- `RefCell<bool>` → `Mutex<bool>`
- `CleanupFn` type updated: `Box<dyn FnOnce()>` → `Box<dyn FnOnce() + Send>`

**Key Changes:**
- Updated `EffectInner` to use `Mutex` for all interior mutability
- Updated trait bounds: `F: Fn() -> Option<CleanupFn> + Clone + Send + Sync + 'static`
- Simplified Drop implementation (Mutex doesn't need try_borrow_mut)
- Changed Arc/Rc clone patterns

### 3. Memo Hook (`crates/flui_core/src/hooks/memo.rs`)

**Replaced:**
- `Rc<MemoInner<T>>` → `Arc<MemoInner<T>>`
- `Rc<dyn Fn(&mut HookContext) -> T>` → `Arc<dyn Fn(&mut HookContext) -> T + Send + Sync>`
- `RefCell<Option<T>>` → `Mutex<Option<T>>`
- `RefCell<Vec<DependencyId>>` → `Mutex<Vec<DependencyId>>`
- `RefCell<bool>` → `Mutex<bool>`

**Key Changes:**
- Updated `MemoInner` to use `Mutex` for all interior mutability
- Updated trait bounds: `T: Clone + Send + 'static`, `F: Fn(&mut HookContext) -> T + Clone + Send + Sync + 'static`
- Updated PanicGuard Drop implementation (removed try_borrow_mut, Mutex always succeeds)
- All test code updated to use `Arc<Mutex<_>>`

### 4. Resource Hook (`crates/flui_core/src/hooks/resource.rs`)

**Replaced:**
- `Rc<F>` → `Arc<F>`

**Key Changes:**
- Updated trait bounds: `T: Clone + Send + 'static`, `E: Clone + Send + 'static`
- Updated function bounds: `F: Fn() -> Fut + Clone + Send + Sync + 'static`, `Fut: Future<Output = Result<T, E>> + Send + 'static`
- Resource already used `Signal<T>` which was updated in step 1

### 5. Test Harness (`crates/flui_core/src/hooks/test_harness.rs`)

**Added Bounds:**
- Added `H::State: Send` bound to `call()` method
- Added `H::State: Send` bound to `rerender()` method

These bounds are required because `HookContext::use_hook` requires `H::State: Send`.

## Thread-Safety Guarantees

After this refactoring:

1. **Signal<T>** is now `Send + Sync` (when T is Send)
2. **Effect** is now `Send + Sync`
3. **Memo<T>** is now `Send + Sync` (when T is Send)
4. **Resource<T, E>** is now `Send + Sync` (when T and E are Send)

All hook types can be safely sent between threads and shared across threads.

## Performance Considerations

### parking_lot::Mutex vs std::sync::Mutex

We use `parking_lot::Mutex` instead of `std::sync::Mutex` because:

1. **2-3x faster** than std::sync::Mutex in most cases
2. **No poisoning** - simpler error handling
3. **Smaller memory footprint** - 1 byte vs 40+ bytes on most platforms
4. **Fair locking** - prevents thread starvation

### Arc vs Rc Performance

`Arc` has a small overhead compared to `Rc`:
- Atomic reference counting (uses atomic instructions)
- Slightly slower increment/decrement operations
- However, this overhead is negligible for UI applications

The benefits (thread-safety) far outweigh the minor performance cost.

## Breaking Changes

### API Changes

All public hook functions now require additional trait bounds:

**Before:**
```rust
pub fn use_signal<T: Clone + 'static>(ctx: &BuildContext, initial: T) -> Signal<T>
```

**After:**
```rust
pub fn use_signal<T: Clone + Send + 'static>(ctx: &BuildContext, initial: T) -> Signal<T>
```

### Callback Bounds

All callbacks now require `Send + Sync`:

**Before:**
```rust
signal.subscribe(|| println!("Changed!"));
```

**After:**
```rust
// Same syntax, but closure must be Send + Sync
signal.subscribe(|| println!("Changed!"));
```

### User Impact

Most user code should continue to work without changes, as long as:
1. Signal values implement `Send` (most types do)
2. Callbacks don't capture non-Send types (rare in UI code)

## Testing

The refactoring was verified by:

1. ✅ `cargo check -p flui_core` - Library compiles successfully
2. ✅ `cargo build -p flui_core` - Library builds successfully
3. ✅ Thread-safety example - Demonstrates signals working across threads

Example demonstrating thread-safety:

```rust
use flui_core::hooks::hook_context::{ComponentId, HookContext};
use flui_core::hooks::signal::SignalHook;
use std::thread;

fn main() {
    let mut ctx = HookContext::new();
    ctx.begin_component(ComponentId(1));
    let signal = ctx.use_hook::<SignalHook<i32>>(0);

    let signal_clone = signal.clone();
    let handle = thread::spawn(move || {
        signal_clone.set(42);
    });

    handle.join().unwrap();
    assert_eq!(signal.get(&mut ctx), 42);
}
```

## Files Modified

1. `crates/flui_core/src/hooks/signal.rs` - Complete refactoring
2. `crates/flui_core/src/hooks/effect.rs` - Complete refactoring
3. `crates/flui_core/src/hooks/memo.rs` - Complete refactoring
4. `crates/flui_core/src/hooks/resource.rs` - Bounds updates
5. `crates/flui_core/src/hooks/test_harness.rs` - Added Send bounds

## Files Created

1. `crates/flui_core/examples/thread_safe_hooks.rs` - Thread-safety example

## Next Steps

1. Update all widget code that uses hooks to ensure compatibility
2. Add more multi-threaded tests
3. Consider adding `#[cfg(test)]` helper macros for common Arc<Mutex<_>> patterns in tests
4. Update documentation to highlight multi-threaded capabilities

## References

- parking_lot documentation: https://docs.rs/parking_lot/
- Rust atomics and locks: https://marabos.nl/atomics/
- Thread-safety in Rust: https://doc.rust-lang.org/nomicon/send-and-sync.html
