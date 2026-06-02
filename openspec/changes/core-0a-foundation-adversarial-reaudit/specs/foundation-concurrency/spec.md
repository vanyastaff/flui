# Foundation Concurrency Specification

## Purpose

Pin the requirements for concurrency-correctness improvements across
`flui-foundation`: eliminating a redundant per-call `Release` atomic store in
`BindingBase`, clarifying the `check_disposed` debug/release dual-path shape in
`ChangeNotifier`, and cross-referencing the removed-during-notify and
listener-panic-isolation requirements that live in their primary D5/D1 specs.

Owner crates: `crates/flui-foundation` (`binding.rs`, `notifier.rs`).

---

## Requirements

### Requirement: BindingBase::instance() MUST use CAS for INITIALIZED flag (F4)

`BindingBase::instance()` in `crates/flui-foundation/src/binding.rs` MUST NOT
unconditionally issue `Self::INITIALIZED.store(true, Release)` on every call.
After first initialization, the steady-state path MUST be a single
`Relaxed`-load CAS-failure branch, achieved by replacing the unconditional store
with:

```rust
let _ = Self::INITIALIZED.compare_exchange(
    false, true,
    std::sync::atomic::Ordering::Release,
    std::sync::atomic::Ordering::Relaxed,
);
```

On the first call: `INITIALIZED = false`, CAS succeeds, flips to `true` with a
`Release` fence — correct initialization ordering.  On every subsequent call:
`INITIALIZED = true`, CAS fails (expected `false` but found `true`), the failure
path uses `Relaxed` — a single atomic load with no cache-coherence write.

**Observable semantics:** Identical to the current implementation.  The only
observable difference is the removal of unnecessary `Release` stores on the
hot path.

**Motivation:** All 5 production bindings — `SchedulerBinding`,
`GestureBinding`, `RendererBinding`, `SemanticsBinding`, `WidgetsBinding` — call
`instance()` on per-frame paths (every `setState`, every frame callback
registration).  Each unconditional `Release` store propagates through cache
coherence to all CPU cores.  The CAS form eliminates N–1 such stores for every
N calls.

#### Scenario: Steady-state instance() does not issue a Release store

- GIVEN a binding that has been initialized (first call to `instance()` succeeded
  and `is_initialized()` returns `true`)
- WHEN `instance()` is called N times in a tight loop (N > 1)
- THEN `INITIALIZED` remains `true` throughout
- AND no `Release`-ordered atomic write-back is issued after the first
  initialization (the CAS failure path is `Relaxed`-load only)
- AND `instance()` returns the same pointer on every call

#### Scenario: First initialization still uses Release ordering

- GIVEN a binding whose `INITIALIZED` flag is `false`
- WHEN `instance()` is called for the first time
- THEN `INITIALIZED` transitions from `false` to `true` with `Release` ordering
  (ensuring the initialization happens-before any subsequent `Acquire` load)
- AND `is_initialized()` returns `true` immediately after

#### Scenario: CAS is a no-op on already-initialized binding

- GIVEN `MyBinding::is_initialized()` returns `true`
- WHEN `MyBinding::instance()` is called
- THEN `INITIALIZED` stays `true` (CAS fails silently)
- AND the pointer returned equals `MyBinding::instance()` called again

---

### Requirement: check_disposed MUST use explicit cfg-gated branches (F20)

`ChangeNotifier::check_disposed` in `crates/flui-foundation/src/notifier.rs`
MUST separate the debug-panic path from the release-warn path using explicit
`#[cfg(debug_assertions)]` and `#[cfg(not(debug_assertions))]` attribute guards.

The following pattern MUST NOT be used:
```rust
debug_assert!(false, "ChangeNotifier used after dispose: ...");
tracing::warn!("ChangeNotifier used after dispose");
```
In this anti-pattern, `debug_assert!(false, ...)` panics in debug builds, making
the `tracing::warn!` call unreachable in debug mode.  The dead code is invisible
and misleads maintainers into thinking both branches fire simultaneously.

The correct shape:
```rust
if self.is_disposed.load(Ordering::Acquire) {
    #[cfg(debug_assertions)]
    panic!("ChangeNotifier used after dispose: ...");
    #[cfg(not(debug_assertions))]
    tracing::warn!("ChangeNotifier used after dispose");
    return true;
}
false
```

#### Scenario: debug_assert!(false, ...) pattern is absent from notifier.rs

- GIVEN `crates/flui-foundation/src/notifier.rs` at HEAD
- WHEN `grep -n "debug_assert!(false" crates/flui-foundation/src/notifier.rs`
  is run
- THEN it exits with code 1 (no matches)

#### Scenario: Debug build panics on use-after-dispose

- GIVEN a `ChangeNotifier` that has been disposed (`dispose()` called)
- WHEN any mutating method (e.g. `add_listener`) is called in a **debug** build
- THEN a panic is raised with a message containing "used after dispose"

#### Scenario: Release build emits tracing::warn! on use-after-dispose

- GIVEN a `ChangeNotifier` that has been disposed
- WHEN any mutating method is called in a **release** build (or with
  `debug_assertions = false`)
- THEN `tracing::warn!` is called with "used after dispose"
- AND `true` is returned (the method short-circuits gracefully)

---

## Cross-references

### F5 — Removed-during-notify listener MUST NOT fire

The primary requirement for the removed-during-notify behavioral change is
specified in `foundation-flutter-parity/spec.md § Requirement:
Removed-during-notify listener MUST NOT fire`.  It lives there because the change
is driven by Flutter parity (D5); see that spec for the acceptance scenario and
downstream-consumer impact.

### F6 — notify_listeners MUST isolate each listener with catch_unwind

The primary requirement for listener-panic isolation is specified in
`foundation-soundness/spec.md § Requirement: notify_listeners MUST isolate each
listener with catch_unwind`.  Cross-dimension: also D2 because a panicking
listener disrupts the ordering guarantee that all N registered listeners fire for
a given notification — the `catch_unwind` wrapper restores that guarantee.

### F19 + F24 — Cascade cycle-detection and SmallVec

The primary requirements for `TreeWrite::remove` cycle detection and
`SmallVec`-backed worklist are specified in `tree-soundness-and-idioms/spec.md`.
These are flui-tree concerns; see that spec for acceptance scenarios and the
`try_remove()` API surface.
