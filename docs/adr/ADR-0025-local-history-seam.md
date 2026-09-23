# ADR-0025: The `LocalHistoryRoute` seam

- **Status:** Accepted
- **Date:** 2026-07-11

## Context

A local-history entry is a small navigation state inside one route: while a route holds
entries, a pop removes the most recent entry instead of the route. This is how Flutter's
Drawer and persistent bottom sheets make "back" dismiss them before the page
(`routes.dart`, `LocalHistoryRoute`, applied to `ModalRoute`, not the `Route` base).
ADR-0019 already carried the read side (`will_handle_pop_internally`, a `did_pop` that can
return `false`); missing were the entry stack, `ModalRoute`'s overrides, and a way for a page
to add entries, since FLUI hands out no route objects after push.

The mechanism ships `pub(crate)`; the handle becomes public beside its first consumer (a
Drawer). Revisit this seam when the typed Router replaces the Navigator-first shape
(ADR-0019).

## Decision

### 1. A registry on `ModalInner`

`LocalHistoryRegistry` sits beside the hero and pop-entry registries on `ModalInner` (the
established route↔page pattern: registry on the route, private inherited scope, handle for
the page). `ModalRoute` overrides `will_handle_pop_internally()` (non-empty) and `did_pop()`
(pop the last entry and return `false`, else delegate). A refused pop leaves the route
`Idle`, fires no `on_pop_invoked`, emits no pop observation and leaves the route's result
future pending, as in Flutter. A `PopScope` veto is checked before entries; a programmatic
`pop()` skips the veto and pops the entry.

### 2. An ambient handle, not a widget

A private `LocalHistoryScope` provides the registry to the page;
`LocalHistoryHandle::maybe_of(ctx).add(entry) -> LocalHistoryEntryHandle` and
`entry_handle.remove()` are the surface. Entry identity is the `Arc`: a handle can remove only
the entry it was minted for, replacing Flutter's `_owner` asserts with a shape where removing
another route's entry cannot be written.

`add` triggers a rebuild of the route, so the handle is acquired in a lifecycle hook and
fired from event or animation callbacks, never from `build`/layout/paint (ADR-0078).

### 3. Delivery and lock discipline

- **User-visible effects are deferred.** `did_pop` runs inside the navigator flush under the
  non-reentrant history lock; an inline `on_remove` calling any `NavigatorHandle` method would
  deadlock. Entries popped by `did_pop` are recorded, and `on_remove` plus the emptied-edge
  `changed_internal_state` drain through `FlushOutcome::apply` after the lock is released —
  still synchronous within the caller's `pop()`, so Flutter's observable ordering holds.
- **The registry mutex is a leaf**: mutate under the lock, release, then fire. An atomic
  `removed` flag on the entry is the linearization point, so a `remove()` racing `did_pop`
  fires `on_remove` exactly once.
- **`maybe_pop` is one critical section.** Disposition and arming the pop happen inside one
  `mutate` closure; two acquisitions let a racing `remove()` turn an entry pop into a route
  pop.
- **No `Arc` cycles.** `on_remove` is consumed (`Option::take`) when it fires or at dispose,
  and the entry handle holds a `Weak` to the registry.

## Flutter divergences

- **`remove()` after the route is disposed is a no-op.** Flutter still fires `onRemove`
  because GC keeps the list alive; keeping callbacks past dispose is exactly the `Arc`-cycle
  leak above, so dispose severs them. Route dispose with live entries fires no `on_remove`,
  matching Flutter.
- **`add` on a popping or disposed route** is dropped with a `tracing::warn!`.
- **A route completed via `remove_route` and then popped** skips `did_pop` and leaves with
  live entries; Flutter asserts there.
- **Not carried:** `impliesAppBarDismissal`, the `persistentCallbacks` deferral (FLUI's
  rebuild inbox is already phase-safe), deprecated `willPop`, and the `NavigationNotification`
  that tells chrome outside the route to re-read `canPop`.

## Alternatives rejected

- **Storage on the erased `RouteEntry`/`RouteRecord`.** Flutter scopes this to `ModalRoute`;
  record-level storage gives every test-double route an entry stack and cannot reach
  `changed_internal_state`.
- **A declarative add-on-mount widget as the primary surface.** The Drawer's entry lives as
  long as the drawer is *open*, driven by animation callbacks while the controller stays
  mounted; a mount-scoped widget cannot express it. Widget sugar can be layered on later.
