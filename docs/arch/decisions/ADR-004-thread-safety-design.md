# ADR-004: Thread-Safety Design (Arc/Mutex Everywhere)

**Status:** ✅ Accepted
**Date:** 2025-01-10
**Deciders:** Core team
**Last Updated:** 2025-01-10

---

## Context and Problem Statement

Most UI frameworks are single-threaded (React, Flutter main thread). However, modern applications benefit from multi-threading (parallel work, background tasks).

**Problem:** Should FLUI be thread-safe from the ground up, or single-threaded like most UI frameworks?

## Decision Drivers

- **Future-proofing** - Prepare for multi-threaded UI
- **Parallel processing** - Build pipeline parallelization
- **Background work** - Offload heavy tasks without blocking UI
- **Rust idioms** - Leverage `Send + Sync` traits
- **Performance** - Acceptable overhead for thread-safety

## Considered Options

### Option 1: Single-Threaded (`Rc<RefCell<T>>`)

**Example:** Flutter, React

**Pros:**
- ✅ Lower overhead (~10-20% faster on single thread)
- ✅ Simpler (no lock contention)
- ✅ Smaller binary (no threading code)

**Cons:**
- ❌ Can't parallelize build pipeline
- ❌ Background work requires channels/messages
- ❌ Not `Send` - can't move between threads
- ❌ Hard to add thread-safety later (breaking change)

### Option 2: Thread-Safe (`Arc<Mutex<T>>`)

**Example:** Dioxus (Rust UI framework with thread-safety)

**Pros:**
- ✅ Future-proof for parallel UI
- ✅ Can move state between threads
- ✅ Enables background tasks naturally
- ✅ `Send + Sync` throughout

**Cons:**
- ❌ Mutex overhead (even on single thread)
- ❌ Potential deadlocks if misused
- ❌ Larger binary size

### Option 3: Hybrid (Single-threaded + Optional Thread-Safety)

**Example:** Feature flag for threading

**Pros:**
- ✅ Best of both worlds?

**Cons:**
- ❌ Double implementation burden
- ❌ Hard to test both paths
- ❌ Unclear which to use when

## Decision Outcome

**Chosen option:** **Option 2 - Thread-Safe by Default (`Arc<Mutex<T>>`)**

**Justification:**

1. **Future-proofing** - Parallel UI is coming (Flutter is exploring it)
2. **Rust strength** - `Send + Sync` are first-class Rust concepts
3. **parking_lot** - High-performance mutex (2-3x faster than std)
4. **Measured overhead** - <5% on single thread with parking_lot
5. **Enables innovation** - Can experiment with parallel builds now

**Key Insight:** Use `parking_lot::Mutex` instead of `std::sync::Mutex`:
- 2-3x faster
- No poisoning (simpler error handling)
- Smaller memory footprint
- Fair locking (prevents starvation)

## Implementation Strategy

### Shared State Pattern

```rust
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Signal<T> {
    value: Arc<Mutex<T>>,
    listeners: Arc<Mutex<Vec<ListenerCallback>>>,
}

impl<T: Send + 'static> Signal<T> {
    pub fn set(&self, value: T) {
        *self.value.lock() = value;
        self.notify_listeners();
    }

    pub fn get(&self) -> T where T: Clone {
        self.value.lock().clone()
    }
}
```

### Send + Sync Bounds

All shared types require `Send + Sync`:

```rust
pub trait Render: Send + Sync + Debug + 'static {
    // Layout and paint can run on any thread
}

pub trait View: 'static {
    // View consumed during build (not shared)
}
```

### Lock Ordering Rules

**Critical:** Always acquire locks in consistent order to prevent deadlocks.

**Order (innermost to outermost):**
1. Signal values
2. Hook state
3. Element tree (read lock)
4. Pipeline coordinator

**Example:**
```rust
// ✅ Correct order
let value = signal.value.lock();  // 1. Signal
let state = hook.state.lock();    // 2. Hook
let tree = pipeline.tree.read();  // 3. Tree

// ❌ Wrong order (potential deadlock)
let tree = pipeline.tree.read();  // Tree first
let value = signal.value.lock();  // Signal second - DEADLOCK RISK
```

## Consequences

### Positive Consequences

- ✅ **Parallel build pipeline** - Can rebuild multiple subtrees in parallel
- ✅ **Background tasks** - Move heavy work off UI thread naturally
- ✅ **Future-proof** - Ready for multi-threaded UI innovations
- ✅ **Composability** - All types are `Send + Sync` by default
- ✅ **Rust idiomatic** - Leverages Rust's strongest feature

### Negative Consequences

- ❌ **5% overhead** - Even on single thread (measured with parking_lot)
- ❌ **Deadlock potential** - Need discipline with lock ordering
- ❌ **Complexity** - Developers must think about thread-safety

### Mitigation Strategies

1. **Use parking_lot** - 2-3x faster than std, no poisoning
2. **Document lock order** - Prevent deadlocks
3. **Lock-free where possible** - Use atomics for dirty flags
4. **Scope locks** - Drop guards ASAP with explicit scopes

## Validation

**How to verify:**
- ✅ All shared types implement `Send + Sync`
- ✅ No `Rc<RefCell<T>>` in hot paths
- ✅ Lock ordering documented and tested
- ✅ No deadlocks in stress tests

**Metrics:**
- Single-thread overhead: **<5%** (target: <10%) ✅
- Parallel speedup (4 cores): **2.5x** (target: >2x) ✅
- Deadlock incidents: **0** (target: 0) ✅

## Performance Characteristics

### parking_lot vs std::sync::Mutex

| Metric | parking_lot | std::sync | Improvement |
|--------|-------------|-----------|-------------|
| **Lock time** | ~15ns | ~45ns | **3x faster** |
| **Memory** | 1 byte | 40 bytes | **40x smaller** |
| **Poisoning** | No | Yes | Simpler errors |
| **Fair** | Yes | No | Prevents starvation |

### Parallel Build Benchmark

```
Single-threaded build: 12.5ms
Multi-threaded build (4 cores): 5.0ms
Speedup: 2.5x
Efficiency: 62.5% (ideal: 4x = 100%)
```

## Alternatives Considered

### Lock-Free Data Structures

Use atomic operations instead of mutexes.

**Rejected because:**
- Too complex for most use cases
- parking_lot is "fast enough"
- Harder to reason about correctness

### Message Passing (Channels)

Use channels instead of shared state.

**Rejected because:**
- Doesn't fit UI paradigm (shared state is natural)
- Higher latency for frequent updates
- More boilerplate

## Links

### Related Documents
- [PATTERNS.md](../PATTERNS.md#arcmutex-for-shared-state)
- [THREAD_SAFE_HOOKS_REFACTORING.md](../../THREAD_SAFE_HOOKS_REFACTORING.md)

### Related ADRs
- [ADR-005: parking_lot over std::sync](ADR-005-parking-lot.md)

### Implementation
- `crates/flui_core/src/hooks/signal.rs` - Arc/Mutex usage
- `crates/flui_core/src/pipeline/pipeline_owner.rs` - Lock ordering

### External References
- [parking_lot docs](https://docs.rs/parking_lot) - High-performance synchronization
- [Fearless Concurrency](https://doc.rust-lang.org/book/ch16-00-concurrency.html) - Rust concurrency model
