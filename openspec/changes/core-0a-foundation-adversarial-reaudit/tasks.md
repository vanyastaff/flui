# SDD Tasks — `core-0a-foundation-adversarial-reaudit`

| Field | Value |
|---|---|
| Phase | sdd-tasks |
| Change ID | `core-0a-foundation-adversarial-reaudit` |
| Chain run | `460923f1` |
| Artifact store | `openspec` (Engram unavailable) |
| skill_resolution | `paths-injected` (multi-agent, clippy-configuration) |
| Status | ✅ Complete — 7 PRs, 27 findings, strict-TDD units |
| strict_tdd | **true** — RED → GREEN → TRIANGULATE → REFACTOR mandatory |

---

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~830–1 050 net (additions + deletions across all 7 PRs) |
| 400-line budget risk | Low |
| Chained PRs recommended | Yes (7 self-contained PRs, ordered by dependency) |
| Suggested split | PR-1 → PR-2 → PR-3 → PR-4 → PR-6 → PR-7 → PR-5 |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

| PR | Theme | Est. +add | Est. -del | Est. net | Cap risk |
|----|-------|-----------|-----------|----------|----------|
| PR-1 | Soundness cluster | ~80 | ~30 | ~50–80 | Low |
| PR-2 | Notifier cluster | ~130 | ~40 | ~90–120 | Low |
| PR-3 | Cascade cluster | ~150 | ~30 | ~100–160 | Low |
| PR-4 | Edition-2024 idiom sweep | ~90 | ~105 | ~80–130 | Low |
| PR-5 | Diagnostics cluster | ~360 | ~20 | ~340–380 | **Medium — near 400-cap** |
| PR-6 | Test gap cluster | ~65 | ~0 | ~65 | Low |
| PR-7 | Slot bon builder | ~55 | ~10 | ~45–55 | Low |
| **Total** | — | **~930** | **~235** | **~770–1 025** | **Low vs 4 000-line budget** |

> **PR-5 note:** Estimated at ~360–380 net LOC; if the `Diagnosticable` derive macro expansion grows beyond 380 LOC, split F12 (enum definition + `kind` field) into a sub-PR and land F15 (proc-macro) + F27 (type_name) as PR-5b. The macro scope cap is `#[diagnostic(skip)]` + named-field structs only — do not extend to enums/generics without splitting.

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low
```

---

## Workload-driven scope reduction recommendation

Total estimated work (~1 050 LOC) is **well within** the 4 000-line session budget. No scope reduction is required. All 27 findings proceed as designed. The only per-PR caution is PR-5 approaching the 400-line cap — see the PR-5 note above.

---

## Dependency Order

```
PR-1 (id.rs + key.rs soundness)
  └── PR-4 (F9 #[expect] depends on F1+F2 clearing unsafe from id.rs/key.rs)

PR-2 (notifier cluster — independent)
  └── (PR-4 F22 clippy::pedantic may surface new pedantic warnings in notifier.rs;
       run after PR-2 if needed, or include suppressions in PR-4)

PR-3 (cascade cluster — independent)

PR-4 (idiom sweep — land after PR-1; OQ3 confirmed: APPEND to existing port-check.sh)

PR-5 (diagnostics — independent; F12 must be committed before F15 in same PR)

PR-6 (test gap — independent)

PR-7 (slot builder — independent)
```

**Recommended merge sequence:** PR-1 → PR-2 → PR-3 → PR-4 → PR-6 → PR-7 → PR-5

---

## Strict TDD Evidence Requirements

The `sdd-apply` agent **MUST** record per unit:

1. **RED evidence** — test output showing the test fails before the fix (compile error or assertion failure).
2. **GREEN diff** — minimal commit diff that makes the test pass.
3. **TRIANGULATE evidence** — additional test(s) run green after the fix.
4. **REFACTOR diff** (optional) — cleanup commit; tests still green.

Exceptions allowed:
- Doc-only sub-findings (F16, F20, F26 within PR-2 U3) do not require a RED test; they are bundled with the cluster's GREEN and REFACTOR commits.
- F1 (removing `unsafe` from a function) uses a compile-test (doc-test), not a runtime test.
- F7, F9, F23, F28, F30 are structural edits that may be batched in a single REFACTOR commit within their PR if the cluster's primary GREEN already passes.

---

## Cross-Vendor Verdict Integration Check

**F2 — `Key::new` fix shape (Codex-mandated):**
The apply agent MUST use the `fetch_update` sentinel pattern. The `.expect()` partial fix (replacing `new_unchecked` with `NonZeroU64::new(id).expect(...)`) is **explicitly rejected** — it eliminates UB but introduces duplicate keys after `catch_unwind` + retry (Codex verdict, design §6 Broadcast 1, §2 D2-1).

Verification: `cargo test -p flui-foundation key_counter_exhaustion` must pass AND `! grep -n "new_unchecked" crates/flui-foundation/src/key.rs` exits 0.

**F19 — Cascade `remove` fix shape (Codex-mandated):**
The apply agent MUST implement `try_remove(id: I) -> Result<Option<Self::Node>, TreeError>` as a new `TreeWrite` default method, and `remove(id: I) -> Option<Self::Node>` MUST delegate to `try_remove`, returning `None` + `tracing::warn!` on `Err(TreeError::CycleDetected)`. A `None`-on-cycle-only implementation without `try_remove` is **not acceptable** (Codex verdict, design §6 Broadcast 3, §2 D2-3).

Verification: `cargo test -p flui-tree cascade_cycle_detection` must pass AND `grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs` exits 0.

---

## PR-1 — Soundness Cluster

**Title:** `fix(foundation): soundness fixes — safe from_raw, fetch_update Key counter, UniqueKey overflow, PhantomData invariance`

**Scope:** Eliminate UB in `Key::new`, remove vacuous `unsafe fn`, add overflow guard to `UniqueKey::new`, make `Id<T>` invariant in `T`, apply structural sweep (F9/F23/F28) to id.rs + key.rs + scheduler imports.

**Findings closed:** F1, F2, F3, F7, F9, F23, F28

**Files touched:**
- `crates/flui-foundation/src/id.rs` — F1, F7, F9, F23, F28
- `crates/flui-foundation/src/key.rs` — F2, F3, F9
- `crates/flui-scheduler/src/id.rs` — F23 (remove unused imports)

---

### U1 — F1: Remove `unsafe` from `Id<T>::from_raw`

**Files:** `crates/flui-foundation/src/id.rs`

**RED test (compile-test):**
Add a doc-comment example to `from_raw` that calls it without `unsafe {}`:
```rust
/// ```rust
/// use flui_foundation::id::{Id, RawId};
/// // This must compile without an `unsafe` block.
/// // (Before the fix, the compiler rejects this because from_raw is `unsafe fn`.)
/// let raw = RawId::new(std::num::NonZeroUsize::new(1).unwrap());
/// let _id: Id<()> = Id::from_raw(raw);
/// ```
```
Run `cargo test -p flui-foundation --doc` — expect compile error on the doc-test before the fix.

**GREEN:** Remove the `unsafe` keyword from `pub unsafe fn from_raw`. Clean up any `unsafe { Id::from_raw(...) }` call sites in the same file (e.g., serde deserialize path at id.rs:608). Run `cargo test -p flui-foundation --doc` — doc-test now compiles and passes.

**TRIANGULATE:** Search for all `unsafe { Id::from_raw(` usages in the workspace; confirm they can be simplified (replace with `Id::from_raw(`)

**REFACTOR:** Update any `// SAFETY:` comment block that was guarding the old `from_raw` call to remove the safety justification and note the call is now safe.

---

### U2 — F2: `Key::new()` — `fetch_update` sentinel pattern

**Files:** `crates/flui-foundation/src/key.rs`

**RED test:**

Add `Key::_test_force_exhausted_state()` (cfg(test) only) that writes `0` directly to the static `COUNTER`, then write:

```rust
#[test]
fn key_counter_exhaustion() {
    use std::panic::catch_unwind;
    // Force counter into the permanent-exhaustion sentinel state (0).
    Key::_test_force_exhausted_state();
    // First call after exhaustion MUST panic.
    let r1 = catch_unwind(Key::new);
    assert!(r1.is_err(), "Key::new must panic when counter is at sentinel 0");
    // Second call must also panic — no silent recovery.
    let r2 = catch_unwind(Key::new);
    assert!(r2.is_err(), "Key::new must keep panicking after exhaustion (permanent sentinel)");
    // Neither result must be Ok(Key(0)) — confirm no zero-value key was produced.
    assert!(!r1.is_ok());
    assert!(!r2.is_ok());
}
```

Run `cargo test -p flui-foundation key_counter_exhaustion` — fails (old `fetch_add + new_unchecked` either panics differently or passes the zero-key through).

**GREEN:** Replace `fetch_add(1, Ordering::Relaxed) + new_unchecked` with the `fetch_update` sentinel shape from design §1 F2. The counter starts at 1; when it wraps from `u64::MAX` to 0, the closure returns `None` → `Err(0)` → `.expect()` panics. The state machine invariants must be in inline comments exactly as specified in design §1 F2. Run `cargo test -p flui-foundation key_counter_exhaustion` — passes.

**TRIANGULATE:**
- `key_uniqueness`: create N keys; assert all `u64` values are distinct (small N test).
- Confirm `NonZeroU64::new(id).expect("Key::new invariant: counter returned 0")` safety net is present.

**REFACTOR:** Remove `#![allow(unsafe_code)]` from key.rs if no `unsafe` blocks remain after this unit (see U4-F9 which will switch to `#[expect]` if needed).

---

### U3 — F3: `UniqueKey::new()` overflow guard

**Files:** `crates/flui-foundation/src/key.rs`

**RED test:**
```rust
#[test]
fn uniquekey_exhaustion_panics() {
    use std::panic::catch_unwind;
    UniqueKey::_test_force_exhausted_state(); // cfg(test) only, sets COUNTER to 0
    let r = catch_unwind(UniqueKey::new);
    assert!(r.is_err(), "UniqueKey::new must panic when counter is exhausted");
}
```
Run — fails (old code does not guard against counter=0).

**GREEN:** Replace `UniqueKey::new` with the same `fetch_update` sentinel pattern (counter starts at 1, 0 = permanent exhaustion). `UniqueKey` uses `u64` field (no `NonZero` niche), so `id` returned is the pre-increment value, guaranteed 1..=u64::MAX on this path.

**TRIANGULATE:** `uniquekey_uniqueness` — create N unique keys; assert all `id` values are distinct.

---

### U4 — F7 + F9 + F23 + F28: Structural sweep

**Files:** `crates/flui-foundation/src/id.rs`, `crates/flui-foundation/src/key.rs`, `crates/flui-scheduler/src/id.rs`

This unit is a batched REFACTOR commit. No new RED tests are required; the PR-1 tests from U1–U3 cover soundness. The structural changes must not break any existing tests.

**F7 — PhantomData invariance:**
Change `PhantomData<T>` → `PhantomData<fn() -> T>` in the `Id<T>` struct definition.
Commit message: `refactor(id): make Id<T> invariant in T via PhantomData<fn() -> T> (F7)`

**F9 — `#[allow]` → `#[expect]`:**
After U1 removes the `unsafe fn from_raw` and U2 replaces `new_unchecked`, verify which files still have `unsafe` blocks:
- id.rs: audit remaining `unsafe { ... }` blocks (e.g., any `unsafe impl` for `Send`/`Sync`). If unsafe remains, replace `#![allow(unsafe_code)]` with `#![expect(unsafe_code, reason = "...")]`. If no unsafe remains, delete the attribute entirely.
- key.rs: same audit. After `fetch_update` replaces `new_unchecked`, if no `unsafe` blocks remain, delete the attribute. If `unsafe` remains, use `#[expect]`.

**F23 — Visibility downgrade:**
- In `crates/flui-foundation/src/id.rs`: downgrade `pub struct RawId(...)` → `pub(crate)` and `pub type Index = usize` → `pub(crate) type Index = usize`.
- In `crates/flui-scheduler/src/id.rs`: remove `use flui_foundation::{RawId, Index, ...}` import lines that are unused.
- **Pre-condition (mandatory):** Run `cargo check -p flui-scheduler` before committing F23. If compilation errors appear, investigate and add a `pub(crate)` re-export to foundation if needed.

**F28 — `Identifier` trait: drop `Into<Index>` supertrait:**
Remove `+ Into<Index>` from the `Identifier` trait definition. Keep the standalone `impl<T: Marker> From<Id<T>> for usize` convenience impl. Grep workspace for `.into()` on `Identifier`-constrained variables and replace with `.get()`.

**Verify (whole unit):** `cargo check --workspace --all-targets` exits 0. Then `just ci` exits 0.

---

### PR-1 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| `flui-scheduler` | F23 import removal | `cargo check -p flui-scheduler` pre-merge gate |
| All crates using `Id::from_raw` with `unsafe { }` | `unused_unsafe` lint → warning | Clean up in same commit as U1 GREEN |
| All crates using `id.into()` expecting `usize` | F28 bound removal | `grep -rn '\.into()' crates/` on Identifier-typed vars; replace with `.get()` |

### PR-1 Verification commands

```bash
cargo test -p flui-foundation key_counter_exhaustion
cargo test -p flui-foundation key_uniqueness
cargo test -p flui-foundation uniquekey_exhaustion_panics
cargo test -p flui-foundation uniquekey_uniqueness
cargo test -p flui-foundation --doc   # from_raw doc-test
cargo check -p flui-scheduler         # F23 pre-merge gate
! grep -n "new_unchecked" crates/flui-foundation/src/key.rs
! grep -n "unsafe fn from_raw" crates/flui-foundation/src/id.rs
grep -n "PhantomData<fn() -> T>" crates/flui-foundation/src/id.rs
! grep -n "Into<Index>" crates/flui-foundation/src/id.rs  # not in trait definition
just ci
```

---

## PR-2 — Notifier Cluster

**Title:** `fix(foundation): notify semantics — snapshot re-check, panic isolation, Default removal, cfg-explicit dispose`

**Scope:** Bring `ChangeNotifier::notify_listeners` to Flutter parity (F5 + F6), remove the surprising `Default for ValueNotifier<T>` (F11), and apply housekeeping to the notifier module (F16 SmallVec comment, F20 cfg-explicit `check_disposed`, F26 doctest `println!` removal).

**Findings closed:** F5, F6, F11, F16, F20, F26

**Files touched:**
- `crates/flui-foundation/src/notifier.rs` — F5, F6, F11, F16, F20, F26
- `crates/flui-foundation/src/lib.rs` — F26 (doctest cleanup)
- `crates/flui-animation/**` — F11 migration (grep-driven)

---

### U1 — F5 + F6: Snapshot re-check + `catch_unwind` isolation

**Files:** `crates/flui-foundation/src/notifier.rs`

**RED tests (two failing tests in one commit):**

```rust
// Test 1 — F5: removed listener must not fire
#[test]
fn removed_listener_does_not_fire_during_notify() { ... }

// Test 2 — F6: listeners after a panicking one must still fire
#[test]
fn listener_fires_after_panic() { ... }
```

Full test bodies are specified in design §1 F5+F6. Run `cargo test -p flui-foundation removed_listener_does_not_fire_during_notify listener_fires_after_panic` — both fail.

**GREEN:** Rewrite `notify_listeners` using the snapshot + ID re-check + `catch_unwind(AssertUnwindSafe(|| callback()))` + `tracing::error!` shape from design §1 F5+F6. Add `use std::panic::{catch_unwind, AssertUnwindSafe}` and `use smallvec::SmallVec` imports. Snapshot type: `SmallVec<[(ListenerId, ListenerCallback); 4]>`.

Run `cargo test -p flui-foundation removed_listener_does_not_fire_during_notify listener_fires_after_panic` — both pass.

**TRIANGULATE:**
- `notify_listeners_fires_all_when_no_panic` — basic smoke test: 3 listeners, no panics, all 3 fire.
- `notify_listeners_empty` — no listeners registered; no panic, no-op.
- `notify_listeners_skips_all_removed` — all listeners removed before notify; none fire.

**REFACTOR:** None required for U1; snapshot shape is already clean.

---

### U2 — F11: Remove `Default for ValueNotifier<T>` + migrate flui-animation

**Files:** `crates/flui-foundation/src/notifier.rs`, `crates/flui-animation/**`

**Pre-condition:** Run `grep -rn 'ValueNotifier' crates/flui-animation/ --include='*.rs' | grep -E 'Default|default\(\)'` to identify all migration sites. List them in the commit message.

**RED:** Remove the `impl<T: Default> Default for ValueNotifier<T>` block from notifier.rs. Run `cargo check --workspace` — expect compilation errors in `flui-animation` (and any other crate using `ValueNotifier::default()` or `#[derive(Default)]` on structs with `ValueNotifier<T>` fields).

**GREEN:** For each migration site in `flui-animation`:
- Replace `ValueNotifier::<T>::default()` with `ValueNotifier::new(T::default())`.
- Replace `#[derive(Default)]` on structs with `ValueNotifier<T>` fields with an explicit `impl Default for Struct { fn default() -> Self { Self { field: ValueNotifier::new(T::default()), ... } } }`.

Run `cargo check --workspace` — exits 0.

**TRIANGULATE:**
- `valunotifier_default_no_longer_exists`: a `#[test]` that asserts `ValueNotifier::<u32>::default()` does NOT compile — this is a compile-error test (use `trybuild` or `compile_fail` doctest). Optional; if trybuild is not already a dev-dependency, skip and rely on the pre-migration RED compile failure as evidence.
- `valuenotifier_new_creates_distinct_notifiers`: two `ValueNotifier::new(0u32)` produce distinct identities (not `== by identity`).

**Note (OQ2 from design):** The `sdd-apply` agent must verify whether `cargo check --workspace` includes `flui-animation`. If `flui-animation` is excluded from the workspace default-members, add an explicit `cargo check -p flui-animation` step. See Delivery Decision section.

---

### U3 — F16 + F20 + F26: Housekeeping refactor

**Files:** `crates/flui-foundation/src/notifier.rs`, `crates/flui-foundation/src/lib.rs`

This unit is a REFACTOR commit (no new RED tests required; the cluster tests from U1 remain green).

**F16 — SmallVec retention comment:**
Add the comment near the `SmallVec` import / listeners field declaration explaining why `tinyvec` was rejected (`ListenerCallback: !Default`). Text from design §1 F16.

**F20 — `check_disposed` cfg-explicit layout:**
Rewrite `check_disposed` to use `#[cfg(debug_assertions)]` / `#[allow(unreachable_code)]` pattern from design §1 F20. Eliminates the misleading "debug_assert! + tracing::warn! in same block" pattern where the warn! was silently dead in debug builds.

**F26 — Doctest `println!` removal:**
Run `grep -rn 'println!' crates/flui-foundation/src/` to locate all instances.
- In doc-comment examples: replace `println!("{}", ...)` with `assert_eq!(...) ` or atomic counter pattern from design §1 F26.
- In `debug.rs:16` DiagnosticLevel doctest: replace `println!("{}", level)` with `assert!(level > DiagnosticLevel::Debug)`.
- In `lib.rs`: scan for any doc-test `println!`.

Run `! grep -rn 'println!' crates/flui-foundation/src/` exits 0 after cleanup.

**Verify:** `cargo test -p flui-foundation --doc` exits 0. `just ci` exits 0.

---

### PR-2 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| `flui-animation` | F11 compile break | Migration in same U2 commit |
| All crates with `ChangeNotifier` | F5/F6 behavior change (removed listeners no longer fire; panicking listeners no longer abort) | Grep for `remove_listener` inside listener closures (RD1 mitigation); document if any "one-shot" pattern found |

### PR-2 Verification commands

```bash
grep -rn 'remove_listener' crates/ --include='*.rs'   # Pre-merge audit for one-shot patterns
cargo test -p flui-foundation removed_listener_does_not_fire_during_notify
cargo test -p flui-foundation listener_fires_after_panic
cargo test -p flui-foundation notify_listeners_fires_all_when_no_panic
! grep -n "impl.*Default.*ValueNotifier" crates/flui-foundation/src/notifier.rs
cargo check -p flui-animation                          # OQ2 gate
! grep -rn 'println!' crates/flui-foundation/src/
just ci
```

---

## PR-3 — Cascade Cluster

**Title:** `fix(tree): cascade cycle-detection — try_remove + HashSet guard + SmallVec worklist, HRTB drop`

**Scope:** Add cycle-detection to the cascade walk (`try_remove` + `HashSet<I>` visited set, `TreeError::CycleDetected`), replace `Vec<I>` with `SmallVec` in the worklist, canonicalize `.get()` in `TreeWriteNav`, and drop over-engineered `for<'a>` HRTB from `TreeReadExt` / `TreeNavExt`.

**Findings closed:** F8, F19, F24, F30

**Files touched:**
- `crates/flui-tree/src/traits/write.rs` — F19, F24, F30
- `crates/flui-tree/src/error.rs` — F19 (`TreeError::CycleDetected` variant)
- `crates/flui-tree/src/traits/read.rs` — F8
- `crates/flui-tree/src/traits/nav.rs` — F8

---

### U1 — F19 + F24: `try_remove` + `HashSet` cycle-guard + `SmallVec`

**Files:** `crates/flui-tree/src/error.rs`, `crates/flui-tree/src/traits/write.rs`

**Pre-condition:** Confirm `Identifier: Hash + Eq` bounds in `crates/flui-foundation/src/id.rs`. (RD6: pre-confirmed; verify before committing.)

**RED test:**

Requires a test tree implementation that exposes a `corrupt_add_child` escape hatch to manually inject a cycle into the slab (bypassing public-API cycle prevention):

```rust
#[test]
fn cascade_cycle_detection() {
    let mut tree = TestTree::new();
    let a = tree.insert(node("a"));
    let b = tree.insert(node("b"));
    let c = tree.insert(node("c"));
    tree.add_child(a, b).unwrap();
    tree.add_child(b, c).unwrap();
    // Inject cycle: c's child list points back to a
    tree.corrupt_add_child(c, a);
    // try_remove must return Err(CycleDetected), not hang or OOM
    let result = tree.try_remove(a);
    assert!(matches!(result, Err(TreeError::CycleDetected { .. })),
        "try_remove must detect the cycle and return Err, got {:?}", result);
    // remove() must return None with tracing::warn! (not panic)
    tree.corrupt_add_child(c, a); // re-inject cycle
    let none_result = tree.remove(a);
    assert!(none_result.is_none(), "remove() must return None on cycle, not panic");
}
```

Run `cargo test -p flui-tree cascade_cycle_detection` — fails (old code hangs or OOMs).

**GREEN (two commits):**

*Commit A — Add `TreeError::CycleDetected` variant:*
```rust
// In error.rs — add #[non_exhaustive] variant:
#[error("cycle detected in cascade removal at node {node_id}")]
CycleDetected { node_id: usize },
```

*Commit B — Add `try_remove` + update `remove`:*
- Add `fn try_remove(...)` as a `TreeWrite` default method (requires `Self: TreeNav<I> + Sized`).
- Use `HashSet<I>` visited set for O(1) cycle detection per node.
- Use `SmallVec<[I; INLINE_TREE_DEPTH]>` for `to_visit` and `worklist` (replaces `Vec<I>`, closes F24).
- `INLINE_TREE_DEPTH` is imported from `crates/flui-tree/src/depth.rs`.
- Post-order drain: reverse the worklist, call `remove_shallow` on each node, return the root node.
- Update `remove` to delegate to `try_remove`: on `Ok(node)` return the node; on `Err(e @ CycleDetected)` log `tracing::warn!` and return `None`.

Full implementation shape is in design §1 F19. The implementation MUST match the `fetch_update` / `try_remove` shapes exactly; do not simplify to a `None`-on-cycle-only path.

Run `cargo test -p flui-tree cascade_cycle_detection` — passes.

**TRIANGULATE:**
- `remove_subtree_no_cycle` — normal subtree removal, no cycle; returns `Ok(Some(root_node))`.
- `remove_leaf_node` — leaf removal; no children to walk; returns `Ok(Some(leaf_node))`.
- `remove_nonexistent_node` — `try_remove` returns `Ok(None)`.
- `remove_shallow_still_available` — `remove_shallow` remains callable directly and is not affected by the `try_remove` wrapping.

---

### U2 — F30: `.get()` canonical path in `TreeWriteNav`

**Files:** `crates/flui-tree/src/traits/write.rs`

This unit is a REFACTOR within PR-3. No new RED test required; existing tests confirm the change is non-breaking.

**Change:** In `move_children` and `insert_child` in the `TreeWriteNav` trait:
- Replace `from.into()` / `to.into()` with `from.get()` / `to.get()`.
- Drop the `I: Into<usize>` bound from these methods.

Commit message: `refactor(tree): use .get() canonical path in TreeWriteNav, drop Into<usize> bound (F30)`

---

### U3 — F8: Drop `for<'a>` HRTB from `TreeReadExt` / `TreeNavExt`

**Files:** `crates/flui-tree/src/traits/read.rs`, `crates/flui-tree/src/traits/nav.rs`

This unit is a REFACTOR within PR-3. No new RED test required.

**Change:** For every method in `TreeReadExt` and `TreeNavExt` that currently carries `F: for<'a> FnMut(&'a Self::Node)` or `F: for<'a> FnMut(...)`, replace with the lifetime-elided form `F: FnMut(&Self::Node)`. The relaxation (removing over-constrained HRTB) is never breaking for callers.

**Verify:** `cargo check --workspace --all-targets` after the change to catch any closure inference regressions. If regressions appear, add explicit lifetime annotations at the call site (do NOT restore the HRTB). RD8 mitigation.

Commit message: `refactor(tree): drop for<'a> HRTB from TreeReadExt/TreeNavExt (F8)`

---

### PR-3 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| All crates using `TreeWrite::remove` | `remove` return type unchanged (`Option<Node>`); behavior change on corrupted trees only | No action for normal code paths |
| All crates using `TreeWrite` | `try_remove` is a new default method; no action required | None |
| All crates using `TreeReadExt`/`TreeNavExt` closures | HRTB relaxation — all existing closure code continues to compile | Verify with `cargo check --workspace` |

### PR-3 Verification commands

```bash
cargo test -p flui-tree cascade_cycle_detection
cargo test -p flui-tree remove_subtree_no_cycle
cargo test -p flui-tree remove_leaf_node
cargo test -p flui-tree remove_nonexistent_node
grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs
! grep -n "Vec<I>" crates/flui-tree/src/traits/write.rs
! grep -n "for<'a> FnMut" crates/flui-tree/src/traits/read.rs
! grep -n "for<'a> FnMut" crates/flui-tree/src/traits/nav.rs
cargo check --workspace --all-targets
just ci
```

---

## PR-4 — Edition-2024 Idiom Sweep

**Title:** `refactor(foundation,tree): edition-2024 idiom sweep — #[expect], clippy::pedantic, delete reinvented macros, port-check.sh gates`

**Scope:** Close the edition-2024 gap: `#[allow]` → `#[expect]` (F9, depends on PR-1), delete `debug_assert_*` reinventions (F29), `BindingBase` CAS optimization (F4), per-site `#[expect]` in `flui-tree` (F21), `clippy::pedantic` alignment for `flui-foundation` (F22), and append four new CI gates to the existing `scripts/port-check.sh` (confirmed APPEND, not create — the script has 13 existing triggers per OQ3 resolution).

**Findings closed:** F4, F9, F21, F22, F29, plus port-check.sh gate additions

**Dependency:** Must land after PR-1 (F9 needs F1+F2 to clear `unsafe` from id.rs/key.rs first).

**Files touched:**
- `crates/flui-foundation/src/id.rs` — F9 (final #[expect] vs delete)
- `crates/flui-foundation/src/key.rs` — F9 (final #[expect] vs delete)
- `crates/flui-foundation/src/binding.rs` — F4
- `crates/flui-foundation/src/assert.rs` — F29
- `crates/flui-foundation/src/lib.rs` — F22
- `crates/flui-tree/src/lib.rs` — F21
- `scripts/port-check.sh` — 4 new gates (append)

---

### U1 — F29: Delete reinvented `debug_assert_*` macros

**Files:** `crates/flui-foundation/src/assert.rs`, all files using the macros

**RED:** Delete the macro definitions (`debug_assert_valid!`, `debug_assert_range!`, `debug_assert_finite!`, `debug_assert_not_nan!`) from `assert.rs`. Run `cargo check --workspace` — expect compilation errors wherever the macros are used.

**GREEN:**
1. Run `grep -rn 'debug_assert_valid!\|debug_assert_range!\|debug_assert_finite!\|debug_assert_not_nan!' crates/ --include='*.rs'` to find all call sites.
2. Replace each call with the equivalent `debug_assert!(...)` from stdlib.
3. If `assert.rs` becomes empty after deletion, delete the file and remove its `mod assert;` declaration from `lib.rs`.

Run `cargo check --workspace` — exits 0.

**TRIANGULATE:** `! grep -rn "macro_rules! debug_assert_valid" crates/flui-foundation/src/` exits 0.

---

### U2 — F4: `BindingBase::instance()` CAS optimization

**Files:** `crates/flui-foundation/src/binding.rs`

No RED test required (F4 is a hot-path optimization, not a correctness change). This is a GREEN-only commit.

**Change:** In the steady-state path after `OnceLock` initialization, replace the unconditional `instance_ref.initialized.store(true, Ordering::Release)` with a `compare_exchange(false, true, Ordering::Release, Ordering::Relaxed)`. The `let _ = ...` discards the result; the CAS is a no-op on already-initialized instances.

Commit message: `perf(binding): use CAS for initialized flag to avoid per-call Release store (F4)`

**TRIANGULATE:** Run `cargo test -p flui-foundation --lib` to confirm no existing binding tests regress.

---

### U3 — F9: `#[allow(unsafe_code)]` → `#[expect]` (or delete)

**Files:** `crates/flui-foundation/src/id.rs`, `crates/flui-foundation/src/key.rs`

**Pre-condition:** After PR-1, inspect each file for remaining `unsafe { ... }` blocks:
- If no `unsafe` remains in a file: delete `#![allow(unsafe_code)]` entirely. The `#[expect]` would immediately fire its "expectation unfulfilled" lint, which is the desired behavior.
- If `unsafe` remains (e.g., `unsafe impl Send for Id<T>`): replace `#![allow(unsafe_code)]` with `#![expect(unsafe_code, reason = "...")]`.

**Commit message:** `refactor(id,key): replace allow(unsafe_code) with expect or delete (F9)`

**Verify:** `! grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/id.rs crates/flui-foundation/src/key.rs` exits 0.

---

### U4 — F21 + F22: per-site `#[expect]` in `flui-tree` + `clippy::pedantic` alignment

**Files:** `crates/flui-tree/src/lib.rs` (and trait files), `crates/flui-foundation/src/lib.rs`

**F21 — `flui-tree` blanket allow sweep:**
1. Remove `#![allow(clippy::too_many_lines)]` from `crates/flui-tree/src/lib.rs`.
2. Run `cargo clippy -p flui-tree -- -W clippy::too_many_lines` to find which functions triggered the blanket allow.
3. For each such function, add `#[expect(clippy::too_many_lines, reason = "...")]` at the function level with a meaningful reason string (per design §1 F21 example).

**F22 — `clippy::pedantic` for `flui-foundation`:**
1. Add `#![warn(clippy::pedantic)]` to `crates/flui-foundation/src/lib.rs`.
2. Run `cargo clippy -p flui-foundation -- -W clippy::pedantic` to surface new pedantic warnings.
3. For each warning: either fix the code (preferred) or add a `#[allow(clippy::module_name_repetitions)]` (or other specific suppression) with a comment explaining why.
4. Do NOT add a blanket `#![allow(clippy::pedantic)]` — defeats the purpose.

**Verify:** `just clippy` exits 0 for both crates.

---

### U5 — port-check.sh: Append 4 new CI gates

**Files:** `scripts/port-check.sh`

**Action:** APPEND (do not replace) the following four gates to the existing `scripts/port-check.sh`. The script currently has 13 triggers. These gates become triggers #14–#17 (or are added as a named block at the end; follow the existing script's trigger-naming convention).

Gates to add (from design §5):
1. **Gate: `println!` in foundation/tree source** — `grep -rn 'println!\|eprintln!\|dbg!'` in `crates/flui-foundation/src/`, `crates/flui-tree/src/`, `crates/flui-macros/src/`.
2. **Gate: module-level `#![allow(unsafe_code)]`** — `grep -rn '^#!\[allow(unsafe_code'` in `crates/flui-foundation/src/`, `crates/flui-tree/src/`.
3. **Gate: reinvented debug_assert macros** — `grep -rn 'macro_rules! debug_assert_valid\|debug_assert_range\|debug_assert_finite\|debug_assert_not_nan'` in `crates/flui-foundation/src/`.
4. **Gate: `new_unchecked` in key.rs** — `grep -n 'new_unchecked' crates/flui-foundation/src/key.rs`.

Each gate must follow the existing script style: exit non-zero with a diagnostic `echo "FAIL: ..."` message, or print `echo "PASS: ..."` in verbose mode.

**Verify:** `bash scripts/port-check.sh -v` exits 0 after the appended gates. All 4 new gates must report PASS.

---

### PR-4 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| All crates using `debug_assert_valid!` etc. | F29 compile break (macro deleted) | Migration in same U1 GREEN commit |
| CI pipeline | 4 new port-check.sh gates | Validate `bash scripts/port-check.sh -v` exits 0 |

### PR-4 Verification commands

```bash
! grep -rn "macro_rules! debug_assert_valid" crates/flui-foundation/src/
! grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/id.rs crates/flui-foundation/src/key.rs
cargo clippy -p flui-tree -- -W clippy::too_many_lines
cargo clippy -p flui-foundation -- -W clippy::pedantic
bash scripts/port-check.sh -v
just ci
```

---

## PR-5 — Diagnostics Cluster

**Title:** `feat(foundation,macros): diagnostics infrastructure — DiagnosticsPropertyKind, Diagnosticable derive, type_name strip`

**Scope:** Add typed `DiagnosticsPropertyKind` variants (F12), strip module paths from `to_diagnostics_node` type names (F27), and add the `#[derive(Diagnosticable)]` proc-macro in `flui-macros` (F15). F12 must be committed before F15.

**Findings closed:** F12, F15, F27

**LOC risk:** ~340–380 net LOC. If macro expansion exceeds 380 LOC, split into PR-5a (F12 + F27) and PR-5b (F15).

**Dependency note:** F12 introduces `DiagnosticsPropertyKind` and adds a `kind` field to `DiagnosticsProperty`. F15 uses `DiagnosticsNodeBuilder::add_property` from F12. They must land in the same PR with F12 committed first. F27 can land in either the F12 or F15 commit.

**Files touched:**
- `crates/flui-foundation/src/debug.rs` — F12, F27
- `crates/flui-macros/src/lib.rs` — F15

---

### U1 — F12: `DiagnosticsPropertyKind` enum + `kind` field

**Files:** `crates/flui-foundation/src/debug.rs`

**RED test:**
```rust
#[test]
fn diagnostics_property_kind_field_exists() {
    let prop = DiagnosticsProperty::new("width", "100.0");
    assert_eq!(prop.kind, DiagnosticsPropertyKind::Generic);
}
```
Run `cargo test -p flui-foundation diagnostics_property_kind_field_exists` — fails (field does not exist).

**GREEN:**
1. Add `DiagnosticsPropertyKind` enum with at least `Generic`, `Enum { description }`, `Flag`, `Iterable { count }`, `OptionalRef`, `Stack`, `Double { unit }`, `Int { unit }`, `Color`, `Offset`, `Rect`, `Size` variants. Full definition from design §1 F12.
2. Add `pub kind: DiagnosticsPropertyKind` field to `DiagnosticsProperty`.
3. `DiagnosticsProperty::new(name, value)` sets `kind = DiagnosticsPropertyKind::Generic` for backwards compatibility.

Run `cargo test -p flui-foundation diagnostics_property_kind_field_exists` — passes.

**TRIANGULATE:**
- `diagnostics_property_flag_kind` — construct a `Flag` kind property, assert `kind == DiagnosticsPropertyKind::Flag`.
- `diagnostics_property_iterable_kind` — construct an `Iterable { count: 3 }` kind property.

---

### U2 — F27: `type_name` strip to short name

**Files:** `crates/flui-foundation/src/debug.rs`

**RED test:**
```rust
#[test]
fn to_diagnostics_node_uses_short_type_name() {
    struct MyWidget;
    impl Diagnosticable for MyWidget {
        fn to_diagnostics_node(&self, name: &str) -> DiagnosticsNode {
            // Implement manually for this test
            DiagnosticsNode::new(name, std::any::type_name::<Self>())
        }
    }
    let w = MyWidget;
    let node = w.to_diagnostics_node("test");
    // type_name::<MyWidget>() includes module path; after fix it should be stripped
    // to just "MyWidget"
    assert_eq!(node.type_name(), "MyWidget",
        "type_name should be short (no module path), got: {}", node.type_name());
}
```
Run `cargo test -p flui-foundation to_diagnostics_node_uses_short_type_name` — fails (full path returned).

**GREEN:** Apply the `rsplit("::").next().unwrap_or(type_name::<Self>())` stripping to `DiagnosticsNode::new` or to any place in `debug.rs` that calls `std::any::type_name`. Full implementation from design §1 F27.

**Note — breaking change acknowledged:** Existing devtools or test matchers asserting on full module paths will break. This is explicitly permitted per project lead mandate. The `sdd-apply` agent must run `grep -rn 'type_name\|to_diagnostics_node' crates/ --include='*.rs'` to identify affected tests and update them in the same commit.

---

### U3 — F15: `#[derive(Diagnosticable)]` proc-macro

**Files:** `crates/flui-macros/src/lib.rs`

**Pre-condition:** U1 (F12) and U2 (F27) are committed; `DiagnosticsNodeBuilder::add_property` API is stable.

**RED test (compile-error test first):**
```rust
// In flui-macros tests or as a doc-test:
use flui_macros::Diagnosticable;
// This must fail to compile before the macro is added (no such derive):
#[derive(Diagnosticable)]
struct TestWidget {
    width: f32,
}
```
Run `cargo test -p flui-macros` — fails with "cannot find derive macro `Diagnosticable`".

**GREEN:** Implement `#[proc_macro_derive(Diagnosticable, attributes(diagnostic))]` in `crates/flui-macros/src/lib.rs`. The macro must:
1. Accept only named-field structs. Tuple structs, enums, generics → `compile_error!("...")` with a clear message.
2. Emit `impl Diagnosticable for Struct { fn to_diagnostics_node(&self, name: &str) -> DiagnosticsNode { ... } }`.
3. Use `std::any::type_name::<Self>().rsplit("::").next().unwrap_or(...)` for the short type name.
4. For each field NOT marked `#[diagnostic(skip)]`: emit one `builder.add_property(stringify!(field_ident), format!("{:?}", self.field_ident))`.
5. Fields marked `#[diagnostic(skip)]` are excluded.

Full macro shape from design §1 F15. **Scope cap:** named-field structs + `#[diagnostic(skip)]` only.

**Run test:**
```rust
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
    assert_eq!(node.type_name(), "TestWidget");
    let props: Vec<_> = node.properties().collect();
    assert_eq!(props.len(), 2);
    assert!(props.iter().any(|p| p.name == "width"));
    assert!(props.iter().any(|p| p.name == "height"));
    assert!(!props.iter().any(|p| p.name == "internal_id"));
}
```
`cargo test -p flui-macros diagnosticable_derive_basic` — passes.

**TRIANGULATE:**
- `diagnosticable_derive_empty_struct` — struct with all fields skipped; produces zero properties.
- `diagnosticable_derive_rejects_tuple_struct` — tuple struct with `#[derive(Diagnosticable)]` emits `compile_error!`. Use `trybuild` or a `compile_fail` doc-test.
- `diagnosticable_derive_rejects_enum` — same.

**REFACTOR:** If macro expansion LOC is under 250, consider extracting the field-iteration logic into a helper function for readability.

---

### PR-5 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| All crates with manual `to_diagnostics_node` impls | F27 type_name change; short name now returned | Grep `type_name` usages; update string-match assertions |
| `flui-rendering` render objects | F15 enables `#[derive(Diagnosticable)]` replace of hand-rolled impls | Optional follow-up; not in scope for this PR |

### PR-5 Verification commands

```bash
cargo test -p flui-foundation diagnostics_property_kind_field_exists
cargo test -p flui-foundation to_diagnostics_node_uses_short_type_name
cargo test -p flui-macros diagnosticable_derive_basic
grep -n "DiagnosticsPropertyKind" crates/flui-foundation/src/debug.rs
grep -n "bon::builder\|Diagnosticable" crates/flui-macros/src/lib.rs
just ci
```

---

## PR-6 — Test Gap Cluster

**Title:** `test(foundation): fill test gaps — BindingBase retry-after-panic, Id at usize::MAX`

**Scope:** Two standalone tests that document boundary behavior not covered by existing test suite (F17, F18). No production code changes — pure test additions.

**Findings closed:** F17, F18

**Files touched:**
- `crates/flui-foundation/src/binding.rs` (tests module) — F17
- `crates/flui-foundation/src/id.rs` (tests module) — F18

---

### U1 — F17: `BindingBase` retry-after-panic test

**Files:** `crates/flui-foundation/src/binding.rs` (tests module)

**RED:** Write the test (it will pass or fail depending on whether `OnceLock::get_or_init` actually resumes after a panic — this is the documented behavior). If the test body is written and `cargo test -p flui-foundation instance_retries_after_panic` fails, investigate the `OnceLock` semantics and adjust. The test body from design §1 F17 covers this scenario.

**GREEN:** Test should pass as-is (this is a behavior documentation test, not a fix). If `OnceLock` does NOT resume on panic in the current Rust version, document the limitation and adjust the test to assert current behavior.

**TRIANGULATE:** `binding_instance_idempotent` — `instance()` called twice returns the same instance (pointer equality).

---

### U2 — F18: `Id` boundary test at `usize::MAX`

**Files:** `crates/flui-foundation/src/id.rs` (tests module)

**RED/GREEN:** Write the test (see design §1 F18). `NonZeroUsize::new(usize::MAX)` is valid; the test documents that `Id` can hold `usize::MAX` and round-trips correctly through `from_raw` / `get`.

Full test body:
```rust
#[test]
fn id_at_usize_max() {
    let id = ElementId::new(usize::MAX);
    assert_eq!(id.get(), usize::MAX);
    let opt: Option<ElementId> = Some(id);
    assert!(opt.is_some());
    let raw = id.into_raw(); // use appropriate accessor
    let id2 = ElementId::from_raw(raw);
    assert_eq!(id, id2);
}
```

**TRIANGULATE:** `id_niche_at_usize_max` — `Option<ElementId>` with `Some(ElementId::new(usize::MAX))` is `Some`; `None` is 0 (niche optimization works at the boundary).

---

### PR-6 Verification commands

```bash
cargo test -p flui-foundation instance_retries_after_panic
cargo test -p flui-foundation id_at_usize_max
cargo test -p flui-foundation id_niche_at_usize_max
just ci
```

---

## PR-7 — Slot bon Builder

**Title:** `refactor(tree): #[bon::builder] for Slot::with_siblings — eliminate positional arg confusion`

**Scope:** Apply `#[bon::builder]` to `Slot::with_siblings` (and `Slot::new` if it has similarly confusable positional arguments) to eliminate the two-indistinguishable-`Option<I>` positional confusion (F14). `bon` is already a workspace dependency per the project constitution.

**Findings closed:** F14

**Files touched:**
- `crates/flui-tree/src/iter/slot.rs`

---

### U1 — F14: `#[bon::builder]` on `Slot::with_siblings`

**Files:** `crates/flui-tree/src/iter/slot.rs`

**RED test:**
Write a test that constructs `Slot::with_siblings` using the new builder syntax. Before the fix, the builder methods do not exist:
```rust
#[test]
fn slot_builder_syntax() {
    let parent_id = TestId::new(1);
    let next_id = TestId::new(2);
    // Builder syntax — fails before #[bon::builder] is added:
    let _slot = Slot::with_siblings()
        .parent(parent_id)
        .index(0usize)
        .depth(Depth::new(1))
        .next_sibling(Some(next_id))
        .call();
}
```
Run `cargo test -p flui-tree slot_builder_syntax` — fails (method-not-found or wrong syntax).

**GREEN:**
1. Add `#[bon::builder]` attribute to `Slot::with_siblings`.
2. Rename `prev` → `prev_sibling` and `next` → `next_sibling` in the parameter list for clarity (the rename is the primary value of the builder adoption).
3. Add `#[builder(default)]` to `index: usize` so callers can omit it when 0.
4. Update all call sites of `Slot::with_siblings(...)` in the `flui-tree` codebase and any downstream crates to use the new builder syntax.

Run `cargo test -p flui-tree slot_builder_syntax` — passes.

**TRIANGULATE:**
- `slot_with_siblings_all_fields` — build with all fields set; assert round-trip.
- `slot_with_siblings_minimal` — build with only required fields (omit `prev_sibling`, `next_sibling`, `first_child`, `last_child`); assert defaults are `None`.

**REFACTOR:** If `Slot::new` (if it exists) also has confusable positional args, apply `#[bon::builder]` in the same PR.

---

### PR-7 Downstream consumer impact

| Crate | Impact | Required action |
|-------|--------|-----------------|
| All crates constructing `Slot::with_siblings(...)` with positional args | Compile break — must switch to builder syntax | Grep `Slot::with_siblings` workspace-wide; update each call site in same commit |

### PR-7 Verification commands

```bash
cargo test -p flui-tree slot_builder_syntax
cargo test -p flui-tree slot_with_siblings_all_fields
cargo test -p flui-tree slot_with_siblings_minimal
grep -n "bon::builder" crates/flui-tree/src/iter/slot.rs
cargo check --workspace --all-targets
just ci
```

---

## Delivery Decision Required

### Total review burden

| PR | Net LOC | Risk |
|----|---------|------|
| PR-1 Soundness | ~50–80 | Low |
| PR-2 Notifier | ~90–120 | Low |
| PR-3 Cascade | ~100–160 | Low |
| PR-4 Idiom sweep | ~80–130 | Low |
| PR-5 Diagnostics | ~340–380 | **Near cap** |
| PR-6 Test gap | ~65 | Low |
| PR-7 Slot builder | ~45–55 | Low |
| **Session total** | **~770–1 025** | **Low vs 4 000-line budget** |

### Recommended PR strategy

**stacked-to-main**: each PR is independently mergeable (all pass `just ci`). Merge in the order: PR-1 → PR-2 → PR-3 → PR-4 → PR-6 → PR-7 → PR-5. Diagnostics (PR-5) last because it is the largest and carries OQ1 uncertainty.

### Open questions from design (OQ1–OQ3) — SUPERVISOR CONFIRMATION REQUIRED BEFORE sdd-apply

**OQ1 — F15 macro peer review (affects PR-5):**
The `Diagnosticable` derive macro shape was not reviewed by Codex (shell access unavailable during design). The design uses field-by-field `add_property` + `#[diagnostic(skip)]` as the MVP. Should PR-5 wait for a Codex peer review of the macro shape before sdd-apply begins?

- **Option A (default):** Proceed with field-by-field MVP as designed. Post-PR-5 review can course-correct; the macro is additive.
- **Option B:** Re-broadcast the F15 question to Codex from a shell-capable session before landing PR-5.

**OQ2 — flui-animation CI scope (affects PR-2 verify step):**
Does `just ci` (i.e., `cargo test --workspace`) compile `flui-animation`? If `flui-animation` is excluded from the workspace default-members:
- PR-2 must add an explicit `cargo check -p flui-animation` step to the verify commands.
- The F11 migration must still compile `flui-animation` cleanly even if it is excluded from the default workspace build.

**OQ3 — port-check.sh create vs append:**
**Resolved:** `scripts/port-check.sh` exists and has 13 triggers. The 4 new gates in PR-4 U5 are APPENDED, not replacing the file.

### Supervisor: confirm/adjust before sdd-apply

> **Please confirm OQ1 and OQ2 answers before the sdd-apply agent starts. sdd-apply may begin on PR-1 through PR-4 immediately (no OQ dependency). PR-5 (diagnostics) and PR-2 (notifier) have the open questions above. Recommended action: start PR-1 → PR-3 → PR-4 → PR-6 → PR-7 while supervisor confirms OQ1 (PR-5) and OQ2 (PR-2) in parallel.**

---

## Full Verification Gate (all 23 Success Criteria)

```bash
just ci                                                               # SC1
! grep -n "new_unchecked" crates/flui-foundation/src/key.rs          # SC2
cargo test -p flui-foundation key_counter_exhaustion                  # SC3
cargo test -p flui-foundation listener_fires_after_panic              # SC4
cargo test -p flui-foundation removed_listener_does_not_fire_during_notify # SC5
! grep -n "impl.*Default.*ValueNotifier" crates/flui-foundation/src/notifier.rs # SC6
cargo test -p flui-tree cascade_cycle_detection                       # SC7
grep -n "fn try_remove" crates/flui-tree/src/traits/write.rs         # SC8
! grep -n "^#!\[allow(unsafe_code" crates/flui-foundation/src/{id,key}.rs # SC9
! grep -rn 'println!' crates/flui-foundation/src/                    # SC10
! grep -n "macro_rules! debug_assert_valid" crates/flui-foundation/src/assert.rs # SC11
cargo test -p flui-macros diagnosticable_derive_basic                 # SC12
grep -n "DiagnosticsPropertyKind" crates/flui-foundation/src/debug.rs # SC13
grep -n "bon::builder" crates/flui-tree/src/iter/slot.rs             # SC14
cargo test -p flui-foundation instance_retries_after_panic            # SC15
cargo test -p flui-foundation id_at_usize_max                        # SC16
! grep -n "for<'a> FnMut" crates/flui-tree/src/traits/read.rs       # SC17
! grep -n "for<'a> FnMut" crates/flui-tree/src/traits/nav.rs        # SC17
! grep -n "unsafe fn from_raw" crates/flui-foundation/src/id.rs     # SC18
! grep -n "Vec<I>" crates/flui-tree/src/traits/write.rs             # SC19
grep -n "PhantomData<fn() -> T>" crates/flui-foundation/src/id.rs   # SC20
! grep -n "Into<Index>" crates/flui-foundation/src/id.rs            # SC21 (in trait def)
bash scripts/port-check.sh -v                                         # SC23
```

SC22 (per-PR LOC budget) is enforced by the estimates in this document and by `git diff --shortstat` spot-checks during apply.

---

*End of tasks.md. The sdd-apply agent may lift implementation shapes directly from `design.md §1`. Codex-mandated F2/F19 fix shapes MUST be honored. Do not start sdd-apply on PR-2 or PR-5 before supervisor confirms OQ1 and OQ2.*
