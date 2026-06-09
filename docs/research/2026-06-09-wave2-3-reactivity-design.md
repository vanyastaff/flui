[← Plan](2026-06-08-beat-flutter-plan.md) · [Foundations C1/C4/C9](../FOUNDATIONS.md#part-iii--the-locked-contracts)

# Wave 2-3 Reactivity — Adversarially-Vetted Design

> **Source:** `wave2-3-reactivity-design` workflow (2026-06-09): 4 read-only investigations of current-main flui-view → design synthesis → harsh-critic adversarial pass. Verdict: **needs-revision** (fixes folded in below). This supersedes the rougher Wave 2/3 sketch in the [plan](2026-06-08-beat-flutter-plan.md).

## Verdict & split

- **Wave 3 (memoization)** — clean, self-contained, unit-testable independent of the app loop. **Implement now.**
- **Wave 2 (production mutable-state path)** — **gated on V-7** (parallel-root-store unification) + a deadlock fix + redraw-chain wiring. Larger PR. See §Wave 2 below.

---

## Wave 3 — `should_skip_rebuild` + `Memo<V>` + dispatch equality-bail

The build-skip optimization (FOUNDATIONS Part II item 4), Druid-trap-safe.

1. **`crates/flui-view/src/view/view.rs`** — add a NEW default method to the `View` trait (after the existing `can_update` at ~:106):
   ```rust
   /// Typed memoization short-circuit. Returns `true` if a same-type, same-position
   /// rebuild can be SKIPPED because `self` is interchangeable with `prev`.
   /// Default `false` (always rebuild = Flutter parity). `where Self: Sized` keeps
   /// `View` object-safe (excluded from the dyn vtable). Opposite polarity to
   /// `can_update` (which is the Flutter type+key matchability gate — do NOT merge them).
   fn should_skip_rebuild(&self, prev: &Self) -> bool where Self: Sized { let _ = prev; false }
   ```
   No `PartialEq` bound, no lifetime, no associated type, all-`&self` → C1 + C4 preserved. The existing `_assert_view_is_object_safe(&dyn View)` test must still compile.

2. **`crates/flui-view/src/element/dispatch.rs`** — the equality-bail in `dispatch_view_update`. **Adversarial MAJOR fix:** evaluate the skip on a BORROW *before* the unconditional `dyn_clone::clone_box` (~:102), else every skip still pays a full `View` clone:
   ```rust
   // before clone_box:
   if let Some(new_ref) = new_view.as_any().downcast_ref::<V>() {   // PORT-CHECK-OK-... (FR-033)
       if core.view().should_skip_rebuild(new_ref) {
           return true; // reuse element; skip clone + replace + rebuild (configs equal)
       }
   }
   // else: existing clone_box → downcast → replace_view_for_dispatch → mark_dirty_for_dispatch
   ```
   On skip we do nothing (configs are equal for the Memo/PartialEq path, so keeping the old config is correct and avoids the clone). Single monomorphic per-`V` site → covers all arities (Single/Optional/Variable funnel through here). Resolve the FR-033 `downcast_ref` allowlist marker in the same edit.

3. **`crates/flui-view/src/view/memo.rs`** — NEW. `Memo<V>` proxy wrapper; the `PartialEq` bound lives ONLY here (C1):
   ```rust
   pub struct Memo<V> { inner: V }
   impl<V: View + PartialEq> View for Memo<V> {
       fn should_skip_rebuild(&self, prev: &Self) -> bool { self.inner == prev.inner }
       // proxy-family create_element forwarding to inner
   }
   ```
   **Adversarial MAJOR (Druid trap at Memo layer):** `Memo` is **unsound for views carrying a callback/`Arc<dyn Fn>` field** — `PartialEq` can't compare closures → a changed handler is silently kept stale. Document `Memo` as unsafe-for-callbacks; ship a stale-closure regression test.

4. **`crates/flui-view/src/lib.rs`** — `mod memo;` + export `Memo` (top-level + prelude). `IntoView` is free via the blanket impl.

5. **`docs/FOUNDATIONS.md:72`** — correct the STALE Part II cell ("can_update defaults to PartialEq") which contradicts the locked C1 prose at :89. New text: default is always-rebuild (`false`); PartialEq-skip is opt-in via `Memo<V>` or a per-view override.

**Tests:** (b) Memo unchanged subtree NOT rebuilt (build-counter probe + `ReconcileEvent` Reuse); changed subtree DOES rebuild. (b') plain non-Memo equal-config view STILL rebuilds (default-false parity = C1 lock). (c) object-safety: `_assert_view_is_object_safe` still compiles + a `dyn View` construction; optional trybuild compile-fail that `dyn_view.should_skip_rebuild(..)` is NOT callable. Stale-closure: a `Memo<ViewWithCallback>` whose closure changed but data equal → documents/guards the unsoundness.

**Contract compliance (verified by the workflow):** C1 — no signals, no blanket `PartialEq`; default `false`. C4 — `where Self: Sized` keeps `View` object-safe. C9 — bail runs where both concrete `V` coexist with zero dyn.

---

## Wave 2 — production mutable-state path (V-7-GATED)

The proven dual-write (`schedule_root_rebuild`, binding.rs:777): mutate `V::State` + set the local dirty atomic **AND** `BuildOwner::schedule_build_for` (the heap `build_scope` drains). Both signals are required. A `set_state_for<V,F>(id, depth, f)` handle on `WidgetsBinding` drops into the existing `VoidCallback` slot (no new bound, C1).

**Why gated — adversarial must-fixes:**
- **FATAL deadlock:** `parking_lot` RwLock is non-reentrant; `draw_frame` holds `inner.write()` across `build_scope`. A setState fired during build self-deadlocks. Fix: keep the local-atomic half lock-free + drain the heap-schedule post-build (mirror `mid_layout_marks`), or a pre-lock re-entrancy guard.
- **V-7 parallel-root-store:** `AppBinding.root_element` (rebuilt via `rebuild_root`, bypassing `build_scope`) vs `WidgetsBinding.element_tree` (what `build_scope` drains). If the renderer paints the former, setState rebuilds the latter → silent no-op. **Must establish a single source of truth for the painted tree first.**
- **Unwired redraw chain:** `set_on_need_frame`/`set_on_build_scheduled` have zero production callers → a between-vsync setState enqueues but never wakes the on-demand event loop.
- **Rigid-downcast silent-loss:** `downcast_mut::<Element<V,Single,StatefulBehavior<V>>>` fails for `AnimatedBehavior`/composed behaviors → mutation skipped while schedule fires. Route via a behavior-agnostic `state_as_any_mut`; enforce mutation⇒schedule (never schedule if mutation was skipped, à la U33's queue⇒flag).
- **False-green:** close V-7 + redraw with an END-TO-END integration test (real loop, red-then-green) before trusting any unit green — `build_scope`-direct unit tests cannot detect a no-op repaint.

Wave 2 is therefore its own PR, sequenced after a V-7 single-painted-tree resolution.
