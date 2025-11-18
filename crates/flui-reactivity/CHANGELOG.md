# Changelog

All notable changes to the `flui-reactivity` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **RuntimeConfig for memory limits** (CRITICAL-16)
  - Added `RuntimeConfig` struct with configurable memory limits:
    - `max_signals: usize` (default: 100,000) - Prevents DoS from signal leaks
    - `max_subscribers_per_signal: usize` (default: 1,000) - Prevents memory exhaustion
    - `max_computed_depth: usize` (default: 100) - Prevents infinite recursion
  - Added `SignalRuntime::with_config()` constructor for custom configurations
  - `SignalRuntime::create_signal()` now panics if `max_signals` is exceeded
  - `SignalRuntime::subscribe()` now uses `config.max_subscribers_per_signal` instead of hard-coded constant
  - Exported `RuntimeConfig` from public API

- **Integration tests for thread safety** (CRITICAL-15)
  - Added comprehensive integration test suite in `tests/thread_safety.rs`:
    - Concurrent signal creation (10 threads × 100 signals)
    - Concurrent signal updates (10 threads × 100 updates)
    - Concurrent subscriptions (10 threads × 50 subscriptions)
    - Concurrent batch updates with deduplication
    - Nested batch updates from multiple threads
    - Subscribe/unsubscribe race conditions
    - Signal cleanup under concurrent load
    - Parallel batch consistency verification
    - Thread-local batching state isolation
    - Stress test with 100 threads × 10,000 operations

- **Batch depth tracking** (HIGH-8)
  - Added `MAX_BATCH_DEPTH` constant (20) to prevent stack overflow
  - Added `WARN_BATCH_DEPTH` constant (10) for early warning
  - `batch()` function now tracks nesting depth and panics if limit exceeded
  - Warns at depth 10 to help developers refactor before hitting limit

- **Lock-free Computed optimization** (PERF-11)
  - Replaced `Mutex<bool>` with `AtomicBool` for `is_dirty` flag in `Computed`
  - Uses `Ordering::Acquire`/`Release` for proper memory ordering
  - Significantly reduces contention in read-heavy workloads
  - No behavior changes, only performance improvement

- **Thread-local batching documentation** (Code Review 2025-01-18)
  - Enhanced `batch()` documentation to clarify thread-local behavior
  - Added examples showing independent batch queues per thread
  - Documented that cross-thread signals are NOT deduplicated across threads
  - Prevents confusion about batching scope in multi-threaded scenarios

### Changed

- **Enhanced panic documentation for `Computed::get()`**
  - Clarified that `get()` panics on circular dependencies (programming errors, not runtime conditions)
  - Added detailed panic conditions: circular dependencies, deadlock timeout (5s), computation panics
  - Documented that panics are NOT cached - computation re-runs on next access
  - This follows Rust conventions: panic on programming errors (like `Vec[index]` for out-of-bounds)

- **BREAKING:** `SignalRuntime::subscribe()` error message now references `RuntimeConfig::max_subscribers_per_signal` instead of `MAX_SUBSCRIBERS_PER_SIGNAL` constant
- **BREAKING:** `MAX_SUBSCRIBERS_PER_SIGNAL` constant is now deprecated (use `RuntimeConfig::max_subscribers_per_signal` instead)
- **BREAKING:** `SignalRuntime::create_signal()` now panics with detailed message when signal limit exceeded (instead of silently creating signals)

### Fixed

- **CRITICAL: Double-unsubscription memory leak in Computed rollback** (Code Review 2025-01-18)
  - Fixed memory leak in `Computed::new()` when subscription fails during creation
  - Rollback logic now uses `std::mem::forget()` to prevent double-unsubscription
  - Prevents affecting unrelated signals if subscription IDs are reused
  - Ensures proper cleanup without triggering StoredSubscription::drop() twice

- **CRITICAL: Race condition in batch emergency flush** (Code Review 2025-01-18)
  - Fixed race condition where notifications could be lost during emergency flush
  - Current notification now inserted BEFORE flush instead of after
  - Prevents out-of-order execution and lost notifications
  - Eliminates race window between lock release and re-acquisition

- **HIGH: Memory ordering bug in Computed is_dirty flag** (Code Review 2025-01-18)
  - Changed from Acquire/Release pattern to atomic swap with AcqRel ordering
  - Prevents stale values when dependencies change rapidly under high contention
  - Atomically checks and resets dirty flag in a single operation
  - Fixes subtle correctness bug that only manifested under concurrent access

- **MEDIUM: Owner cleanup race condition** (Code Review 2025-01-18)
  - Fixed potential double-cleanup in `OwnerInner::drop()` and `Owner::cleanup()`
  - Changed from separate load/store to atomic `compare_exchange`
  - Prevents race where two threads both see disposed=false and both run cleanup
  - Ensures cleanup runs exactly once even under concurrent access

- **Deadlock detection in Computed signals** (CRITICAL-1)
  - Added 5-second timeout for lock acquisition in `Computed::recompute()`
  - Prevents indefinite hangs from circular dependencies across threads
  - Three lock points now protected: compute_fn, dependencies, subscriptions
  - Returns `SignalError::DeadlockDetected` with resource name and timeout
  - Uses proper `Result<(), ReactivityError>` instead of `.expect()` panic
  - Clear error message indicates deadlock and suggests reviewing dependency graph
  - Thread-local cycle detection already handled same-thread cycles
  - This fix addresses cross-thread deadlock scenarios

- **Panic safety in Signal::update()** (HIGH-18)
  - Fixed critical bug where panics in update closures would trigger subscribers with stale data
  - Added `value_changed` flag to track successful completion
  - Now only notifies subscribers if closure completes without panicking
  - If panic occurs, subscribers are NOT notified and panic is re-raised
  - Same fix applied to `update_mut()`

- **Signal Clone documentation** (CRITICAL-3)
  - Clarified that `Signal<T>` derives `Clone` but is NOT `Copy` for all generic types
  - Updated comments in `use_reducer` to explain clone behavior
  - Prevents confusion about when `.clone()` is needed

### Documentation

- **Enhanced panic documentation**
  - `Signal::get()` - Documents panic behavior (already existed)
  - `Signal::update()` - Documents panic safety and non-notification on panic
  - `Computed::get()` - Enhanced documentation about panic behavior:
    - Panics are NOT cached (computation re-runs on next access)
    - Computed remains dirty if computation panics
    - Will panic again on every access until it succeeds or dependencies change
  - `SignalRuntime::create_signal()` - Documents panic on signal limit exceeded
  - `batch()` - Documents panic on excessive nesting depth

- **Architecture improvements**
  - Better separation of concerns between signal storage and runtime management
  - Clear ownership model for RuntimeConfig
  - Thread-safety guarantees verified by integration tests

## Migration Guide (Unreleased → Next)

### For Library Users

If you're using `flui-reactivity` as a dependency:

**No changes required for most users.** The changes are backwards compatible except for edge cases:

1. **If you're creating more than 100,000 signals:** Increase the limit via `RuntimeConfig`
   ```rust
   use flui_reactivity::{RuntimeConfig, SignalRuntime};

   let config = RuntimeConfig {
       max_signals: 1_000_000,
       ..Default::default()
   };

   // Note: Currently only the global SIGNAL_RUNTIME uses default config
   // Custom config support requires CRITICAL-14 (scoped runtime API)
   ```

2. **If you're hitting subscriber limits:** The behavior is the same, but error messages now reference `RuntimeConfig::max_subscribers_per_signal`

3. **If you rely on subscribers being notified after panics:** This was a bug. Update closures should not panic. If they do, subscribers are no longer notified with stale data.

### For Contributors

If you're developing `flui-reactivity`:

1. **Run new integration tests:**
   ```bash
   cargo test -p flui-reactivity --test thread_safety
   ```

2. **Run stress test (optional):**
   ```bash
   cargo test -p flui-reactivity --test thread_safety -- --ignored stress_test
   ```

3. **Use RuntimeConfig for custom limits:**
   - Import from `flui_reactivity::RuntimeConfig`
   - All limits are now configurable instead of hard-coded constants

4. **Check batch nesting depth:**
   - If you see warnings about batch depth >= 10, refactor to reduce nesting
   - Depth 20 will panic to prevent stack overflow

## Known Limitations

### Unbounded Memory Growth (Issue #22)

**Signals are never removed from the runtime.** When a `Signal<T>` is dropped, it remains in the global `SignalRuntime` forever. This causes:

- Memory leak in long-running applications
- After 100,000 signals created, app will panic (default `max_signals` limit)
- No automatic garbage collection of unused signals

**Workarounds:**
- Limit signal creation via `RuntimeConfig::max_signals`
- Avoid creating signals in hot paths or loops
- Restart application periodically for long-running services
- Use `signal.owned(owner)` to tie signals to component lifetime

**Future Fix:** Planned for v0.2.0 - Add reference counting or weak references for automatic signal cleanup.

See `ISSUES.md` issue #22 for detailed discussion and proposed solutions.

---

## [0.1.0] - Previous Release

Initial release with core reactivity features:
- Signal primitive with thread-safe updates
- Computed values with automatic dependency tracking
- Batch updates with deduplication
- Effect scheduling with priorities
- Hook system (use_signal, use_memo, use_effect, etc.)
- Context API for dependency injection
- Full thread-safety with Arc/Mutex

---

## Versioning Policy

This project follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version for incompatible API changes
- **MINOR** version for backwards-compatible functionality additions
- **PATCH** version for backwards-compatible bug fixes

Breaking changes are clearly marked with **BREAKING** in the changelog.
