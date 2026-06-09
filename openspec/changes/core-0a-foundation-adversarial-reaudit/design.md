# SDD Design — `core-0a-foundation-adversarial-reaudit`

| Field | Value |
|---|---|
| Phase | sdd-design |
| Change ID | `core-0a-foundation-adversarial-reaudit` |
| Chain run | `460923f1` |
| Artifact store | `openspec` (Engram unavailable) |
| skill_resolution | `paths-injected` (multi-agent, rust-ownership-system, clippy-configuration) |
| Status | ✅ Complete — 27 fix-in-this-change findings designed, 7 PR groups allocated |
| Peer-review broadcasts | F2/F6/F19 captured from exploration (Codex); F15 macro shape BROADCAST FAILED (subagent lacks shell/bash access) |

---

## Design overview

Seven self-contained PRs, ordered by severity (P0 soundness → P1 parity → P2/P3 idioms → diagnostics → tests), each ≤400 net LOC. Every PR follows strict-TDD: failing test commit → fix commit → refactor commit.

| PR | Theme | Findings | Crates changed | Estimated net LOC |
|---|---|---|---|---|
| PR-1 | Soundness cluster | F1, F2, F3, F7, F9, F23, F28 | `flui-foundation`, `flui-scheduler` | ~120 |
| PR-2 | Notifier cluster | F5, F6, F11, F16, F20, F26 | `flui-foundation` | ~160 |
| PR-3 | Cascade cluster | F8, F19, F24, F30 | `flui-tree` | ~160 |
| PR-4 | Edition-2024 idiom sweep | F4, F21, F22, F29 | `flui-foundation`, `flui-tree` | ~130 |
| PR-5 | Diagnostics cluster | F12, F15, F27 | `flui-foundation`, `flui-macros` | ~360 |
| PR-6 | Test gap cluster | F17, F18 | `flui-foundation` | ~65 |
| PR-7 | Slot bon builder | F14 | `flui-tree` | ~55 |

---

## Section 1 — Concrete implementation shapes

### PR-1: Soundness cluster (F1, F2, F3, F7, F9, F23, F28)

#### F1 — Remove `unsafe` from `Id<T>::from_raw`

**File:** `crates/flui-foundation/src/id.rs:213-217`

```rust
// BEFORE
#[inline]
pub const unsafe fn from_raw(raw: RawId) -> Self {
    Self(raw, PhantomData)
}

// AFTER
/// Constructs an `Id<T>` from a raw `RawId`.
///
/// This function is safe: `RawId` is a `NonZeroUsize` newtype that enforces
/// non-zero by construction; every valid `RawId` is a valid `Id<T>` for any
/// marker `T` (markers are uninhabited ZSTs carrying no invariants).
#[inline]
pub const fn from_raw(raw: RawId) -> Self {
    Self(raw, PhantomData)
}
```

**Caller impact:** Any `unsafe { Id::from_raw(raw) }` block becomes `Id::from_raw(raw)`. The `unsafe { ... }` wrapper is no longer required; Rust emits `unused_unsafe` lint (warning, not error) which is cleaned up in the same commit. The `serde` deserialize path in `id.rs:608` also simplifies.

**TDD shape:** No failing test needed (removing `unsafe` from a function is not behaviour-observable). Include a compile-test that confirms calling `from_raw` without `unsafe {}` compiles successfully (doc-comment example).

---

#### F2 — `Key::new()` UB: `fetch_update` sentinel pattern

**File:** `crates/flui-foundation/src/key.rs:138-160`

**Chosen shape: `fetch_update` sentinel** (see §2 tradeoff table and §6 cross-vendor log)

```rust
// AFTER
#[allow(clippy::new_without_default)]
#[inline]
pub fn new() -> Self {
    static COUNTER: AtomicU64 = AtomicU64::new(1);

    // DESIGN: 0 is the permanent-exhaustion sentinel.
    //
    // State machine under catch_unwind+retry:
    //  • counter = N (1..=u64::MAX-1): returns Ok(N), stores N+1. Key(N). ✓
    //  • counter = u64::MAX: returns Ok(MAX), stores 0 (exhausted). Key(MAX). ✓
    //  • counter = 0 (exhausted): closure returns None → Err(0) → .expect() panics.
    //    No mutation. No duplicate keys. No UB. ✓
    //  • second catch_unwind after panic at counter=0: same Err(0) → panic. ✓
    //
    // Without this shape: the old `fetch_add + new_unchecked` path would:
    //  - At counter=MAX: panic from `assert!(id != u64::MAX)`. Counter is now 0.
    //  - After catch_unwind+retry: counter=0, fetch_add returns 0, PASSES the old
    //    assert (0 != u64::MAX), calls new_unchecked(0) → **UB**.
    let id = COUNTER
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            if current == 0 {
                None // Exhausted sentinel: refuse update, return Err
            } else {
                Some(current.wrapping_add(1)) // u64::MAX → 0 marks exhaustion
            }
        })
        .expect("Key counter exhausted: 2^64 unique keys have been allocated");

    // `id` is the pre-increment value from fetch_update.
    // Since counter starts at 1 and only reaches 0 via MAX→0 wrapping,
    // `id` can never be 0 on this code path; the check is a safety net.
    Self(NonZeroU64::new(id).expect("Key::new invariant: counter returned 0"))
}
```

**Remove:** `#![allow(unsafe_code)]` from key.rs (or replace with `#![expect(unsafe_code, ...)]` if other unsafe remains; see F9).

**Test — `key_counter_exhaustion` (RED before fix, GREEN after):**
```rust
#[test]
fn key_counter_exhaustion() {
    // Simulate exhausted state by directly writing 0 to the static counter.
    // We can't reach u64::MAX organically in tests; write directly via
    // a test helper that exposes the counter for reset, or use a
    // separate isolated counter in a test-only constructor.
    //
    // Implementation choice: add a #[cfg(test)] Key::reset_counter_for_test()
    // that replaces the static with 0 and verifies the next call panics
    // without producing a zero-valued key.
    use std::panic::catch_unwind;
    Key::_test_force_exhausted_state(); // sets static COUNTER = 0
    let result = catch_unwind(|| Key::new());
    assert!(result.is_err(), "Key::new must panic when counter is exhausted");
    // Confirm no zero key was produced (test the non-UB invariant)
    // A second catch_unwind must also panic (no silent recovery):
    let result2 = catch_unwind(|| Key::new());
    assert!(result2.is_err(), "Key::new must keep panicking after exhaustion");
}
```

---

#### F3 — `UniqueKey::new()` overflow check

**File:** `crates/flui-foundation/src/key.rs:476-481`

```rust
// AFTER
pub fn new() -> Self {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    // Use same sentinel pattern as Key::new: 0 = permanent exhaustion.
    let id = COUNTER
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            if current == 0 {
                None
            } else {
                Some(current.wrapping_add(1))
            }
        })
        .expect("UniqueKey counter exhausted: 2^64 unique keys allocated");
    Self { id }
}
```

**Note:** `UniqueKey` uses a `u64` field (no `NonZero` niche), so no `NonZeroU64::new` call is needed here. The sentinel-0 means COUNTER reaches 0 only after wrapping from MAX. `id` returned by `fetch_update` is the pre-increment value; since COUNTER starts at 1 and wraps via MAX→0, `id` ranges 1..=u64::MAX (never 0). The exhaustion case panics without mutation.

---

#### F7 — `Id<T>` PhantomData: covariant → invariant

**File:** `crates/flui-foundation/src/id.rs` (struct definition)

```rust
// BEFORE
pub struct Id<T: Marker>(RawId, PhantomData<T>);

// AFTER
// PhantomData<fn() -> T> makes Id<T> invariant in T.
// Without invariance, a Marker sub-type relationship (if added later) would
// allow unsound Id<Sub> → Id<Super> coercions across tree layers.
// Current markers are all 'static, so the change is zero-observable today
// but necessary for soundness under any future parameterized marker.
pub struct Id<T: Marker>(RawId, PhantomData<fn() -> T>);
```

---

#### F9 — `#[allow(unsafe_code)]` → `#[expect(unsafe_code)]`

**Files:** `crates/flui-foundation/src/id.rs`, `crates/flui-foundation/src/key.rs`

```rust
// BEFORE (both files)
#![allow(unsafe_code)]

// AFTER: Replace with #[expect] so the lint fires if unsafe is fully removed
#![expect(
    unsafe_code,
    reason = "NonZeroUsize/NonZeroU64 invariants require unsafe blocks for \
              raw pointer construction; audit before removing"
)]
```

**Note:** After F1 removes `unsafe fn from_raw` and F2 removes `new_unchecked`, verify whether any `unsafe { ... }` blocks remain in each file. If id.rs has no remaining unsafe after F1+F7, remove the attribute entirely (the `#[expect]` will cause a compile warning "expectation unfulfilled" once no unsafe code remains, pointing to its own deletion). Same for key.rs after F2+F3.

---

#### F23 — I-10 closure: `RawId` / `Index` visibility downgrade + scheduler cleanup

**Files:** `crates/flui-foundation/src/id.rs`, `crates/flui-scheduler/src/id.rs`

```rust
// crates/flui-foundation/src/id.rs
// BEFORE
pub struct RawId(NonZeroUsize);
pub type Index = usize;

// AFTER: downgrade to crate-internal; public API uses `Id<T>` only
pub(crate) struct RawId(NonZeroUsize);
pub(crate) type Index = usize;
```

```rust
// crates/flui-scheduler/src/id.rs
// BEFORE (unused imports — the body never actually uses RawId or Index)
use flui_foundation::{RawId, Index, /* ... */};

// AFTER: remove the unused import lines
use flui_foundation::{/* only what is actually used */};
```

**Pre-condition:** confirm with `cargo check -p flui-scheduler` that `RawId`/`Index` names do not appear in the scheduler's impl bodies (proposal RK3 confirms this; verify before merge).

---

#### F28 — `Identifier` trait: remove `Into<Index>` supertrait bound

**File:** `crates/flui-foundation/src/id.rs` (Identifier trait definition)

```rust
// BEFORE
pub trait Identifier:
    Copy + Clone + Eq + PartialEq + Hash + Debug + Into<Index> + 'static
{
    fn get(self) -> Index;
    fn zip(index: Index) -> Self;
    fn try_zip(index: Index) -> Option<Self>;
}

// AFTER: remove Into<Index>; callers use .get() as the canonical path
pub trait Identifier:
    Copy + Clone + Eq + PartialEq + Hash + Debug + 'static
{
    fn get(self) -> Index;
    fn zip(index: Index) -> Self;
    fn try_zip(index: Index) -> Option<Self>;
}
```

**Keep:** `impl<T: Marker> From<Id<T>> for usize` convenience impl (standalone, not a trait bound).

**Caller migration:** Any `let n: usize = id.into()` → `let n: usize = id.get()`. A workspace grep for `.into()` on `Identifier`-typed values should find zero call sites after F30's `.get()` canonicalization in write.rs.

---

### PR-2: Notifier cluster (F5, F6, F11, F16, F20, F26)

#### F5 + F6 — `notify_listeners`: remove-during-notify + panic isolation

**File:** `crates/flui-foundation/src/notifier.rs`

These two fixes combine into a single rewrite of `notify_listeners`:

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Call all the registered listeners.
///
/// Snapshot semantics (Flutter parity):
/// - A snapshot of `(id, callback)` pairs is taken under lock before any
///   callback fires. The lock is released before iteration.
/// - Before each callback fires, the listener's registration is re-checked.
///   If the listener was removed during notify (e.g. a previous callback
///   called `remove_listener`), the callback is silently skipped (F5 fix).
/// - Each callback is wrapped in `catch_unwind(AssertUnwindSafe(|| ...))`.
///   If a callback panics, the panic payload is logged via `tracing::error!`
///   and iteration continues with the next listener (F6 fix).
///
/// # Concurrency
///
/// The snapshot is taken atomically under lock. Post-snapshot removals are
/// detected via the `contains_key` re-check. Post-snapshot additions are
/// NOT fired in the current notify cycle (same as Flutter).
pub fn notify_listeners(&self) {
    if self.check_disposed() {
        return;
    }

    // Snapshot: collect (ListenerId, callback) pairs.
    // SmallVec<[_; 4]> avoids heap allocation for the common ≤4-listener case.
    let snapshot: smallvec::SmallVec<[(ListenerId, ListenerCallback); 4]> = self
        .listeners
        .lock()
        .iter()
        .map(|(&id, cb)| (id, Arc::clone(cb)))
        .collect();

    for (id, callback) in &snapshot {
        // F5: re-check registration before firing.
        // Acquires lock briefly; releases before the callback runs.
        if !self.listeners.lock().contains_key(id) {
            continue; // Removed during notify; skip.
        }
        // F6: isolate each callback's panic.
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| callback())) {
            tracing::error!(
                listener_id = ?id,
                panic_payload = ?payload,
                "ChangeNotifier listener panicked; continuing with remaining listeners"
            );
        }
    }
}
```

**Tests (RED before fix, GREEN after):**

```rust
#[test]
fn removed_listener_does_not_fire_during_notify() {
    // Given: a ChangeNotifier with listener A and listener B.
    // Listener A removes listener B during its own callback.
    // When: notify_listeners() is called.
    // Then: listener B does NOT fire (post-removal skip).
    let notifier = ChangeNotifier::new();
    let fired_b = Arc::new(AtomicBool::new(false));
    let fired_b_clone = Arc::clone(&fired_b);
    let notifier_clone = notifier.clone();

    let id_b_cell = Arc::new(Mutex::new(None::<ListenerId>));
    let id_b_cell_clone = Arc::clone(&id_b_cell);

    let id_a = notifier.add_listener(Arc::new(move || {
        if let Some(id) = *id_b_cell_clone.lock() {
            notifier_clone.remove_listener(id);
        }
    }));

    let id_b = notifier.add_listener(Arc::new(move || {
        fired_b_clone.store(true, Ordering::SeqCst);
    }));
    *id_b_cell.lock() = Some(id_b);

    notifier.notify_listeners();
    assert!(!fired_b.load(Ordering::SeqCst), "removed listener must not fire");
    let _ = id_a;
}

#[test]
fn listener_fires_after_panic() {
    // Given: a ChangeNotifier with 3 listeners: panic-1, listener-2, listener-3.
    // When: notify_listeners() is called.
    // Then: listener-2 and listener-3 still fire despite listener-1 panicking.
    let notifier = ChangeNotifier::new();
    let fired_2 = Arc::new(AtomicBool::new(false));
    let fired_3 = Arc::new(AtomicBool::new(false));
    let (fired_2c, fired_3c) = (Arc::clone(&fired_2), Arc::clone(&fired_3));

    notifier.add_listener(Arc::new(|| panic!("intentional test panic")));
    notifier.add_listener(Arc::new(move || {
        fired_2c.store(true, Ordering::SeqCst);
    }));
    notifier.add_listener(Arc::new(move || {
        fired_3c.store(true, Ordering::SeqCst);
    }));

    notifier.notify_listeners(); // must not abort

    assert!(fired_2.load(Ordering::SeqCst), "listener-2 must fire after listener-1 panics");
    assert!(fired_3.load(Ordering::SeqCst), "listener-3 must fire after listener-1 panics");
}
```

---

#### F11 — Remove `impl Default for ValueNotifier<T>`

**File:** `crates/flui-foundation/src/notifier.rs`

```rust
// REMOVE this impl entirely:
// impl<T: Default> Default for ValueNotifier<T> {
//     fn default() -> Self {
//         Self::new(T::default())
//     }
// }
```

**Rationale:** `Default for ValueNotifier<T>` produces a notifier with a default-constructed value AND a fresh identity (no listeners). Two `ValueNotifier<u32>::default()` calls produce distinct notifiers with equal values — they are `==` by value but different identities. This violates the principle of least surprise: `PartialEq` on a listenable checks `value == other.value` but `Default` creates observably different objects. Flutter's `ValueNotifier` has no default constructor for the same reason.

**Downstream migration (flui-animation, 8 files):**  
Replace `ValueNotifier::<T>::default()` / `#[derive(Default)]` on structs with `ValueNotifier<T>` fields with explicit `ValueNotifier::new(T::default())` constructors. See §3 breaking change inventory.

---

#### F16 — SmallVec retention comment

**File:** `crates/flui-foundation/src/notifier.rs` (near the SmallVec import / listeners field declaration)

```rust
// ADD/IMPROVE comment explaining why SmallVec is used and why tinyvec was rejected:
// Inline-storage callback snapshot (audit I-4 / F24 parity).
// SmallVec<[_; 4]> chosen over tinyvec::ArrayVec because ListenerCallback
// is `Arc<dyn Fn()>` which does NOT implement `Default`, and tinyvec requires
// Default for all element types. SmallVec imposes no Default bound.
```

---

#### F20 — `check_disposed` cfg-explicit layout

**File:** `crates/flui-foundation/src/notifier.rs`

```rust
// AFTER: cfg-explicit; eliminates the misleading "debug_assert! + tracing::warn!
// in same block" pattern where the warn! was silently dead in debug builds.
#[inline]
fn check_disposed(&self) -> bool {
    if self.is_disposed.load(Ordering::Acquire) {
        // Debug: hard contract violation — panic immediately.
        #[cfg(debug_assertions)]
        panic!(
            "ChangeNotifier used after dispose: once dispose() has been \
             called, the notifier can no longer be used"
        );
        // Release: degrade gracefully with a warning (Flutter parity).
        #[allow(unreachable_code)]
        {
            tracing::warn!("ChangeNotifier used after dispose");
            return true;
        }
    }
    false
}
```

The `#[allow(unreachable_code)]` applies to the release-only block in debug mode where `panic!()` diverges. The intent is unambiguous: debug panics, release warns.

---

#### F26 — `println!` in doc-comment doctests

**File:** `crates/flui-foundation/src/lib.rs`, `crates/flui-foundation/src/notifier.rs`, any other doctests with `println!`

```rust
// BEFORE: doc-comment example
/// ```rust
/// use flui_foundation::notifier::{ChangeNotifier, Listenable};
/// let notifier = ChangeNotifier::new();
/// let id = notifier.add_listener(Arc::new(|| println!("Changed!")));
/// notifier.notify_listeners();
/// ```

// AFTER: remove println! — use a counter or comment
/// ```rust
/// use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
/// use flui_foundation::notifier::{ChangeNotifier, Listenable};
/// let notifier = ChangeNotifier::new();
/// let count = Arc::new(AtomicU32::new(0));
/// let count2 = Arc::clone(&count);
/// let _id = notifier.add_listener(Arc::new(move || {
///     count2.fetch_add(1, Ordering::Relaxed);
/// }));
/// notifier.notify_listeners();
/// assert_eq!(count.load(Ordering::Relaxed), 1);
/// ```
```

Similarly for `DiagnosticLevel` doctest in `debug.rs:16` (`println!("{}", level)`):
```rust
/// ```rust
/// use flui_foundation::DiagnosticLevel;
/// let level = DiagnosticLevel::Info;
/// assert!(level > DiagnosticLevel::Debug);
/// assert_eq!(level.as_str(), "info");
/// ```
```

**Sweep target:** `grep -rn 'println!' crates/flui-foundation/src/` to find all instances.

---

### PR-3: Cascade cluster (F8, F19, F24, F30)

#### F8 — Drop HRTB from `TreeReadExt` / `TreeNavExt`

**Files:** `crates/flui-tree/src/traits/read.rs`, `crates/flui-tree/src/traits/nav.rs`

```rust
// BEFORE
fn for_each<F>(&self, mut f: F)
where
    F: for<'a> FnMut(&'a Self::Node),

// AFTER: lifetime elision; the compiler infers the same bound
fn for_each<F>(&self, mut f: F)
where
    F: FnMut(&Self::Node),
```

Apply to every method in `TreeReadExt` and `TreeNavExt` that currently carries the `for<'a>` HRTB. The relaxation (removing over-constrained HRTB) is never breaking for callers.

---

#### F19 + F24 — Cascade cycle-detection + `try_remove` + SmallVec

**Files:** `crates/flui-tree/src/traits/write.rs`, `crates/flui-tree/src/error.rs`

**Step 1: Add `TreeError::CycleDetected` variant**

```rust
// crates/flui-tree/src/error.rs
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TreeError {
    // ... existing variants ...

    /// A cycle was detected during cascade traversal.
    ///
    /// This indicates a corrupted tree (a node's child chain eventually
    /// points back to an ancestor). Under the normal public API (`add_child`,
    /// `set_parent`) cycle creation is rejected at insertion time; this error
    /// fires as a defense-in-depth guard in the cascade walk, not as a
    /// first-line API validation.
    ///
    /// The `node_id` field records the first node whose ID was seen twice
    /// in the worklist during the depth-first pre-walk.
    #[error("cycle detected in cascade removal at node {node_id}")]
    CycleDetected {
        /// The node ID (as `usize`) at which the cycle was detected.
        node_id: usize,
    },
}
```

**Step 2: Add `try_remove` to `TreeWrite` trait**

```rust
// crates/flui-tree/src/traits/write.rs
// INLINE_TREE_DEPTH is re-exported from flui_tree::depth
use crate::depth::INLINE_TREE_DEPTH;

/// Removes a node and all its descendants with cycle-detection.
///
/// This is the semantic-carrying path: returns `Err(TreeError::CycleDetected)`
/// if a corrupted cycle is found during the cascade walk rather than OOM-ing.
///
/// [`remove`] calls this and maps the `Err` to `None` + `tracing::warn!`.
/// Callers that need to distinguish a genuine "node not found" (`Ok(None)`)
/// from a cycle error (`Err(CycleDetected)`) should call `try_remove` directly.
///
/// # Cycle detection
///
/// Uses a `HashSet<I>` visited set (O(1) per node, O(N) space).
/// `I: Hash + Eq` is already required by the `Identifier` supertrait.
///
/// # Worklist allocation
///
/// Both the to-visit stack and the collected worklist use
/// `SmallVec<[I; INLINE_TREE_DEPTH]>` (inline=32 entries) to avoid heap
/// allocation for typical shallow subtrees. Deeper subtrees spill to the heap.
fn try_remove(&mut self, id: I) -> Result<Option<Self::Node>, TreeError>
where
    Self: super::TreeNav<I> + Sized,
{
    use std::collections::HashSet;
    use smallvec::SmallVec;

    if !self.contains(id) {
        return Ok(None);
    }

    let mut worklist: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
    let mut to_visit: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
    let mut visited: HashSet<I> = HashSet::new();

    to_visit.push(id);
    while let Some(current) = to_visit.pop() {
        if !visited.insert(current) {
            // current was already visited: cycle detected.
            tracing::warn!(
                node_id = current.get(),
                "cycle detected in cascade removal; aborting traversal"
            );
            return Err(TreeError::CycleDetected { node_id: current.get() });
        }
        worklist.push(current);
        for child_id in self.children(current) {
            to_visit.push(child_id);
        }
    }

    // Post-order drain: leaves before parents, root last.
    let mut root_node: Option<Self::Node> = None;
    for node_id in worklist.into_iter().rev() {
        let removed = self.remove_shallow(node_id);
        if node_id == id {
            root_node = removed;
        }
    }
    Ok(root_node)
}
```

**Step 3: Update `remove` to delegate to `try_remove`**

```rust
fn remove(&mut self, id: I) -> Option<Self::Node>
where
    Self: super::TreeNav<I> + Sized,
{
    match self.try_remove(id) {
        Ok(node) => node,
        Err(e @ TreeError::CycleDetected { .. }) => {
            tracing::warn!(
                error = ?e,
                "TreeWrite::remove encountered a cycle; returning None. \
                 Use try_remove() to handle this case explicitly."
            );
            None
        }
        Err(e) => {
            tracing::error!(error = ?e, "TreeWrite::remove unexpected error");
            None
        }
    }
}
```

**F24 note:** The `Vec<I>` worklist and `to_visit` stack are replaced by `SmallVec<[I; INLINE_TREE_DEPTH]>` in the same edit. This closes F24 at zero additional LOC cost.

**Test — `cascade_cycle_detection` (RED before fix, GREEN after):**

```rust
#[test]
fn cascade_cycle_detection() {
    // Create a tree implementation with a deliberately corrupted cycle.
    // The test must complete without OOM or hang.
    // After fix: try_remove returns Err(TreeError::CycleDetected{..}).
    let mut tree = TestTree::new();
    let a = tree.insert(node("a"));
    let b = tree.insert(node("b"));
    let c = tree.insert(node("c"));
    // Valid tree: a → b → c
    tree.add_child(a, b).unwrap();
    tree.add_child(b, c).unwrap();
    // Corrupt: manually set c's child to a (bypasses public API cycle check)
    tree.corrupt_add_child(c, a); // test-only escape hatch
    // try_remove should detect cycle and return Err, not OOM/hang
    let result = tree.try_remove(a);
    assert!(matches!(result, Err(TreeError::CycleDetected { .. })));
    // remove() should return None with tracing::warn! (not OOM/hang)
    // (assert via tracing subscriber in test)
    tree.corrupt_add_child(c, a); // re-corrupt for second assertion
    let none_result = tree.remove(a);
    assert!(none_result.is_none());
}
```

---

#### F30 — `TreeWriteNav`: `.get()` canonical path, drop `Into<usize>` bound

**File:** `crates/flui-tree/src/traits/write.rs`

```rust
// BEFORE: move_children
fn move_children(&mut self, from: I, to: I) -> TreeResult<()>
where
    Self: Sized,
    I: Into<usize>, // ← redundant; .get() is canonical
{
    if !self.contains(from) {
        return Err(TreeError::not_found(from.into())); // ← Into<usize>
    }
    // ...
}

// AFTER
fn move_children(&mut self, from: I, to: I) -> TreeResult<()>
where
    Self: Sized, // dropped I: Into<usize>
{
    if !self.contains(from) {
        return Err(TreeError::not_found(from.get())); // ← .get() canonical
    }
    // ...
}
```

Apply same pattern to `insert_child` in the same trait.

---

### PR-4: Edition-2024 idiom sweep (F4, F21, F22, F29)

#### F4 — `BindingBase::instance()` CAS optimization

**File:** `crates/flui-foundation/src/binding.rs`

```rust
// BEFORE: in the steady-state path after initialization
instance_ref.initialized.store(true, Ordering::Release); // per-call Release on hot path

// AFTER: only perform the Release store on the 0→1 transition
let _ = instance_ref.initialized.compare_exchange(
    false, true, Ordering::Release, Ordering::Relaxed
);
// No-op if already true; avoids cache-line ping-pong in multi-core steady state.
```

---

#### F21 — `flui-tree` blanket `#![allow]` → per-site `#[expect]`

**File:** `crates/flui-tree/src/lib.rs` and individual trait files

```rust
// BEFORE (lib.rs top level):
#![allow(clippy::too_many_lines)]

// AFTER: remove blanket allow; add per-function expect with reason
// In write.rs, on the remove() method:
#[expect(
    clippy::too_many_lines,
    reason = "cascade walk is inherently a single logical operation; \
              splitting would obscure the post-order drain contract"
)]
fn remove(&mut self, id: I) -> Option<Self::Node>
```

Sweep all functions in `flui-tree` that triggered the blanket allow; add per-site `#[expect]` with reason strings.

---

#### F22 — `clippy::pedantic` alignment

**File:** `crates/flui-foundation/src/lib.rs`

```rust
// BEFORE (no pedantic warning)
// AFTER: add to match flui-tree's lint stack
#![warn(clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions, // acceptable for FLUI's module layout
    // add per-module suppressions as needed after running clippy::pedantic
)]
```

---

#### F29 — Delete reinvented `debug_assert_*` macros

**File:** `crates/flui-foundation/src/assert.rs`

**Step 1:** Grep workspace for all usages of `debug_assert_valid!`, `debug_assert_range!`, `debug_assert_finite!`, `debug_assert_not_nan!`.

**Step 2:** Replace each usage with stdlib `debug_assert!`. Example:
```rust
// BEFORE
debug_assert_valid!(n > 0, "n must be positive, got {n}");
// AFTER
debug_assert!(n > 0, "n must be positive, got {n}");
```

**Step 3:** Delete the macro definitions from `assert.rs`. If `assert.rs` becomes empty after deletion, delete the file and remove its `mod assert;` declaration from `lib.rs`.

---

### PR-5: Diagnostics cluster (F12, F15, F27)

#### F12 — `DiagnosticsPropertyKind` enum

**File:** `crates/flui-foundation/src/debug.rs`

```rust
/// The kind of a diagnostics property, determining how it is displayed.
///
/// Mirrors Flutter's typed `DiagnosticsProperty<T>` subclass hierarchy
/// (`EnumProperty`, `FlagProperty`, `IterableProperty`, etc.) but as an
/// enum variant instead of class inheritance.
///
/// The `Generic` variant is the fallback for all types not explicitly listed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticsPropertyKind {
    /// A generic property displayed as `{name}: {value:?}`.
    Generic,
    /// An enum property; `description` overrides the formatted value string.
    Enum {
        /// Optional human-readable description of the current enum variant.
        description: Option<std::borrow::Cow<'static, str>>,
    },
    /// A boolean flag property; displayed as `{name}` (true) or omitted (false).
    Flag,
    /// An iterable property; `count` is the number of elements.
    Iterable { count: usize },
    /// An optional reference; displayed as `{name}: <null>` when absent.
    OptionalRef,
    /// A stack of strings (e.g. stack traces).
    Stack,
    /// A double/float with an optional unit (e.g. `"dp"`, `"px"`).
    Double { unit: Option<std::borrow::Cow<'static, str>> },
    /// An integer with an optional unit.
    Int { unit: Option<std::borrow::Cow<'static, str>> },
    /// A color value (RGBA hex display).
    Color,
    /// An `Offset` / `Point2D` value.
    Offset,
    /// A `Rect` value.
    Rect,
    /// A `Size` value.
    Size,
}
```

Add a `kind` field to `DiagnosticsProperty`:
```rust
pub struct DiagnosticsProperty {
    pub name: String,
    pub value: String,
    pub level: DiagnosticLevel,
    pub kind: DiagnosticsPropertyKind,  // NEW
}
```

Keep `DiagnosticsProperty::new(name, value)` as the `Generic`-kind constructor for backwards compat.

---

#### F15 — `#[derive(Diagnosticable)]` proc-macro

**File:** `crates/flui-macros/src/lib.rs`

**Macro shape (see §6 for broadcast note):**

```rust
// In flui-macros:
#[proc_macro_derive(Diagnosticable, attributes(diagnostic))]
pub fn derive_diagnosticable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse the derive input.
    // Emit impl Diagnosticable for Struct:
    //   fn to_diagnostics_node(&self, name: &str) -> DiagnosticsNode {
    //     let type_short_name = std::any::type_name::<Self>()
    //         .rsplit("::")
    //         .next()
    //         .unwrap_or(std::any::type_name::<Self>());
    //     let mut builder = DiagnosticsNodeBuilder::new(name, type_short_name);
    //     for each field NOT marked #[diagnostic(skip)]:
    //       builder.add_property(stringify!(field_ident), format!("{:?}", self.field_ident));
    //     builder.build()
    //   }
    // ...
}
```

**Supported attributes:**
- `#[diagnostic(skip)]` — exclude a field from the diagnostics output.

**Scope cap:** The MVP macro ONLY handles named-field structs. Tuple structs, enums, and generics emit a compile error with a clear message: `"#[derive(Diagnosticable)] requires a named-field struct"`. Extending to enums is deferred.

**Test — `diagnosticable_derive_basic` (RED before fix, GREEN after):**
```rust
// In flui-macros tests:
#[derive(Diagnosticable)]
struct TestWidget {
    width: f32,
    height: f32,
    #[diagnostic(skip)]
    internal_id: u64,
}

#[test]
fn diagnosticable_derive_basic() {
    let widget = TestWidget { width: 100.0, height: 200.0, internal_id: 42 };
    let node = widget.to_diagnostics_node("TestWidget");
    assert_eq!(node.type_name(), "TestWidget"); // short name, not full path
    let props: Vec<_> = node.properties().collect();
    assert_eq!(props.len(), 2); // width + height, NOT internal_id
    assert!(props.iter().any(|p| p.name == "width"));
    assert!(props.iter().any(|p| p.name == "height"));
    assert!(!props.iter().any(|p| p.name == "internal_id")); // skipped
}
```

---

#### F27 — `type_name` strip in `to_diagnostics_node`

**File:** `crates/flui-foundation/src/debug.rs`

```rust
// In any place that uses std::any::type_name to produce display names:
// BEFORE
let type_name = std::any::type_name::<Self>();
// AFTER: strip module path, keep only the last segment
let type_name = std::any::type_name::<Self>()
    .rsplit("::")
    .next()
    .unwrap_or_else(|| std::any::type_name::<Self>());
// "flui_rendering::objects::render_padding::RenderPadding" → "RenderPadding"
```

**Breaking:** devtools / test matchers that string-match on full module paths will break. This is explicitly allowed per project lead mandate. Downstream grep: `grep -rn 'type_name\|diagnostics_node\|to_diagnostics_node' crates/` to identify affected tests before landing.

---

### PR-6: Test gap cluster (F17, F18)

#### F17 — `BindingBase` retry-after-panic test

**File:** `crates/flui-foundation/src/binding.rs` (tests module)

```rust
#[test]
fn instance_retries_after_panic() {
    // Given: a BindingBase where init_instances panics on first call.
    // When: a second call to instance() is made after catching the panic.
    // Then: the second call succeeds (OnceLock resumes uninitialized state
    //       after a panic in get_or_init — the value is NOT stored on panic).
    use std::panic::catch_unwind;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CALL_COUNT: AtomicU32 = AtomicU32::new(0);

    struct RetryBinding;
    impl BindingBase for RetryBinding {
        fn init_instances(&mut self) {
            let count = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            if count == 0 {
                panic!("simulated first-init failure");
            }
        }
    }

    static INSTANCE: OnceLock<RetryBinding> = OnceLock::new();

    // First call panics (init_instances panics).
    let result = catch_unwind(|| INSTANCE.get_or_init(|| {
        let mut b = RetryBinding;
        b.init_instances();
        b
    }));
    assert!(result.is_err(), "first init must panic");

    // Second call succeeds (OnceLock does not persist poison state).
    let instance = INSTANCE.get_or_init(|| {
        let mut b = RetryBinding;
        b.init_instances();
        b
    });
    assert!(std::ptr::eq(instance, INSTANCE.get().unwrap()));
}
```

---

#### F18 — `Id` boundary test at `usize::MAX`

**File:** `crates/flui-foundation/src/id.rs` (tests module)

```rust
#[test]
fn id_at_usize_max() {
    // NonZeroUsize can hold usize::MAX; the ID should be constructible.
    let id = ElementId::new(usize::MAX);
    assert_eq!(id.get(), usize::MAX);
    // Option<ElementId> uses the niche (zero), so Some(id) should be representable.
    let opt: Option<ElementId> = Some(id);
    assert!(opt.is_some());
    // Round-trip through from_raw / get.
    let raw = id.into_raw(); // or equivalent accessor
    let id2 = ElementId::from_raw(raw);
    assert_eq!(id, id2);
}
```

---

### PR-7: Slot bon builder (F14)

**File:** `crates/flui-tree/src/iter/slot.rs`

```rust
// BEFORE: Slot::with_siblings takes 5 positional args, two are
// indistinguishable Option<I> pairs — easy to swap prev/next silently.
pub fn with_siblings(
    parent: I,
    index: usize,
    depth: Depth,
    prev: Option<I>,
    next: Option<I>,
    first_child: Option<I>,
    last_child: Option<I>,
) -> Self { ... }

// AFTER: bon builder eliminates positional confusion
#[bon::builder]
pub fn with_siblings(
    parent: I,
    #[builder(default)]
    index: usize,
    depth: Depth,
    prev_sibling: Option<I>,
    next_sibling: Option<I>,
    first_child: Option<I>,
    last_child: Option<I>,
) -> Self { ... }

// Call sites change from:
//   Slot::with_siblings(parent, 1, depth, None, Some(next), None, None)
// to:
//   Slot::with_siblings()
//       .parent(parent)
//       .index(1)
//       .depth(depth)
//       .next_sibling(next)
//       .call()
```

---

## Section 2 — Alternatives considered and tradeoffs

### Decision D2-1: F2 — Key::new UB fix shape

**Context:** `Key::new()` uses `fetch_add + new_unchecked`. After counter wraps to 0 under catch_unwind, `new_unchecked(0)` is UB.

| Alternative | Shape | Status | Tradeoff |
|---|---|---|---|
| **A. `.expect()` partial fix** | Replace `new_unchecked(id)` with `NonZeroU64::new(id).expect(...)`. Add `assert!(id != 0)` before expect. | **Rejected** (Codex verdict) | Eliminates UB but introduces duplicate-key bug: after `MAX` panic+catch, retry returns `id=1` (COUNTER now at 2), which was already issued to a previous caller. Two distinct `Key::new()` calls return the same key value. Violates uniqueness contract. |
| **B. Saturating CAS loop** | After fetch_add returns `u64::MAX`, use compare_exchange to replace COUNTER with `u64::MAX` (saturate instead of wrap). | **Not chosen** | More complex than needed; saturating-at-MAX still panics correctly on the `MAX` return but doesn't prevent the "counter stays at MAX and wraps on next add" edge. Needs two atomic ops in the exhaustion path. |
| **C. `fetch_update` sentinel (CHOSEN)** | `fetch_update` uses closure: return `None` if current==0 (Err path → panic), else `Some(current.wrapping_add(1))`. 0 is permanent exhaustion sentinel. | **Adopted** (Codex verdict) | Eliminates UB. Prevents duplicate keys: after MAX→0 transition, all subsequent calls see current==0 → None → Err → panic. No mutation. Single atomic operation per call in steady state (same as original). Counter-0 state is permanent and self-consistent. |

**Verdict:** Alternative C. The design MUST NOT use Alternative A (Codex explicitly ruled it incomplete).

---

### Decision D2-2: F5 — Remove-during-notify: snapshot ID re-check vs. copy-on-write

| Alternative | Shape | Tradeoff |
|---|---|---|
| **A. ID snapshot re-check (CHOSEN)** | Snapshot `(id, callback)` pairs; re-check `contains_key(id)` before each callback | Simple. ~5 extra LOC in `notify_listeners`. Brief lock re-acquisition per callback (acceptable: lock is held only for the lookup, not during callback). |
| **B. Copy-on-write listener map** | Make `listeners` field an `Arc<HashMap<...>>`; on `remove_listener`, replace the Arc (COW). `notify_listeners` clones the Arc, not the HashMap. | Lower per-callback overhead (no lock re-acquisition). Higher implementation complexity. Requires `Arc::make_mut` semantics or explicit copy. Not worth the complexity for a listener map that's rarely >10 entries. |
| **C. Generation counter** | Add a `notify_generation: AtomicU64` to ChangeNotifier. Listeners removed mid-notify carry the old generation and are skipped by ID-based watermark. | Complex. Adds 8B per notifier. Doesn't help for the case where a listener removes a *different* listener. ID re-check is simpler and handles both cases. |

---

### Decision D2-3: F19 — Cascade cycle guard: HashSet vs linear scan vs step-cap-only

| Alternative | Shape | Tradeoff |
|---|---|---|
| **A. `HashSet<I>` visited set (CHOSEN)** | `HashSet::insert` returns false if duplicate; cycle detected. O(1) per node, O(N) space. | Requires `I: Hash + Eq` (already in `Identifier` bounds — confirmed). Optimal for large subtrees. Extra allocator call for small trees (mitigated by `SmallVec` below). |
| **B. `SmallVec<[I; N]>` linear scan** | Check `visited.contains(&current)` with O(N) scan per push. | Zero extra allocation for trees ≤N nodes. O(N²) for large trees — unacceptable for production subtrees of any size. Acceptable only as a fallback if `Hash` is not available. |
| **C. Step cap only** | `max_steps = len * 2`; abort if exceeded. | O(1) extra space. Misses cycles in very small cyclic components (cycle length ≤ 1). Also potentially wrong bound for valid subtrees with high branching factor. Codex rates this as "fail-closed backstop, not primary guard." |

**Verdict:** Alternative A (HashSet). `INLINE_TREE_DEPTH=32` SmallVec is used for `to_visit` and `worklist` to minimize allocator pressure for typical subtrees; the `HashSet` is a separate allocation but only used for corruption detection.

---

### Decision D2-4: F15 — Diagnosticable derive macro: field-by-field vs builder-pattern vs trait-object

| Alternative | Shape | Tradeoff |
|---|---|---|
| **A. Field-by-field `builder.add_property` (CHOSEN)** | Macro iterates struct fields, emits one `add_property(name, debug_fmt)` call per field. `#[diagnostic(skip)]` excludes fields. | Simple. ~200-300 LOC macro (under 400 budget). MVP scope: named-field structs only. Easy to extend. |
| **B. Custom `DiagnosticsPropertyKind` inference** | Macro inspects field types at compile time; emits appropriate `DiagnosticsPropertyKind` variant per field. | Higher LOC (type inference in proc-macro is complex). Would produce richer output (e.g. `Color` kind for `Color` fields) but requires mapping from type names to kinds — brittle. Defer to a post-MVP iteration. |
| **C. Runtime trait object** | Diagnosticable blanket impl that uses reflection/`Any` to collect field values at runtime. | No proc-macro needed. But Rust has no runtime reflection; requires explicit `Field` trait impls or `inventory!` registration. More complex. Proc-macro is the idiomatic approach. |

---

### Decision D2-5: F7 — PhantomData variance: `fn() -> T` vs `fn(T) -> ()` vs `*const T`

| Alternative | Shape | Tradeoff |
|---|---|---|
| **A. `PhantomData<fn() -> T>` — invariant (CHOSEN)** | `fn() -> T` makes `Id<T>` invariant in T. Most conservative: neither covariant nor contravariant. | Correct for an ID type: `Id<Sub>` must NOT be implicitly accepted where `Id<Super>` is expected (covariance would allow unsound casts if Marker hierarchy ever exists). |
| **B. `PhantomData<*const T>` — covariant, no Send/Sync** | Covariant in T; additionally strips `Send` and `Sync` from the impl. | Wrong direction: covariance allows `Id<Sub> → Id<Super>` substitution which we do NOT want. Also breaks `Send + Sync` which `Id<T>` must have (used across async boundaries). |
| **C. Keep `PhantomData<T>` — covariant** | Status quo. | Covariant in T. Today all markers are `'static` uninhabited ZSTs with no subtype relations, so observable behavior is identical. But violates the least-surprise principle for future parameterized markers. Not fixing it is a latent soundness risk. |

---

## Section 3 — Breaking change inventory

| Finding | Break | Downstream crates affected | Mechanical fix |
|---|---|---|---|
| **F5** (remove-during-notify) | Listeners removed mid-notify no longer fire in the current cycle | `flui-animation` (8 files using ChangeNotifier), any crate using self-removing listener as a "one-shot after current frame" pattern | **Grep:** `grep -rn 'remove_listener' crates/` inside listener closures. For each hit: determine if "fires-after-remove" is load-bearing. If yes: refactor to register once (use `remove_listener` outside the notify path). **Check required before PR-2 merge.** |
| **F11** (Default removal for ValueNotifier) | `ValueNotifier::<T>::default()` no longer compiles; `#[derive(Default)]` on structs with `ValueNotifier<T>` fields fails | `flui-animation` (8 files, per cycle-3 audit) | Replace each `ValueNotifier::default()` / `#[derive(Default)]` with explicit `new(T::default())` or custom `Default` impl. **Grep:** `grep -rn 'ValueNotifier' crates/ --include='*.rs' | grep -E 'Default|default()'`. **Migration must land in the same PR-2 commit.** |
| **F23** (RawId/Index visibility) | `RawId` and `Index` become `pub(crate)` in `flui-foundation`; any external crate that names these types fails compilation | `flui-scheduler` (imports cleaned up in PR-1) | Remove `use flui_foundation::{RawId, Index}` from `flui-scheduler/src/id.rs`. **Pre-condition:** confirm via `cargo check -p flui-scheduler` before merge. |
| **F27** (type_name strip) | `to_diagnostics_node` name field changes from `"flui_rendering::RenderFoo"` to `"RenderFoo"` | devtools/inspector text matchers, any tests asserting on full type name strings | **Explicitly permitted** per project lead mandate. Update any string-match tests found by: `grep -rn 'type_name\|to_diagnostics_node' crates/` to use short names. |
| **F28** (Into<Index> removal from Identifier) | `let n: usize = id.into()` no longer compiles when `id` is of an `Identifier` type | Anywhere in workspace that uses `.into()` on an Identifier value expecting `usize` | Replace `id.into()` with `id.get()`. The `From<Id<T>> for usize` impl stays as a convenience, so `usize::from(id)` still compiles for `Id<T>` specifically. Scope: workspace grep for `into()` on `Identifier`-constrained variables — expected to be zero or near-zero post-F30. |
| **F30** (TreeWriteNav Into<usize> bound drop) | `move_children` / `insert_child` no longer require `I: Into<usize>` bound | No callers are affected (bound relaxation); internal uses replaced with `.get()` | None — relaxation is always compatible. |

---

## Section 4 — PR sequencing

All PRs follow the strict-TDD discipline: `RED` (failing test or compile failure) → `GREEN` (fix) → `TRIANGULATE` (additional scenarios) → `REFACTOR` (cleanup). Each PR is independently mergeable with `just ci` passing.

### Dependency order

```
PR-1 (id.rs + key.rs soundness)
  └── PR-4 (idiom sweep — F9 depends on F1/F2 clearing unsafe from id.rs/key.rs)

PR-2 (notifier cluster — independent)

PR-3 (cascade cluster — independent)

PR-4 (idiom sweep — can land after PR-1; F22 requires PR-2's notifier changes for pedantic clean)

PR-5 (diagnostics — independent; F15 proc-macro depends on F12's DiagnosticsProperty types)
  F12 must be in same PR as F15, or F12 first.

PR-6 (test gap — independent)

PR-7 (slot builder — independent)
```

Recommended merge order: PR-1, PR-2, PR-3, PR-4, PR-6, PR-7, PR-5 (diagnostics last, largest).

### PR-1 strict-TDD evidence shape

| Step | Commit message | Files changed | What breaks/passes |
|---|---|---|---|
| RED | `test(id): add compile-test for safe from_raw` | `id.rs` | Adds doc-test that calls `Id::from_raw(raw)` without `unsafe {}` — compiler rejects (unsafe fn) |
| GREEN | `fix(id): remove unsafe from Id::from_raw (F1)` | `id.rs` | Doc-test compiles and passes |
| RED | `test(key): add key_counter_exhaustion regression test (F2)` | `key.rs` | Test fails: `catch_unwind(Key::new())` after simulated counter=0 produces 0-key (UB path) or hangs |
| GREEN | `fix(key): replace fetch_add+new_unchecked with fetch_update sentinel (F2)` | `key.rs` | Test passes |
| TRIANGULATE | `test(key): uniquekey exhaustion (F3) + Id boundary (separate from F18)` | `key.rs` | Additional edge cases |
| REFACTOR | `refactor(id,key): F7 PhantomData, F9 #[expect], F23 visibility, F28 Identifier (PR-1 sweep)` | `id.rs`, `key.rs`, `binding.rs`, `scheduler/id.rs` | `just ci` exits 0 |

### PR-2 strict-TDD evidence shape

| Step | Commit message | Files changed |
|---|---|---|
| RED | `test(notifier): add removed_listener_does_not_fire test (F5)` | `notifier.rs` |
| RED | `test(notifier): add listener_fires_after_panic test (F6)` | `notifier.rs` |
| GREEN | `fix(notifier): snapshot ID re-check + catch_unwind isolation (F5+F6)` | `notifier.rs` |
| GREEN | `fix(notifier): remove Default impl for ValueNotifier + migrate flui-animation (F11)` | `notifier.rs`, `flui-animation/**` |
| REFACTOR | `refactor(notifier): F16 SmallVec comment, F20 cfg-explicit check_disposed, F26 println! doctests` | `notifier.rs`, `lib.rs` |

### PR-3 strict-TDD evidence shape

| Step | Commit message | Files changed |
|---|---|---|
| RED | `test(tree): add cascade_cycle_detection regression test (F19)` | `write.rs`, test infra |
| GREEN | `feat(tree): add try_remove + HashSet cycle guard + SmallVec worklist (F19+F24)` | `write.rs`, `error.rs` |
| GREEN | `fix(tree): remove() delegates to try_remove() (F19)` | `write.rs` |
| REFACTOR | `refactor(tree): F8 HRTB drop (read.rs, nav.rs), F30 .get() canonical (write.rs)` | `read.rs`, `nav.rs`, `write.rs` |

### PR-5 dependency within PR

F12 must be added first (it introduces `DiagnosticsPropertyKind` and the `kind` field), then F15 (the derive macro uses `DiagnosticsNodeBuilder::add_property` from F12's type system). F27 can land in either F12 or F15 commit.

---

## Section 5 — `port-check.sh` additions

The following refusal-trigger-shaped patterns should become permanent CI gates. Add them to `scripts/port-check.sh` (create the file if absent):

### Gate 1 — `println!` in foundation/tree source (F26)

```bash
# Refusal trigger: Constitution Principle 6 — no println!/eprintln!/dbg! in shipped code.
# Covers doc-comment examples and inline code.
echo "==> Checking for println!/eprintln!/dbg! in foundation/tree source..."
if grep -rn 'println!\|eprintln!\|dbg!' \
    crates/flui-foundation/src/ \
    crates/flui-tree/src/ \
    crates/flui-macros/src/; then
    echo "FAIL: println!/eprintln!/dbg! found in source (Constitution Principle 6)"
    exit 1
fi
echo "PASS: no println!/eprintln!/dbg! in foundation/tree/macros source"
```

### Gate 2 — Module-level `#![allow(unsafe_code)]` (F9)

```bash
# Refusal trigger: #[allow] must be replaced with #[expect] (Rust 1.81+).
# Module-level allow(unsafe_code) is the banned pattern; per-site expect is required.
echo "==> Checking for blanket #![allow(unsafe_code)] at module level..."
if grep -rn '^#!\[allow(unsafe_code' \
    crates/flui-foundation/src/ \
    crates/flui-tree/src/; then
    echo "FAIL: module-level #![allow(unsafe_code)] found; use #![expect(..., reason=\"...\")]"
    exit 1
fi
echo "PASS: no blanket allow(unsafe_code) at module level"
```

### Gate 3 — Reinvented debug_assert macros (F29)

```bash
# Refusal trigger: custom debug_assert wrappers that duplicate stdlib.
echo "==> Checking for reinvented debug_assert macros..."
if grep -rn 'macro_rules! debug_assert_valid\|macro_rules! debug_assert_range\|macro_rules! debug_assert_finite\|macro_rules! debug_assert_not_nan' \
    crates/flui-foundation/src/; then
    echo "FAIL: reinvented debug_assert_* macros found; use stdlib debug_assert!"
    exit 1
fi
echo "PASS: no reinvented debug_assert macros"
```

### Gate 4 — `NonZeroU64::new_unchecked` in key.rs (F2)

```bash
# Refusal trigger: UB-capable unsafe construction of NonZero types in key.rs.
echo "==> Checking for new_unchecked in key.rs..."
if grep -n 'new_unchecked' crates/flui-foundation/src/key.rs; then
    echo "FAIL: NonZeroU64::new_unchecked in key.rs (UB risk)"
    exit 1
fi
echo "PASS: no new_unchecked in key.rs"
```

---

## Section 6 — Cross-vendor consultation log

### Broadcast 1: F2 — Key::new UB (Codex, captured exploration.md)

**Broadcast date:** 2026-05-24 (parent harness, post explore-step)  
**Vendor:** Codex (OpenAI gpt-5-codex)  
**Question:** Is `NonZeroU64::new_unchecked(0)` after catch_unwind actually UB? What is the correct fix shape?

**Codex verdict (summary):**
1. Yes, UB is valid — `new_unchecked(0)` is invalid per Rustonomicon §3.2, even if practically unreachable organically.
2. Yes, `catch_unwind` makes the overflow panic realistic in test harnesses / tokio tasks / plugin shells.
3. `NonZeroU64::new(id).expect(...)` partial fix is INCOMPLETE — eliminates UB but introduces duplicate keys after catch_unwind+retry.
4. **Correct shape: `fetch_update` sentinel.** Counter=0 is permanent exhaustion. Retries panic without mutation, UB, or duplicates.

**Design integration:** §1 F2 implementation uses the Codex-mandated `fetch_update` shape exclusively.

---

### Broadcast 2: F6 — Listener panic aborts notify (Codex, captured exploration.md)

**Broadcast date:** 2026-05-24 (parent harness, post explore-step)  
**Vendor:** Codex (OpenAI gpt-5-codex)  
**Question:** Is `catch_unwind(AssertUnwindSafe(|| callback()))` + `tracing::error!` the correct Rust idiom for listener panic isolation?

**Codex verdict (summary):**
1. Yes, finding is valid — current FLUI diverges from Flutter.
2. `AssertUnwindSafe` is justified because `ChangeNotifier` itself has no borrowed mutable invariants across the unwind boundary.
3. `set_hook` is not an alternative (global, observational, cannot resume iteration).
4. Fix confirmed correct. Mandate regression test: "listener 2 fires after listener 1 panics."

**Design integration:** §1 F5+F6 `notify_listeners` implementation uses the Codex-confirmed shape. The mandatory regression test `listener_fires_after_panic` is included in PR-2.

---

### Broadcast 3: F19 — Cascade cycle OOM (Codex, captured exploration.md)

**Broadcast date:** 2026-05-24 (parent harness, post explore-step)  
**Vendor:** Codex (OpenAI gpt-5-codex)  
**Question:** Is F19 truly P0? What is the correct fix shape for cascade cycle-detection?

**Codex verdict (summary):**
1. Valid finding. Not P0 unless corrupted storage is attacker-reachable via deserialization/plugin/FFI. Severity: P1 (defense-in-depth).
2. `HashSet<I>` visited set preferred (O(N) space, O(1) per node) if `I: Hash + Eq` available — confirmed available in `Identifier` bounds.
3. Step cap (`len * 2`) is a fail-closed backstop, not a primary guard.
4. `None` vs `Err`: add `try_remove(...) -> Result<Option<Node>, TreeError>` and have `remove` delegate. Preserves existing `Option<Node>` public contract.
5. Guard location: put in trait default. `remove_shallow` cannot detect cascade cycles.

**Design integration:** §1 F19 design uses HashSet visited set, `try_remove` + `remove` delegation, `TreeError::CycleDetected` variant. Severity is recorded as P1 in this design.

---

### Broadcast 4: F15 — Diagnosticable derive macro shape (BROADCAST FAILED)

**Broadcast date:** 2026-05-25 (this design session)  
**Intended vendor:** Codex (OpenAI gpt-5-codex) via `codex exec`  
**Question:** Should the `#[derive(Diagnosticable)]` proc-macro emit field-by-field `add_property` calls (MVP) or attempt type-aware `DiagnosticsPropertyKind` inference at derive time?

**Status: BROADCAST FAILED** — This subagent session lacks shell/bash access (`codex exec` unavailable). Analysis proceeds on single-model basis.

**Single-model analysis:**
- Field-by-field `add_property` (Alternative A in §2 D2-4) is the standard proc-macro derive MVP pattern. It is bounded in scope, testable, and composable with F12's `DiagnosticsPropertyKind` enum.
- Type-aware kind inference at derive time is complex, brittle (requires matching on type name strings), and not needed for the initial use case (which is just "show all fields in devtools").
- **Verdict: Alternative A (field-by-field, `#[diagnostic(skip)]` only).** Type-aware kind inference is a future iteration.

**Recommendation for supervisor:** If this decision should receive a Codex review before sdd-apply begins on PR-5, please re-broadcast from a shell-capable session. See §8 open questions.

---

## Section 7 — Risk register

| ID | Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|---|
| RD1 | **F5 behavioral break: listeners removed inside a callback ("one-shot" pattern)** — callers relying on "removed listener fires once, then self-unregisters" behavior (the fire happens before remove takes effect) may break. | Medium | High | Pre-PR-2 grep: `grep -rn 'remove_listener' crates/ --include='*.rs'` inside listener closure bodies. If any "one-shot" pattern found, replace with a `bool` flag inside the closure (`let already_fired = AtomicBool::new(false); if !already_fired.swap(true, ...)`) rather than remove-during-notify. |
| RD2 | **F11 Default removal: flui-animation compile break** — 8 files use ChangeNotifier/ValueNotifier, may derive `Default` on structs with `ValueNotifier<T>` fields. | Medium | High | Migration must land in same PR-2 commit. Grep: `grep -rn 'ValueNotifier' crates/flui-animation/`. For each hit that uses `#[derive(Default)]`: replace with explicit impl. Since flui-animation is disabled (`workspace = false` in root Cargo.toml or excluded from default-members), the compile failure is caught by `just check --workspace`, not `just build`. Confirm scope. |
| RD3 | **F23 RawId/Index visibility: scheduler API break** — if `flui-scheduler/src/id.rs` actually uses `RawId`/`Index` in impl bodies (not just in unused imports). | Low | Medium | Pre-merge `cargo check -p flui-scheduler` confirms zero compilation errors. If any use is found, keep the types `pub(crate)` but add a `pub(crate) type SchedulerIndex = flui_foundation::...` re-export in foundation. |
| RD4 | **F2 fetch_update edge: off-by-one in sentinel logic** — misreading fetch_update return value semantics (it returns Ok(old_value), not Ok(new_value)). | Low | Critical | The implementation comment explicitly documents: "id is the pre-increment value." The `key_counter_exhaustion` regression test validates the post-MAX state. Code review checklist: verify the comment and the `NonZeroU64::new(id)` call. |
| RD5 | **F15 proc-macro scope creep** — `#[derive(Diagnosticable)]` expansion may exceed 400 LOC if attempting to handle generics, enums, or complex attribute parsing. | Medium | Medium | Cap scope at named-field structs only. Generics, enums, tuple structs → compile error with clear message. LOC budget: 200-300 LOC. If expansion grows beyond 350 LOC, split F12 (DiagnosticsPropertyKind) and F15 (derive macro) into two sub-PRs within PR-5. |
| RD6 | **F19 HashSet `I: Hash + Eq` bound** — If the `Identifier` supertrait does NOT have `Hash + Eq`, the `HashSet<I>` approach requires a new bound. | Very Low | High | **Pre-confirmed:** `Identifier` bounds include `Hash + Eq` (per exploration.md §Risks and proposal §RK6). Verified by reading `crates/flui-foundation/src/id.rs` Identifier definition. Risk is residual. |
| RD7 | **F29 prelude breakage for external consumers** — external crates using `flui_foundation::prelude::debug_assert_valid!` fail to compile after macro deletion. | Very Low | Low | This is a workspace-internal crate (no published crate version). Workspace grep for macro names outside `flui-foundation/src/` is clean per proposal §RK7. |
| RD8 | **F8 HRTB relaxation reveals downstream inference failures** — removing `for<'a>` from TreeReadExt/TreeNavExt may cause some complex closure inference to fail if the compiler previously relied on the explicit lifetime annotation to choose between closures. | Low | Medium | Test by running `cargo check --workspace --all-targets` after F8 change before merging. If inference regressions appear, add explicit lifetime annotations at the call site rather than restoring the HRTB. |
| RD9 | **F20 check_disposed unreachable_code warning** — the `#[allow(unreachable_code)]` suppression in the release-only block may mask genuine unreachable code in future refactors. | Low | Low | The `#[allow(unreachable_code)]` is tightly scoped to the `{ tracing::warn!(...); return true; }` block. Document in comment: "release-only; unreachable in debug builds after cfg(debug_assertions) panic! above." |

---

## Section 8 — Open questions for supervisor

### OQ1 — F15 macro peer review

The `Diagnosticable` derive macro shape (§1 F15, §2 D2-4) was not peer-reviewed by Codex due to shell access unavailability in this subagent. The design uses field-by-field `add_property` with `#[diagnostic(skip)]` as the MVP scope.

**Question:** Should PR-5 (Diagnostics cluster) wait for a Codex review of the macro shape before sdd-apply begins? If yes, please re-broadcast the F15 prompt from a shell-capable session (see §6 Broadcast 4 for the question text).

**Default if no action:** Proceed with field-by-field MVP shape as designed. Post-PR-5 review can course-correct if needed (derive macro is additive; changing the expansion format does not break callers, only re-derive is needed).

### OQ2 — F11 flui-animation scope confirmation

The proposal notes flui-animation is DISABLED (per AGENTS.md: `flui-animation — DISABLED until integration`). The workspace root `Cargo.toml` may exclude it from default-members.

**Question:** Does `just ci` (i.e. `cargo test --workspace`) compile `flui-animation`? If not, the F11 migration in PR-2 may not catch flui-animation compile errors without an explicit `cargo check -p flui-animation`.

**Recommended action:** PR-2 should include `cargo check -p flui-animation` as an explicit verify step, or the task notes should confirm that flui-animation is excluded from `--workspace` and the break is deferred.

### OQ3 — port-check.sh: create new or amend existing?

The design proposes four new port-check.sh gates (§5). It is unclear whether `scripts/port-check.sh` already exists and has other gates, or whether this design should create it from scratch.

**Question:** Does `scripts/port-check.sh` already exist? If yes, the gates in §5 should be appended. If no, a new file should be created with a header and the four gates. The sdd-tasks phase should clarify.

---

## Section 9 — File change summary (all PRs)

| File | PRs | Change type |
|---|---|---|
| `crates/flui-foundation/src/id.rs` | PR-1 | F1 (safe from_raw), F7 (PhantomData), F9 (#[expect]), F23 (pub(crate)), F28 (Identifier) |
| `crates/flui-foundation/src/key.rs` | PR-1 | F2 (fetch_update), F3 (UniqueKey), F9 (#[expect]) |
| `crates/flui-foundation/src/notifier.rs` | PR-2 | F5 (re-check), F6 (catch_unwind), F11 (Default removal), F16 (comment), F20 (cfg-explicit), F26 (println! cleanup) |
| `crates/flui-foundation/src/binding.rs` | PR-4, PR-6 | F4 (CAS), F17 (test) |
| `crates/flui-foundation/src/debug.rs` | PR-5 | F12 (DiagnosticsPropertyKind), F27 (type_name strip) |
| `crates/flui-foundation/src/assert.rs` | PR-4 | F29 (delete macros) |
| `crates/flui-foundation/src/lib.rs` | PR-2, PR-4 | F22 (clippy::pedantic), F26 (doctest cleanup) |
| `crates/flui-tree/src/traits/write.rs` | PR-3 | F19 (try_remove + HashSet + SmallVec), F24 (SmallVec), F30 (.get()) |
| `crates/flui-tree/src/traits/read.rs` | PR-3 | F8 (HRTB drop) |
| `crates/flui-tree/src/traits/nav.rs` | PR-3 | F8 (HRTB drop) |
| `crates/flui-tree/src/error.rs` | PR-3 | F19 (CycleDetected variant) |
| `crates/flui-tree/src/iter/slot.rs` | PR-7 | F14 (#[bon::builder]) |
| `crates/flui-tree/src/lib.rs` | PR-4 | F21 (per-site #[expect]) |
| `crates/flui-tree/src/depth.rs` | PR-3 (read-only) | INLINE_TREE_DEPTH constant used for SmallVec sizing |
| `crates/flui-macros/src/lib.rs` | PR-5 | F15 (Diagnosticable derive macro) |
| `crates/flui-scheduler/src/id.rs` | PR-1 | F23 (remove unused imports) |
| `crates/flui-animation/**` | PR-2 | F11 migration (Default → explicit new) |
| `scripts/port-check.sh` | PR-4 (or standalone) | F26/F9/F29/F2 CI gates (see §5) |

---

*End of design.md. Proceed to `tasks.md`. The sdd-apply agent may lift implementation shapes directly from §1. Codex F2/F6/F19 verdicts are captured in §6 and MUST be honored — do not revert to `.expect()` partial fix for F2 or omit the `try_remove` delegation for F19.*
