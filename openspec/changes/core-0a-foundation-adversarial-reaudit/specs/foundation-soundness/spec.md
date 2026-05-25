# Foundation Soundness Specification

## Purpose

Pin the requirements for eliminating unsafe surface that is either vacuous
(performs no actual unsafe operation), provably undefined-behaviour-producing
(reachable through `catch_unwind` + retry), or crash-class production bugs
(a panicking listener silently aborts downstream notifications). These
requirements close the D1-soundness and D1+D2+D5-cross-dimension findings
surfaced by the adversarial re-audit of `flui-foundation` and `flui-tree`.

Owner crates: `crates/flui-foundation` (`id.rs`, `key.rs`, `notifier.rs`).

---

## Requirements

### Requirement: Id::from_raw MUST be a safe function (F1)

`Id<T>::from_raw(raw: RawId) -> Self` in `crates/flui-foundation/src/id.rs` MUST
be declared as `pub const fn from_raw(raw: RawId) -> Self` — NOT `pub unsafe const
fn from_raw(raw: RawId) -> Self`.

The function body wraps a validated `RawId` (a `NonZeroUsize` newtype) with a
`PhantomData<fn() -> T>` tag.  No `transmute`, no pointer arithmetic, no
`*_unchecked` call appears in the body.  Marking it `unsafe fn` is vacuous: it
signals "caller must uphold invariants" when there are no invariants to uphold
(markers are uninhabited `'static` ZSTs; any `RawId` is valid for any marker by
the type system's construction).

**Rust-native change:**
- (a) `unsafe` keyword removed from the public function signature.
- (b) Better because `unsafe fn` carries an implicit safety contract; a vacuous
  contract misleads contributors into believing a contract exists where it does
  not, and imposes unnecessary `unsafe {}` blocks on callers.
- (c) Downstream consumers: callers wrapping the call in `unsafe { Id::from_raw(r) }`
  will receive `#[warn(unused_unsafe)]`; this is benign and trivially fixable.

**Acceptance criterion:** SC18 — `! grep -n "unsafe fn from_raw"
crates/flui-foundation/src/id.rs` exits 0.

#### Scenario: from_raw is callable in safe Rust

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD (after change)
- WHEN `Id::<SomeMarker>::from_raw(raw_id)` is written in a safe Rust function body
- THEN it compiles without an enclosing `unsafe {}` block

#### Scenario: No unsafe fn signature in id.rs for from_raw (SC18)

- GIVEN `crates/flui-foundation/src/id.rs` at HEAD
- WHEN `grep -n "unsafe fn from_raw" crates/flui-foundation/src/id.rs` is run
- THEN it exits with code 1 (no matches)

---

### Requirement: Key::new MUST use the fetch_update sentinel pattern (F2)

`Key::new()` in `crates/flui-foundation/src/key.rs` MUST implement counter
management via `AtomicU64::fetch_update` with counter value `0` as a
permanent-exhaustion sentinel.  `NonZeroU64::new_unchecked` MUST NOT appear
in this function.

**Required invariants:**

1. `COUNTER` is initialised to `1`.  Valid key values are `1 ..= u64::MAX`.
2. `fetch_update` closure: if `current == 0` (permanent-exhaustion sentinel)
   → return `None` (CAS fails → `.expect("Key counter overflow")` panics,
   leaving `COUNTER = 0` unmodified).  Otherwise → return
   `Some(current.wrapping_add(1))` (advances counter; `u64::MAX + 1` wraps to
   the sentinel `0`).
3. The returned `id` (old value from `fetch_update`) is passed to
   `NonZeroU64::new(id).expect("counter returned zero")`.  `id` is always
   non-zero when `fetch_update` succeeds: the closure only returns `Some` when
   `current != 0`, so the old value is in `1 ..= u64::MAX`.
4. A `// INVARIANT:` or `// SAFETY:` comment MUST document the sentinel.

**Why fetch_update is required (Codex verdict):**
The partial fix `NonZeroU64::new(id).expect(...)` eliminates UB but introduces a
correctness bug: after `COUNTER` wraps to `0` and the `expect` panics, a
`catch_unwind` + retry sees `COUNTER = 0` → `fetch_add` returns `0` → the
`expect` panics again but with `COUNTER` now equal to `1` (the fetch_add wrote
`1`).  A second retry therefore returns `id = 1`, producing a **duplicate** of
the very first key ever issued.  The sentinel pattern closes both the UB and the
duplicate-key risk in one shape.

**Flutter ref:** N/A — this is a Rust-native safety improvement with no Flutter
equivalent.

**Acceptance criteria:** SC2 (`! grep -rn "new_unchecked"
crates/flui-foundation/src/key.rs` exits 0), SC3 (`cargo test -p flui-foundation
key_counter_exhaustion` exits 0).

#### Scenario: Last valid key is u64::MAX, next call panics permanently

- GIVEN the Key counter has been driven (via a test-internal helper) to exactly
  `u64::MAX - 1`
- WHEN `Key::new()` is called — `current = u64::MAX - 1`, counter advances to
  `u64::MAX`, returns `Key(u64::MAX - 1)`
- WHEN `Key::new()` is called again — `current = u64::MAX`, counter advances to
  `0` (sentinel), returns `Key(u64::MAX)` [last valid key]
- WHEN `Key::new()` is called a third time — `current = 0` (sentinel), closure
  returns `None`, `fetch_update` returns `Err(0)`, `.expect()` panics
- THEN no `Key` with value `0` has ever been produced
- AND all issued keys `1 ..= u64::MAX` are unique

#### Scenario: catch_unwind + retry does not produce duplicate keys (SC3)

- GIVEN the Key counter is at `0` (sentinel — permanent exhaustion, reachable in
  test via a reset helper or by driving counter to wrap)
- WHEN `std::panic::catch_unwind(|| Key::new())` is called → panics (sentinel)
- WHEN `std::panic::catch_unwind(|| Key::new())` is called AGAIN → panics again
- THEN `COUNTER` remains `0` after both panics (no mutation on CAS failure)
- AND no `Key` is produced with a value matching any previously-issued key
- AND (contrast with `.expect()` partial fix): if `fetch_add + new(id).expect()`
  were used instead, the retry WOULD return `id = 1`, producing a duplicate — the
  test MUST fail under that implementation

#### Scenario: No new_unchecked in key.rs (SC2)

- GIVEN `crates/flui-foundation/src/key.rs` at HEAD
- WHEN `grep -n "new_unchecked" crates/flui-foundation/src/key.rs` is run
- THEN it exits with code 1 (no matches)

---

### Requirement: UniqueKey::new MUST guard against counter overflow (F3)

`UniqueKey::new()` in `crates/flui-foundation/src/key.rs` MUST assert that the
counter has not wrapped before returning a new key.  The contract "Each
`UniqueKey` instance is different from all other keys" (expressed in the type's
doc-comment) MUST be enforced at the `u64::MAX` boundary.

Fix shape: immediately after `let id = COUNTER.fetch_add(1, Ordering::Relaxed)`,
add `assert!(id != u64::MAX, "UniqueKey counter exhausted — cannot issue more unique keys")`.

This mirrors the pattern used by `Key::new` (pre-F2) and closes the gap where
`UniqueKey` silently produces duplicate IDs after `2^64 – 1` calls.  The
assertion panics rather than producing a silent collision because silent key
collision violates a documented type-level invariant.

**Acceptance criterion:** `cargo test -p flui-foundation unique_key_overflow_guard`
exits 0 (regression test that counter at `u64::MAX - 1` → panics on next call).

#### Scenario: UniqueKey at boundary panics before wrapping

- GIVEN a test that drives the UniqueKey COUNTER to `u64::MAX - 1` via an
  internal reset helper
- WHEN `UniqueKey::new()` is called → returns a key with `id = u64::MAX - 1`
  (the pre-increment value), COUNTER advances to `u64::MAX`
- WHEN `UniqueKey::new()` is called → `id = u64::MAX`, `assert!(u64::MAX !=
  u64::MAX, ...)` PANICS with "UniqueKey counter exhausted"
- THEN no two issued `UniqueKey` values in the test have the same `id`

---

### Requirement: notify_listeners MUST isolate each listener with catch_unwind (F6) [PRIMARY]

`ChangeNotifier::notify_listeners` in `crates/flui-foundation/src/notifier.rs`
MUST wrap each individual callback invocation in:
```rust
std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()))
```
On `Err(payload)`, it MUST emit:
```rust
tracing::error!(?payload, "ChangeNotifier listener panicked; continuing");
```
and continue to the next callback in the snapshot.  The notification loop MUST
complete even if one or more listeners panic.

**Why AssertUnwindSafe is justified:** `ListenerCallback = Arc<dyn Fn() + Send +
Sync + 'static>`.  The closure borrows `&callback` (an Arc reference).  The
`ChangeNotifier` itself holds no `&mut`-borrowed invariants across the unwind
boundary: the snapshot was taken by cloning the Arcs into a local `SmallVec`
before the loop, so there is no borrowed mutable state in the notifier body at
the point of the `catch_unwind`.  The `AssertUnwindSafe` wrapper is therefore
correct for the callback invocation.

**Why this is a soundness requirement, not merely a parity requirement:** A
panicking listener at position N causes the Rust runtime to unwind through
`notify_listeners`, through `ValueNotifier::set_value`, and through every other
frame on the call stack — leaving frame-level data in an inconsistent state and
silently skipping listeners N+1, N+2, ...  In a frame-pipeline context, listener
N+1 may be the renderer's repaint subscriber — its silence produces a stale frame.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:443-470` — `notifyListeners` for-loop with per-listener `try { _listeners[i]?.call(); } catch (exception, stack) { FlutterError.reportError(...); }`.

**Rust-native breaking change:**
- (a) Panics from listeners are now caught and logged rather than propagated.
- (b) Matches Flutter's per-listener isolation; prevents frame-pipeline corruption.
- (c) Downstream consumers: test scaffolds that rely on a notifier-propagating
  panic for assertion (e.g. `std::panic::catch_unwind(|| notifier.notify())`)
  will no longer catch listener panics via this path and MUST switch to
  tracing-subscriber-based verification.

**Acceptance criterion:** SC4 — `cargo test -p flui-foundation listener_fires_after_panic` exits 0.

Cross-referenced in: `foundation-concurrency/spec.md` (D2 ordering guarantee under
panic isolation), `foundation-flutter-parity/spec.md` (D5 parity contract with
Flutter's `notifyListeners`).

#### Scenario: Listener[2] fires after Listener[1] panics — 3-listener case (MANDATORY)

- GIVEN a `ChangeNotifier` with exactly 3 registered listeners:
  - `listener[0]`: records that it was called (e.g. increments an `AtomicUsize`)
  - `listener[1]`: panics with `panic!("deliberate-test-panic")`
  - `listener[2]`: records that it was called
- WHEN `notify_listeners` is called (e.g. via a `ValueNotifier::set_value`)
- THEN `listener[0]` fires and increments its counter to 1
- AND `listener[1]` panics but the panic is caught by `catch_unwind`
- AND `listener[2]` fires and increments its counter to 1 (NOT skipped)
- AND exactly one `tracing::error!` event is emitted whose payload contains
  "deliberate-test-panic"
- AND the `ChangeNotifier` is NOT disposed (notifier state is consistent after
  the loop)

#### Scenario: tracing::error! captures the panic payload

- GIVEN a listener registered to a ChangeNotifier
- WHEN the listener panics with `panic!("specific-sentinel-string")`
- WHEN `notify_listeners` is called
- THEN a `tracing::error!` event is emitted with `?payload` capturing the panic
  value, which contains or can be downcasted to the string "specific-sentinel-string"

#### Scenario: Multiple panicking listeners do not prevent any non-panicking listener from firing

- GIVEN a ChangeNotifier with 5 listeners where listeners at indices 1 and 3 panic
- WHEN `notify_listeners` is called
- THEN listeners at indices 0, 2, and 4 all fire
- AND exactly 2 `tracing::error!` events are emitted
