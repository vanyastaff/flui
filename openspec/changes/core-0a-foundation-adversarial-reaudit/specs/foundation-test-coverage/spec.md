# Foundation Test Coverage Specification

## Purpose

Pin the test-gap requirements surfaced by the adversarial re-audit: a
BindingBase retry-after-panic test (F17), an `Id` boundary test at
`usize::MAX` (F18), and the cross-change references for the regression tests
mandated by the soundness (F2, F6) and tree-soundness (F19) requirements.

Owner crates: `crates/flui-foundation` (`binding.rs`, `id.rs`),
`crates/flui-tree` (`traits/write.rs`).

---

## Requirements

### Requirement: BindingBase MUST have a retry-after-panic test (F17)

`crates/flui-foundation/src/binding.rs` MUST contain a test named
`instance_retries_after_panic` (or equivalent) that exercises the
`OnceLock::get_or_init` recovery semantic: after a panic in `new()` leaves
the cell unchanged, a subsequent call to `instance()` MUST succeed and flip
`INITIALIZED` to `true`.

This test is the symmetric complement of the existing
`init_panic_does_not_flip_initialized_flag` test (which verifies "panic leaves
flag false").  The new test verifies "retry after panic succeeds."

**Required test shape:**

```rust
// A binding whose new() panics only on the FIRST call.
struct RetryingBinding { counter: AtomicUsize }
impl RetryingBinding {
    fn new() -> Self {
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("transient init failure");
        }
        Self { counter: AtomicUsize::new(0) }
    }
}
impl_binding_singleton!(RetryingBinding);

#[test]
fn instance_retries_after_panic() {
    // First call panics; flag stays false.
    let r1 = std::panic::catch_unwind(|| RetryingBinding::instance());
    assert!(r1.is_err());
    assert!(!RetryingBinding::is_initialized());

    // Second call succeeds and flips flag.
    let inst = RetryingBinding::instance();
    assert!(RetryingBinding::is_initialized());

    // Subsequent calls return the same pointer (singleton guarantee).
    assert!(std::ptr::eq(inst, RetryingBinding::instance()));
}
```

**Acceptance criterion:** SC15 — `cargo test -p flui-foundation
instance_retries_after_panic` exits 0.

#### Scenario: Retry after transient panic succeeds (SC15)

- GIVEN a `RetryingBinding` whose `new()` panics on the first call only
- WHEN `std::panic::catch_unwind(|| RetryingBinding::instance())` is called →
  panics; `INITIALIZED` stays `false`
- WHEN `RetryingBinding::instance()` is called again → succeeds; `INITIALIZED`
  becomes `true`
- THEN `RetryingBinding::is_initialized()` returns `true`
- AND calling `RetryingBinding::instance()` a third time returns a pointer equal
  to (same address as) the second call's result

#### Scenario: is_initialized() is false after the first failing call

- GIVEN the first call to `RetryingBinding::instance()` panics (caught via
  `catch_unwind`)
- WHEN `RetryingBinding::is_initialized()` is queried
- THEN it returns `false` (the cell is unchanged by a panicking init)

---

### Requirement: Id MUST have a usize::MAX boundary test (F18)

`crates/flui-foundation/src/id.rs` tests MUST include:

1. **`id_at_usize_max`**: constructs `ViewId::zip(usize::MAX)` and asserts
   `id.unzip() == usize::MAX` — boundary value is reachable without panic.
2. **`id_overflow_wrap_panics`** (`#[should_panic]`): calls
   `ViewId::zip(usize::MAX.wrapping_add(1))` — wraps to `0`, which
   `NonZeroUsize::new(0)` rejects → panic.

These tests document the boundary behaviour at the slab-index limits and provide
a regression target for any future change to the `zip`/`unzip` implementation.

**Acceptance criterion:** SC16 — `cargo test -p flui-foundation id_at_usize_max`
exits 0.

#### Scenario: Id at usize::MAX is constructible without panic (SC16)

- GIVEN `ViewId::zip(usize::MAX)` is called
- THEN it returns an `Id` without panicking
- AND `id.unzip()` returns `usize::MAX`

#### Scenario: Id wrapping to 0 panics

- GIVEN `usize::MAX.wrapping_add(1)` equals `0`
- WHEN `ViewId::zip(0)` is called (or equivalently
  `ViewId::zip(usize::MAX.wrapping_add(1))`)
- THEN a panic is raised (the `NonZeroUsize` invariant rejects `0`)

#### Scenario: Test is annotated #[should_panic] for the overflow case

- GIVEN `crates/flui-foundation/src/id.rs` tests at HEAD
- WHEN the `id_overflow_wrap_panics` test function is inspected
- THEN it carries `#[should_panic]` (or `#[should_panic(expected = "...")]`)

---

## Cross-references

### F2 — Key counter exhaustion regression test

The regression test `key_counter_exhaustion` required by SC3 is specified as
the acceptance scenario "catch_unwind + retry does not produce duplicate keys"
in `foundation-soundness/spec.md § Requirement: Key::new MUST use the
fetch_update sentinel pattern`.  The test must:
- Drive the counter to the sentinel state.
- Verify that two consecutive `catch_unwind` calls both panic.
- Verify that `COUNTER` remains at `0` after both panics (no mutation).
- Verify that no `Key` with a duplicate value is ever produced.

### F6 — Listener fires after panic regression test

The regression test `listener_fires_after_panic` required by SC4 is specified
as the acceptance scenario "Listener[2] fires after Listener[1] panics — 3-listener
case" in `foundation-soundness/spec.md § Requirement: notify_listeners MUST
isolate each listener with catch_unwind`.  The test must use exactly 3 listeners,
where the middle one panics, and verify that the third listener fires AND
`tracing::error!` is emitted.

### F19 — Cascade cycle regression test

The regression test `cascade_cycle_detection` required by SC7 is specified as
the acceptance scenario "Cyclic tree does not OOM or hang" in
`tree-soundness-and-idioms/spec.md § Requirement: TreeWrite::remove cascade
MUST detect cycles`.  The test must:
- Construct a corrupted cyclic tree (A → B → A) via a test helper bypassing
  `add_child` cycle-check.
- Call `tree.try_remove(root)`.
- Assert `Err(TreeError::CycleDetected { .. })` is returned within bounded time.
- Assert `tracing::warn!` is emitted.
