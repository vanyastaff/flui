# flui-reactivity

Production-ready reactive state management system for Rust, inspired by React Hooks and Solid.js. Provides signals, computed values, effects, and a comprehensive hook system with full thread-safety.

## Features

### Core Reactivity System

- **Signals** (`signal.rs`) - Reactive state holders with automatic change tracking
- **Computed** (`computed.rs`) - Derived values that update when dependencies change
- **Runtime** (`runtime.rs`) - Global signal runtime with DashMap for lock-free access
- **Batching** (`batch.rs`) - Batch multiple updates for better performance
- **Scheduler** (`scheduler.rs`) - Effect scheduling with priorities

### Hooks System

All hooks are thread-safe and follow React's rules of hooks:

- **`use_signal`** - Create reactive state (via Signal::new())
- **`use_callback`** - Memoized callbacks with dependency tracking
- **`use_reducer`** - Redux-style state management with dispatch
- **`use_ref`** - Mutable references without triggering re-renders
- **`use_effect`** - Side effects with automatic cleanup
- **`use_memo`** - Memoized computations
- **`use_resource`** - Async data fetching (requires `async` feature)

### Context System

- **Context Provider** (`context_provider.rs`) - Provide/consume context values
- **Context** (`context.rs`) - Hook context management with lifecycle
- **Owner** (`owner.rs`) - Ownership tracking for cleanup

### Utilities

- **Error Handling** (`error.rs`) - Comprehensive error types
- **Traits** (`traits.rs`) - Core traits (Hook, ReactiveHook, AsyncHook)
- **Test Harness** (`test_harness.rs`) - Testing utilities
- **Async** (`async.rs`) - Async runtime integration (optional)

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                     │
│              (use_signal, use_effect, etc.)             │
└─────────────────────────────────────────────────────────┘
                           │
┌─────────────────────────────────────────────────────────┐
│                     Hooks System                         │
│   (callback, reducer, ref, effect, memo, resource)      │
└─────────────────────────────────────────────────────────┘
                           │
┌─────────────────────────────────────────────────────────┐
│                   Core Reactivity                        │
│        (Signal, Computed, Runtime, Scheduler)           │
└─────────────────────────────────────────────────────────┘
                           │
┌─────────────────────────────────────────────────────────┐
│                  Storage & Sync                          │
│       (DashMap, parking_lot, Arc<Mutex<T>>)            │
└─────────────────────────────────────────────────────────┘
```

## Usage Examples

### Basic Signal

```rust
use flui_reactivity::Signal;

let count = Signal::new(0);
println!("Count: {}", count.get());

count.set(42);
count.update(|n| n + 1);
```

### Effect with Cleanup

```rust
use flui_reactivity::{use_effect, DependencyId};

let count_dep = DependencyId::new(count.id().0);
use_effect(ctx, vec![count_dep], || {
    println!("Count changed!");
    
    // Cleanup function
    Some(Box::new(|| {
        println!("Cleaning up!");
    }))
});
```

### Computed Values

```rust
use flui_reactivity::Computed;

let count = Signal::new(0);
let doubled = Computed::new(move || count.get() * 2);

println!("Doubled: {}", doubled.get());
```

### Reducer Pattern

```rust
use flui_reactivity::{use_reducer, Reducer};
use std::sync::Arc;

#[derive(Clone)]
enum Action {
    Increment,
    Decrement,
}

let reducer = Arc::new(|state: &i32, action: Action| match action {
    Action::Increment => state + 1,
    Action::Decrement => state - 1,
});

let (count, dispatch) = use_reducer(ctx, 0, reducer);
dispatch.send(Action::Increment);
```

### Async Resource Loading

```rust
use flui_reactivity::{use_resource, ResourceState};

let user_resource = use_resource(ctx, vec![], || {
    Box::pin(async {
        fetch_user().await
    })
});

match user_resource.state() {
    ResourceState::Loading => println!("Loading..."),
    ResourceState::Ready(user) => println!("User: {:?}", user),
    ResourceState::Error(err) => println!("Error: {}", err),
    ResourceState::Idle => println!("Idle"),
}
```

### Context Providing

```rust
use flui_reactivity::{provide_context, use_context, ContextId};

// Provide context
let theme_id = ContextId::new();
provide_context(theme_id, "dark");

// Consume context
let theme: Option<String> = use_context(theme_id);
```

## Thread Safety

All types are designed for multi-threaded UI applications:

- ✅ `Signal<T>` is `Send + Sync` (requires `T: Send`)
- ✅ All hooks use `Arc<Mutex<T>>` with parking_lot
- ✅ `SignalRuntime` uses `DashMap` for lock-free reads
- ✅ Callbacks must be `Send + Sync + 'static`

## Feature Flags

```toml
[dependencies]
flui-reactivity = { version = "0.1", features = ["async"] }
```

- **`async`** - Enable async support (use_resource, async runtime)
- **`serde`** - Serialization support for signals

## Testing

Run all tests:

```bash
cargo test --lib
```

Run with async features:

```bash
cargo test --lib --features async
```

## Implementation Status

✅ **Complete** (81/81 tests passing):
- Core reactivity (Signal, Computed, Runtime)
- All hooks (callback, reducer, ref, effect, memo, resource)
- Context system (Provider, Consumer)
- Error handling
- Batching and scheduling
- Owner/cleanup system
- Thread-safety

## Performance

- **Lock-free reads**: DashMap enables concurrent signal access
- **Batching**: Group updates to minimize re-renders
- **Lazy evaluation**: Computed values only update when accessed
- **Memoization**: Callbacks and computations cache results
- **Zero-cost**: Hook IDs are compile-time checked

## Contributing

This crate is part of the FLUI framework. See the main FLUI repository for contribution guidelines.

## License

MIT OR Apache-2.0
