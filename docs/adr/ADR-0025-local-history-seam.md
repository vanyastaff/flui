# ADR-0025 — The `LocalHistoryRoute` seam

- **Status:** **Accepted — U1 landed 2026-07-11; U2 gated (§6).** Produced by a design pass (chief-architect agent) and an adversarial critique (harsh-critic agent), 2026-07-11. The critique's three structural findings are folded in below as **binding constraints**, not suggestions; one of them exposed a live deadlock in the already-shipped `PopScope` fan-out, fixed immediately in `7b038dee`. U1 is implementable now; U2's public surface is gated (§6).
- **Date:** 2026-07-11
- **Deciders:** chief-architect (delivery/lock discipline, §3.3); api-design-lead (the three U2 public types, §3.2 — sign-off due before U2, not U1); product-steward (§6's U2 timing); qa-lead (the §5 edge-case matrix).
- **Relates to:** ADR-0019 (deferred `LocalHistoryRoute`; the `will_handle_pop_internally`/`did_pop → false` machinery already transcribed and commented as waiting — `route.rs:178-186`, `:215-224`; `history.rs` `can_pop`/`pop_disposition_of_top`); PopScope (`pop_scope.rs`, whose registry pattern and — post-`7b038dee` — deferred delivery this reuses).

---

## 1. Context

A `LocalHistoryEntry` is a mini navigation state inside one route: while a route holds entries, a pop removes the most recent **entry** instead of the route (`routes.dart:736-742`) — how Flutter's Drawer and persistent bottom sheets make back dismiss *them* before the page. FLUI pre-cut the read side; missing are the entry stack, `ModalRoute`'s two overrides, and a page-reachable way to add entries (FLUI hands out no route objects after push).

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/routes.dart`, master `3.33.0-0.0.pre-6280-g88e87cd963f`: the mixin (`:747-973`, applied to `ModalRoute` at `:1266`, **not** the `Route` base); `LocalHistoryEntry { onRemove, impliesAppBarDismissal }` (`:708-723`); `addLocalHistoryEntry` (`:882-896`, `changedInternalState` on the empty→non-empty edge); `removeLocalHistoryEntry` (`:902-927`, `onRemove` synchronous); `didPop` popping the last entry and returning `false` — the route stays, `_popCompleter` never completes (`:950-967`); `willHandlePopInternally` = non-empty (`:970-972`). Navigator side: a refused `didPop` returns the entry to `idle`, **skips** `onPopInvokedWithResult(true, …)` (`navigator.dart:3357-3379`) and adds **no** pop observation (`:4513-4531`) — observers stay silent, the route's future stays pending. Canonical consumer: material Drawer (`drawer.dart:522-555`) — the entry's lifetime tracks the *open drawer* via animation-status callbacks while the widget stays mounted; `Scaffold`'s persistent bottom sheet is the same imperative shape (`scaffold.dart:2379`, `:2573`).

## 3. The Rust shape

### 3.1 Placement — a third `ModalInner` registry

`local_history: LocalHistoryRegistry` beside `heroes` and `pop_entries` (`modal_route.rs:170-185`) — the established route↔page pattern (registry on `ModalInner`, private inherited scope, public handle; rule of three). `ModalRoute` overrides: `will_handle_pop_internally()` → non-empty (`routes.dart:970-972`); `did_pop()` → pop the last entry and return `false`, else delegate. **Rejected:** storage on the erased `RouteEntry`/`RouteRecord` layer — Flutter scopes this to `ModalRoute`; record-level storage gives every `SimpleRoute` test double an entry stack and cannot reach `changed_internal_state`.

### 3.2 Public surface — an ambient handle, not a widget

A private `LocalHistoryScope` (the `PopEntryScope` pattern) provides the registry to the page. Public (U2): `LocalHistoryHandle::maybe_of(ctx)` / `.add(LocalHistoryEntry) -> LocalHistoryEntryHandle` / `entry_handle.remove()`. Entry identity is the `Arc` — a handle can only remove the entry it was minted for, replacing Flutter's `_owner` asserts (`:883`, `:903-904`) with a shape where cross-route theft is syntactically absent.

**Rejected: a declarative add-on-mount widget as the primary surface.** The Drawer evidence is decisive: the entry's lifetime is the *open drawer*, driven from animation callbacks while `DrawerController` stays mounted (`drawer.dart:525-545`) — a mount-scoped widget cannot express the canonical consumer. `PopScope` is a widget because Flutter's is; that's loyalty, not a rule that every route seam is widget-shaped. Sugar can layer on later.

Acquisition discipline (trigger #22): `add` is rebuild-adjacent (`changed_internal_state` → `mark_entry_needs_build`), so the handle follows the `rebuild_handle()` rule — acquired in `init_state`/`did_change_dependencies`, fired from event/animation callbacks, never in `build`/layout/paint; if gated as a `BuildContext` capability its token joins `check-frame-capability-scope.sh` in the same change.

### 3.3 Binding constraints from the adversarial review

1. **All user-visible effects defer through `FlushOutcome`.** `did_pop` fires inside the flush under the non-reentrant history lock (`navigator.rs` `mutate`); an inline `on_remove` calling any `NavigatorHandle` method — even `can_pop()` — deadlocks same-thread. The repo canon already says so twice (`binding.rs` Correction 1; `FlushOutcome`'s deliver-outside-the-lock design), and the critique proved the point by finding the same bug live in the shipped `PopScope` fan-out (fixed, `7b038dee`, with a watchdog regression test). So: entries popped by `did_pop` are **recorded**; `on_remove`, the emptied-edge `changed_internal_state`, and any other user-visible effect drain through `FlushOutcome::apply` after the lock is released — still synchronous within the caller's `pop()`, preserving Flutter's observable ordering. `entry_handle.remove()` outside a flush fires `on_remove` directly (`routes.dart:902-927`) — same callback, same execution context either way.
2. **The registry mutex is a leaf.** Mutate under lock, release, then fire (the `pop_scope.rs` clone-out pattern). The linearization point for removal is an atomic `removed` flag on the entry inner, so a concurrent `entry_handle.remove()` racing `did_pop` fires `on_remove` **exactly once** — "idempotent" as a mechanism, not a doc-comment.
3. **`maybe_pop` becomes one critical section.** Today `pop_disposition_of_top` and `pop_erased` are two separate lock acquisitions (`navigator.rs:551`, `:558`); entry churn (per-gesture, per the Drawer) makes the TOCTOU window hot — a disposition decided "Pop (an entry exists)" can pop the **route** after a racing `remove()`. Disposition + `arm_pop` move inside one `mutate` closure. (Pre-existing race; this seam is what makes it reachable in practice.)
4. **No Arc cycles.** The natural consumer stores the entry handle in state captured by `on_remove` (`drawer.dart:522`) — with an owned `Arc<dyn Fn>` that's `EntryInner → closure → state → handle → EntryInner`, leaked forever under Rust. `on_remove` is consumed (`Option::take`) at fire-or-dispose time, and the entry handle holds `Weak` to the registry.

### 3.4 Pop-flow trace (verified against sources)

`pop` → flush Pop arm → `handle_pop` → `RouteRecord::did_pop` → typed `ModalRoute::did_pop` pops an entry, returns `false` **without completing** — the future stays pending (`route.rs:370-376` ≙ `routes.dart:964-966`); state → `Idle`, no `on_pop_invoked` (`history.rs` ≙ `navigator.dart:3368-3371`); no `Observation::Pop` (≙ `:4517-4519`). The refusal machinery, observer silence, and the bottom-route `can_pop` case activate the moment `ModalRoute` overrides the two methods — **`history.rs` needs no arm changes** (the §3.3 changes are delivery-shape, not arm logic). Bundled doc fix: `RouteHistory::pop`'s comment claims `false` on refusal; the code returns `true` unconditionally and is right (`history.rs:684-695`). Ordering with `PopScope`: veto before entries, already correct (`history.rs` ≙ `routes.dart:2033-2042`); pinned test — a `can_pop=false` scope blocks `maybe_pop` even while entries exist; programmatic `pop()` skips the veto and pops the entry.

## 4. Deliberately absent (named)

`impliesAppBarDismissal` (+ counter, `:749` — no AppBar); the `persistentCallbacks` deferral (`:914-925` — FLUI's rebuild inbox is phase-safe); deprecated `willPop`; Navigator-2.0 interplay; declarative entry-widget sugar; **and the `canPop`-change notification path**: Flutter dispatches `NavigationNotification` so chrome outside the route re-reads `canPop` when `willHandlePopInternally` flips — FLUI rebuilds only the overlay entry; nothing notifies out-of-route `can_pop()` readers (also named at `pop_scope.rs:23-24`).

## 5. Edge semantics, each pinned with a U1 test

- **`add` on a popping/disposed route** → inert drop + `tracing::warn!` (the registry outlives the route; `changed_internal_state` is already inert then).
- **Route dispose with live entries** → `onRemove` does **not** fire (Flutter GC-drops the list); in Rust that is an act: closures dropped at dispose, outside any lock (§3.3.4).
- **`remove()` after the route died** → `on_remove` fires (Flutter's dispose never clears `_owner`; `removeLocalHistoryEntry` still runs).
- **`handle_pop`'s `is_completed` early-return**: a route completed via `remove_route` then popped skips `did_pop` — leaves with live entries, `on_remove` silent; Flutter asserts page-based there (`navigator.dart:3361-3366`), FLUI doesn't. Equivalence note + test.
- The Flutter material-free example (`routes.dart:762-880`) is the U1 integration test.

## 5a. U1 as landed (2026-07-11)

Shipped per the constraints: `LocalHistoryRegistry` on `ModalInner` (leaf lock;
atomic `removed` flag as the exactly-once linearization), the two `ModalRoute`
overrides, deferred delivery — `did_pop` *records* the owed `on_remove` and the
flush notes the route in `FlushOutcome::refused_pops`; `apply` drains via
`ModalHandle::drain_local_history` with no lock held, the emptied-edge
`changed_internal_state` riding the same drain — and `maybe_pop`'s disposition
+ act collapsed into one `mutate` critical section. Red-checked: dropping the
`did_pop` arm pops the route instead of the entry; firing inline instead of
deferring hangs the watchdog re-entrancy test. **One §5 pin corrected at
implementation:** `remove()` after the route died is a **no-op**, diverging
from Flutter (which fires `onRemove` because GC lets `_localHistory` float) —
keeping callbacks alive past dispose is precisely the §3.3.4 Arc-cycle leak,
so dispose severs; recorded in the module docs. The `pub(crate)` surface
(`LocalHistoryHandle::maybe_of`/`add`, entry handles) is exercised by the
Flutter material-free example as the integration test, plus the veto-ordering,
observer-silence, pending-future, single-route-claim, exactly-once, and
sever-at-dispose pins.

## 6. Units and the recommendation

- **U1 — mechanism + `pub(crate)` surface, ship now**: registry, overrides, §3.3 delivery/lock constraints (incl. the `maybe_pop` critical-section fix), §5 matrix, the doc fix. The integration test runs against the `pub(crate)` scope/handle.
- **U2 — public surface, gated**: the visibility flip + api-design-lead sign-off of the three types, landed **beside the first consumer** (the Catalog Drawer) or on an explicit product-steward call. The critique is right that "usable today exactly as PopScope" overstated: PopScope is passive-declarative with day-one utility; this handle demands animation-driven imperative consumers that don't exist in-tree yet.
- **U3 — deferred**: §4 items.

## 7. Risks

The §3.3 constraints *are* the risk register: inline delivery (deadlock), non-leaf registry lock (AB/BA inversion), two-step `maybe_pop` (TOCTOU), owned `on_remove` (leak). Semver: U1 adds no public surface; U2 adds three types, `LocalHistoryEntry` stays a builder so `implies_app_bar_dismissal` can join without breakage.
