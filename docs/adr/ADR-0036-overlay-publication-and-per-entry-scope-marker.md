# ADR-0036: Overlay publication and the per-entry scope marker

*`Overlay`, `OverlayEntry`, `OverlayEntryId`, and `OverlayHandle` are published from `flui-widgets`' crate root. `Overlay::of`/`Overlay::maybe_of` resolve a new `OverlayScope` — an `InheritedView` marker mounted **per entry**, wrapping that entry's built child — rather than a `findAncestorStateOfType`-style walk to a shared `OverlayState`. This mirrors the 3.44.0 oracle's own mechanism, not an earlier one, and it is a `depend_on` (dependency-registering) lookup, not a lookup-only `get`, because the oracle's `Overlay.maybeOf` genuinely registers one.*

---

- **Status:** Accepted
- **Date:** 2026-07-18
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-widgets/src/overlay/{mod.rs,entry.rs,tests.rs}` (visibility of `Overlay`/`OverlayEntry`/`OverlayEntryId`/`OverlayHandle`; new `Overlay::of`/`maybe_of`, `OverlayScope`); `crates/flui-widgets/src/lib.rs` (crate-root + prelude re-exports); `crates/flui-widgets/src/navigator/navigator_tests.rs` (the mounted-tree lookup test); `crates/flui-widgets/src/interaction/draggable.rs` (the first live consumer — feedback painting, a follow-on change on this same branch); `docs/ROADMAP.md` (Cross.H corrections)
- **Related:** ADR-0008 (`flui-view` leapfrog — the `O(1)` per-element `InheritedView`/`depend_on` seam this ADR is the next production consumer of, alongside `Theme`); `flui-material`'s `ScaffoldScope`/`Theme` (the `get`-vs-`depend_on` precedent this ADR distinguishes itself from, below)

---

## Context

`crates/flui-widgets/src/overlay/` is a complete, tested port of Flutter's `Overlay`/`OverlayEntry`/`_Theater` (`overlay.dart`), already load-bearing: `Navigator` mounts an `Overlay` at `navigator.rs:1610` and every route's content builds inside one of its entries. Every item in the module was `pub(crate)`, though — nothing outside `flui-widgets` (and, until now, nothing outside `Navigator`) could name `Overlay`, `OverlayEntry`, or `OverlayHandle` at all.

This surfaced as a concrete gap while porting `Draggable` (`interaction/draggable.rs`): the oracle's `_DragAvatar` inserts its `feedback` widget into the nearest ancestor `Overlay`, found via `Overlay.of(context)`. FLUI had no such lookup — `Navigator` constructs and holds its `OverlayHandle` directly, with no `InheritedWidget`-style mechanism for an arbitrary descendant to find it. `Draggable::feedback` was accepted and stored but never painted, and the gap was recorded honestly in both `draggable.rs`'s module docs and `docs/ROADMAP.md`'s Cross.H section rather than silently skipped.

Separately, `docs/ROADMAP.md`'s Cross.H section still carried an older, now-stale claim: that a "`BuildContext` inherited-data hole" gated `Catalog.1` (`Theme` needing it). That hole was real when written, but ADR-0008 closed it — `ctx.depend_on::<T>()` is a live, `O(1)`, per-element `InheritedView` mechanism today, and `Theme` (`flui-material`) already ships on it in production. The roadmap text had not been corrected to say so, which made this ADR's actual subject — an `Overlay`-specific lookup gap, not a general one — read as a duplicate of an already-closed issue. Both are corrected in this change (see "Roadmap correction," below).

### The oracle mechanism has itself changed since FLUI's Flutter reference vintage

Most of `flui-widgets` tracks Flutter tag `3.33.0-0.0.pre-6280-g88e87cd963f` (see `overlay/mod.rs`'s own parity header). Checked directly against `3.44.0`'s `widgets/overlay.dart`: modern `Overlay.maybeOf` resolves `_RenderTheaterMarker` — a private `InheritedWidget` that `_OverlayEntryWidgetState.build` mounts around **each entry's own child** — via `context.dependOnInheritedWidgetOfExactType<_RenderTheaterMarker>()`, which registers a real dependency (`createDependency: true` is the method's default). Older Flutter releases instead used `context.findAncestorStateOfType<OverlayState>()`: a plain ancestor walk to one shared `State`, with no per-entry granularity and no dependency at all.

The difference matters here beyond mere trivia: a `findAncestorStateOfType`-style port would hand back a single `OverlayHandle` looked up by walking element ancestry (cheap in this codebase too, since `flui-view`'s tree already supports ancestor walks) but would register **no dependency** — indistinguishable, from a caller's perspective, from `ScaffoldScope::maybe_of`'s `get`-based lookup (`flui-material`). Porting the *current* oracle mechanism instead of the historical one means this ADR reproduces `dependOnInheritedWidgetOfExactType`'s actual contract: a genuine dependency, and a marker mounted per entry rather than once per `Overlay`.

## Decision

### 1. Publish four types; keep the mutation surface private

`Overlay`, `OverlayEntry`, `OverlayEntryId`, and `OverlayHandle` move from `pub(crate)` to `pub`, re-exported from the crate root and the prelude (matching every other widget in this crate — see `lib.rs`'s existing flat-re-export convention). Their fields stay private; their *existing* methods (`insert`, `insert_all`, `rearrange`, `mark_needs_build`, `remove`, the `with_opaque`/`with_maintain_state` builders, …) stay exactly as visible as they were — `pub(crate)`. `Navigator` and `Draggable`'s feedback layer (the next section) are this surface's only callers, in-crate, both before and after this change; nothing here needed it wider, and widening the mutation surface — a public `OverlayHandle::insert`, a public `Overlay::new`/`initialEntries` constructor for an app author to build a standalone overlay — is explicitly **not** this ADR's gate (see Deferred, below).

Publishing the *types* without their mutation methods is a deliberate, narrow cut: it is exactly enough for `Overlay::of`/`maybe_of` to return a nameable `OverlayHandle` from a `pub fn` (Rust requires a public function's return type to be reachable), and exactly the shape the oracle's own `Overlay`/`OverlayEntry` classes are *used* for by every consumer this codebase has today. A wider public mutation API is a real, separate design question — deciding whether `OverlayHandle::insert` should take the oracle's `above`/`below` named-argument shape or a Rust-native alternative is not something this change should decide as a side effect of unblocking a lookup.

### 2. `OverlayScope`: an `InheritedView` marker mounted per entry, not per overlay

A new `pub(crate)` type, `OverlayScope`, implements `InheritedView<Data = OverlayHandle>`. It stays `pub(crate)` — like its oracle counterpart `_RenderTheaterMarker` (leading underscore: private, never named outside `overlay.dart`), nothing outside the `overlay` module ever names `OverlayScope` directly. `Overlay::of`/`maybe_of` are the only door.

`OverlayEntryViewState::build` — the method that already runs an entry's builder (`(view.entry.builder())(ctx)`) — now wraps that builder's result in `OverlayScope::new(view.overlay.clone(), child)`, where `view.overlay` is the enclosing `Overlay`'s handle, threaded down from `OverlayState::build` through a new field on the (already-private) `OverlayEntryView`. This is the direct analogue of `_OverlayEntryWidgetState.build` wrapping its child in `_RenderTheaterMarker(overlayState: ..., child: ...)`.

Mounting per entry rather than once per `Overlay` matters for nested overlays: `Navigator` routes can themselves contain another `Overlay` (a nested `Navigator`, or a future `OverlayPortal`-style widget), and a descendant inside the inner one must resolve *that* overlay, not the outer one. Because each entry gets its own marker, and `flui-view`'s inherited-element map already resolves the *nearest* enclosing provider of a given type with no extra code, nearest-wins for nested overlays falls out for free — pinned by `overlay_maybe_of_resolves_the_nearest_enclosing_overlay` in `overlay/tests.rs`.

### 3. `depend_on`, not `get` — a ruling that distinguishes this from `ScaffoldScope`

`Overlay::maybe_of` resolves via `BuildContextExt::depend_on` (which registers a dependency), not the lookup-only `get` that `crate::ScaffoldScope::maybe_of` (`flui-material`) uses for its `DrawerHandle`. The two precedents look similar on the surface — both hand back a stable, `Clone`-cheap capability handle — but the ruling differs because *what they are ports of* differs:

- `ScaffoldScope` is a FLUI invention with no oracle contract of its own to be loyal to (Flutter's real `Scaffold.of` predates modern `InheritedWidget` dependency ergonomics and is itself a `findAncestorStateOfType`-style lookup in the oracle for the drawer API specifically); using `get` there is a legitimate, independently-justified choice for an ambient capability object whose own methods read live state.
- `Overlay.of`/`maybeOf` **is** an oracle-contracted call, and the current oracle (3.44.0) specifically calls `dependOnInheritedWidgetOfExactType` — i.e., Flutter itself decided this lookup should register a dependency. Porting behavior loyally (Prime Directive #1) means reproducing that decision, not re-deriving a "no dependency needed" argument from FLUI's own reasoning about handle stability.

Concretely, this means an `Overlay::maybe_of` caller in `did_change_dependencies` is re-notified if a *different* overlay identity ever replaces the resolved one (see `OverlayScope::update_should_notify`'s `Arc::ptr_eq`-based check) — a contract this ADR's test suite pins directly (`overlay_scope_update_should_notify_is_true_only_on_handle_identity_change`) since no reachable production path exercises it end-to-end today (an `OverlayEntryView`'s `overlay` field never changes across a mounted entry's lifetime).

### 4. No new `BuildContext` capability, no frame-phase change

`Overlay::of`/`maybe_of` are ordinary `depend_on` calls — the same mechanism `MediaQuery::of`/`Theme::of` already use — not a new capability threaded through `BuildContext`. `scripts/check-frame-capability-scope.sh` (port-check trigger #22) is untouched: nothing here adds a lifecycle-only token, and the one new production caller this unlocks (`DraggableState`, see the follow-on change) acquires the handle in `did_change_dependencies` — a lifecycle hook, not `build` — exactly the `ADR-0018` pattern this codebase already uses for `RebuildHandle`.

## Roadmap correction

`docs/ROADMAP.md`'s Cross.H section carried two corrections, made in this change:

1. The stale "`BuildContext` inherited-data hole (**gates Catalog.1**)" line is removed from Cross.H's goal statement and the parallelism map; a resolved-note replaces it, crediting ADR-0008 and noting `Theme`'s production use. This was not a claim this ADR's own subject invalidated — it was already false before this change, just never corrected.
2. The `Draggable`-surfaced "**Known gap:** no `Overlay.of(context)`-style ancestor lookup" entry is flipped to a **Fixed** note pointing at this ADR. The neighboring "**Known gap:** no widget-reachable fresh hit-test capability" entry is **not** touched — it remains open, and still gates `DragTarget` accepting a drag `Draggable`'s feedback can now visibly follow.

## Deferred (named, not silently dropped)

- **`rootOverlay`.** The oracle's `Overlay.of(context, rootOverlay: true)` walks past nested overlays to the outermost one. `Overlay::of`/`maybe_of` always resolve the nearest. No caller needs the root variant yet.
- **`LookupBoundary`.** Flutter's mechanism for a widget to opt its subtree out of `Overlay.of` (and other `InheritedWidget` lookups) reaching past it. Not ported; nothing in this codebase has an equivalent boundary concept yet.
- **`OverlayPortal`.** The oracle's declarative alternative to manual `OverlayEntry` insertion (build a portal, get an overlay child without holding an entry handle yourself). Not started — a substantial standalone port, not a byproduct of publishing the existing imperative API.
- **`Overlay.wrap`, a public `Overlay::new`/`initialEntries` constructor.** An app author cannot construct a standalone `Overlay` today — only `Navigator` does. The published `Overlay`/`OverlayHandle` types are usable through `Overlay::of`/`maybe_of` (read-only, from a caller's perspective) and, in-crate, through the still-private mutation surface. Whether/how to open construction to app authors is a separate design question.
- **The fresh hit-test capability** (`docs/ROADMAP.md`'s Cross.H, unchanged by this ADR). `Draggable`'s feedback can now paint and follow the pointer, but `DragTarget` acceptance still needs a widget-reachable, arbitrary-position hit test this codebase does not have — see the neighboring Cross.H entry.
- **`Focus.of`.** Named here only as the next widget this same `depend_on`-per-marker pattern would naturally extend to, should a future task need ambient `Focus` ancestor lookup with ADR-0026's ownership model — not committed to by this ADR.
