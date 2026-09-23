# ADR-0076: Public overlay API — lookup and mutation

- **Status:** Accepted
- **Date:** 2026-09-23
- **Absorbs:** ADR-0036

## Context

`crates/flui-widgets/src/overlay/` is a port of Flutter's `Overlay`/`OverlayEntry`/`_Theater`.
`Navigator` mounts one and builds every route inside its entries; `Draggable` paints its
feedback in one; a hero flight inserts an entry above the navigator's.

Until 2026-07 the whole module was `pub(crate)`. Two needs opened it:

1. **Lookup.** A descendant (first `Draggable`) must find the nearest overlay, as
   `Overlay.of(context)` does in Flutter.
2. **Mutation.** Overlays are the building block for menus, tooltips, snackbars, dialogs and
   custom routes. `flui-material`, `flui-cupertino` and app code live outside `flui-widgets`
   and need to create overlays, insert and rearrange entries, and drive an entry's lifecycle.
   Flutter publishes exactly this surface (`OverlayState.insert/rearrange/mounted`,
   `OverlayEntry.remove/markNeedsBuild/opaque/maintainState`), and its material menus and
   tooltips are built on it.

The first version of this ADR was motivated by a proposed split of `flui-widgets` into
widgets, text, scrolling and navigation crates (#1272). That split was rejected:
`flui-widgets` stays one crate, and `Navigator` stays in-crate. The public surface stands on
the second need alone.

## Decision

### 1. The published surface

Re-exported from the `flui-widgets` crate root: `Overlay`, `OverlayEntry`, `OverlayEntryId`,
`OverlayHandle`, `InsertPosition`. The `overlay` module itself stays private (a public module
would also make `OverlayState` nameable, which is `pub` only because `StatefulView::State`
must be).

| Item | Flutter counterpart | Before mount / after unmount or removal |
|---|---|---|
| `Overlay::of` / `Overlay::maybe_of` | `Overlay.of` / `Overlay.maybeOf` | Nearest enclosing overlay (§4) |
| `OverlayHandle::new` | `GlobalKey<OverlayState>` + `Overlay(initialEntries:)` | An empty list no overlay has mounted |
| `Overlay::new(handle)` | `Overlay` widget | Mounting publishes the rebuild capability into the handle; dispose revokes it. One handle, one mounted `Overlay` (§2a); rebuilding with a different handle moves onto it |
| `OverlayHandle::is_mounted` | `OverlayState.mounted` | `false` before the first mount and after dispose |
| `OverlayHandle::insert` | `OverlayState.insert` | Unmounted: the entry joins the list, nothing rebuilds (§2). An entry another overlay holds, or this one already holds, is refused and logged (§2a) |
| `OverlayHandle::rearrange` | `OverlayState.rearrange` | Unmounted: the list is reordered, nothing rebuilds. Foreign entries are refused, repeats count once |
| `InsertPosition { Top, Above(e), Below(e) }` | `above:`/`below:` arguments | A reference the overlay does not hold falls back to `Top` |
| `OverlayEntry::new` | `OverlayEntry(builder:)` | Unattached; the builder runs on the layer's builds only |
| `OverlayEntry::remove` | `OverlayEntry.remove` | Always leaves the list; the rebuild runs only if mounted (§2). A second call logs `tracing::error!` and returns (Flutter asserts) |
| `OverlayEntry::mark_needs_build` | `OverlayEntry.markNeedsBuild` | Inert before mount and after unmount |
| `OverlayEntry::set_opaque` / `set_maintain_state` | `opaque =` / `maintainState =` | Stored and read by the next build; an unchanged value does nothing |
| `OverlayEntry::is_attached` | `_overlay != null` | `true` from insert until remove, mounted or not |

`InsertPosition` replaces Flutter's two optional named arguments and makes its "not both
`above` and `below`" assertion unrepresentable.

`is_mounted` and `is_attached` are two names for two questions: is the overlay's state alive,
and is this entry in some overlay's list (which it is before that overlay mounts and after it
unmounts).

### 2. Mutating an unmounted overlay changes the list; the next mount applies it

The handle, not the mounted `Overlay` view, owns the entry list. `insert` and `rearrange` on a
handle whose overlay is not mounted change the list and rebuild nothing; an `Overlay` that
later mounts with that handle builds the list as it is then. Inserting before the first mount
is how `initialEntries` is expressed, so there is no separate constructor argument.

For the same reason `remove` diverges from Flutter. Flutter's `OverlayEntry.remove` returns
early when the overlay is unmounted, harmless there because the list dies with the state. Here
the next mount would build a detached entry nothing could remove, so `remove` always takes the
entry out of the list and only the rebuild waits for a mount.

### 2a. Misuse is refused, logged, never a panic

Each refusal logs `tracing::error!` (a caller error under `docs/PANIC-POLICY.md`; Flutter
asserts the same preconditions):

- **An entry lives in one overlay, once.** `insert` refuses an entry another live overlay holds
  or this overlay already holds; `rearrange` refuses foreign entries and counts a repeat once.
- **One handle serves one mounted `Overlay`.** A second concurrent mount with the same handle
  builds nothing; the rebuild slot is released only by the element that holds it.
- **An entry moved between overlays in one frame keeps its rebuild capability.** Its slot is
  released only by the view that published it.
- **A replacement handle takes over.** An `Overlay` rebuilt with a different handle releases
  the old handle's slot and claims the new one.

### 3. Bookkeeping stays private

`OverlayEntry::{id, element_id, builder}`, `OverlayHandle::{len, is_same, insert_all}`, the
builder-form constructors, `OverlayScope`, `Theater`, `OverlayShared`, `OnstagePlan` and
`OverlayEntryViewState` stay `pub(crate)`, like their Flutter counterparts
(`_RenderTheaterMarker`, `_Theater`). Tests read stacking order through
`flui_widgets::testing::overlay_probe::OverlayProbe` (feature `testing`); the public API never
exposes the list.

### 4. Lookup: a per-entry `OverlayScope` marker resolved with `depend_on`

`OverlayEntryViewState::build` wraps each entry's built child in `OverlayScope`, a private
`InheritedView<Data = OverlayHandle>` carrying the enclosing overlay's handle — the analogue of
Flutter 3.44's `_RenderTheaterMarker`, not the older `findAncestorStateOfType<OverlayState>()`
walk. Because the marker is per entry, a descendant of a nested overlay resolves the nearest
one with no extra code.

`Overlay::maybe_of` resolves through `depend_on`, which registers a dependency. **This diverges
from Flutter**, whose `Overlay.maybeOf` passes `createDependency: false`. The reason is
FLUI-specific: `Draggable`'s drag session is a `Send + Sync` gesture object with no
`BuildContext`, so it cannot re-run the lookup mid-drag as Flutter's `_DragAvatar` does. The
handle is resolved ahead of time in `init_state`/`did_change_dependencies` and cached; the
dependency re-notifies the state if a different overlay identity replaces the resolved one
(`OverlayScope::update_should_notify` compares handles with `Arc::ptr_eq`).

The lookup is an ordinary inherited read, like `Theme::of`, not a lifecycle capability.

## Consequences

- The overlay is semver surface: changing `InsertPosition` or the entry lifecycle is a
  breaking change.
- Material and Cupertino menus, tooltips and dialogs can be built outside `flui-widgets` on the
  same API the navigator uses.
- `navigator_tests::overlay_publishes_the_lookup_and_mutation_contract` pins the published
  names and the module's privacy; the overlay tests pin §2, §2a and nearest-wins lookup.

## Deferred

- `rootOverlay` (walk past nested overlays to the outermost); `LookupBoundary`.
- `OverlayPortal`, the declarative alternative to holding entry handles.
- `Overlay.wrap`, and `rearrange`'s `above:`/`below:` placement of the unmentioned group.

## Alternatives rejected

- **Error or no-op on mutation of an unmounted overlay.** A no-op loses entries inserted while
  a route or branch is temporarily unmounted; an error makes every caller race the tree's
  lifecycle, which the handle exists to avoid.
- **A lookup-only `get` for `Overlay::maybe_of`** (Flutter's shape). The cached handle in a
  context-free gesture session would go stale silently.
- **Publishing the `overlay` module.** It would expose `OverlayState` and the theater
  machinery as semver surface.
