# ADR-0076: Public overlay mutation API

*The overlay mutation surface becomes public API. That covers the constructors (`OverlayHandle::new`, `Overlay::new`), `OverlayHandle::{insert, rearrange, is_mounted}` with a public `InsertPosition`, and the `OverlayEntry` lifecycle (`new`, `remove`, `mark_needs_build`, `set_opaque`, `set_maintain_state`, `is_attached`). It supersedes the "keep the mutation surface private" half of ADR-0036 §1, and closes ADR-0036's deferred "public `Overlay::new` constructor". The per-entry marker (`OverlayScope`) and the `Theater`/`OverlayShared`/`OnstagePlan` machinery stay private.*

---

- **Status:** Accepted
- **Date:** 2026-09-23
- **Deciders:** @vanyastaff
- **Supersedes:** ADR-0036 §1, the sentence keeping the existing mutation methods `pub(crate)`, and ADR-0036's Deferred item "a public `Overlay::new`/`initialEntries` constructor", as far as `Overlay::new` goes. The four published types and all of §2–§4 stand.
- **Scope:** `crates/flui-widgets/src/overlay/{mod.rs,entry.rs,tests.rs}` (visibility, rustdoc, one behaviour test); `crates/flui-widgets/src/lib.rs` (`InsertPosition` re-export; the module stays private); `crates/flui-widgets/src/testing/overlay_probe.rs` (test-only inspection); `crates/flui-widgets/src/navigator/navigator_tests.rs` (the export guard, rewritten for this contract)
- **Related:** ADR-0036 (overlay publication); ADR-0019 (Navigator routing seam); issue #1272 (the flui-widgets crate split that forces the question)

---

## Context

ADR-0036 published `Overlay`, `OverlayEntry`, `OverlayEntryId` and `OverlayHandle` so that `Overlay::of`/`maybe_of` could return a nameable handle. It kept every mutation method `pub(crate)`. Its only callers were in-crate (`Navigator` and `Draggable`'s feedback layer), and the shape of a public insert (Flutter's `above:`/`below:` named arguments or a Rust-native alternative) was a question it declined to settle as a side effect.

Two things have changed.

1. **The callers are leaving the crate.** Issue #1272 splits `flui-widgets` into widgets, text editing, scrolling and navigation. `Navigator`, `Hero`, the route machinery and `WidgetsApp` move to `flui-navigation`, and every one of them drives the overlay. The navigator owns and builds its overlay, route entries are inserted and rearranged, `opaque`/`maintainState` follow the route, and a hero flight inserts an entry above the navigator's. A `pub(crate)` surface cannot serve a sibling crate.
2. **The shape question is answered.** `InsertPosition { Top, Above(entry), Below(entry) }` has carried `insert`/`insert_all` for the whole life of the port. It makes Flutter's "not both `above` and `below`" assertion (`overlay.dart:661`) unrepresentable, and no caller has needed anything else.

Flutter itself publishes this surface (`OverlayState.insert/rearrange/mounted`, `OverlayEntry.remove/markNeedsBuild/opaque/maintainState`). `flui-material`'s menus and tooltips are built on exactly that API in the oracle.

## Decision

### 1. Publish exactly what the compiler says navigation uses

The items are re-exported from the crate root, with `InsertPosition` joining the four types ADR-0036 published. The `overlay` module itself stays private: a public module would also make `OverlayState` nameable, which is `pub` only because `StatefulView::State` must be. The set was not chosen by reading the code:

1. Every `pub(crate)` in `overlay/{mod,entry,theater}.rs` was narrowed to `pub(in crate::overlay)`.
2. `cargo check -p flui-widgets` then reported one privacy error (E0603/E0624) for each use outside the module.
3. The errors whose primary span lies in `navigator/**` or `app/widgets_app.rs` name these twelve items, and nothing else:

| Item | Flutter counterpart | Before mount / after unmount or removal |
|---|---|---|
| `OverlayHandle::new` | `GlobalKey<OverlayState>` + `Overlay(initialEntries:)` | An empty list no overlay has mounted |
| `Overlay::new(handle)` | `Overlay` widget | Mounting publishes the rebuild capability into the handle; dispose revokes it. One handle, one mounted `Overlay` (see §2a); rebuilding with a different handle moves onto it |
| `OverlayHandle::is_mounted` | `OverlayState.mounted` | `false` before the first mount and after dispose |
| `OverlayHandle::insert` | `OverlayState.insert` | Unmounted: the entry joins the list, nothing rebuilds (see §2). An entry another overlay holds, or this one already holds, is refused and logged (see §2a) |
| `OverlayHandle::rearrange` | `OverlayState.rearrange` | Unmounted: the list is reordered, nothing rebuilds (see §2). Foreign entries are refused, repeats count once |
| `InsertPosition` | `above:`/`below:` arguments | A reference the overlay does not hold falls back to `Top` |
| `OverlayEntry::new` | `OverlayEntry(builder:)` | Unattached; the builder runs on the layer's builds only |
| `OverlayEntry::remove` | `OverlayEntry.remove` | Always leaves the list; the rebuild runs only if mounted. This diverges from Flutter's `if (!overlay.mounted) return`, see §2. A second call logs `tracing::error!` and returns, per PANIC-POLICY (Flutter asserts) |
| `OverlayEntry::mark_needs_build` | `OverlayEntry.markNeedsBuild` | Inert before mount and after unmount |
| `OverlayEntry::set_opaque` | `OverlayEntry.opaque =` | Unattached or unmounted: stored and read by the next build; an unchanged value does nothing |
| `OverlayEntry::set_maintain_state` | `OverlayEntry.maintainState =` | Same as `set_opaque` |
| `OverlayEntry::is_attached` | `_overlay != null` | `true` from insert until remove, mounted or not; `false` once the overlay's handles are all dropped |

`OverlayHandle::is_mounted` and `OverlayEntry::is_attached` keep two names because they answer two questions. The first asks whether the overlay's state is alive. The second asks whether one entry is in an overlay's list, which it is before that overlay mounts and after it unmounts. Each one's rustdoc opens with the contrast. Renaming either later would be a breaking change.

### 2. Mutating an unmounted overlay changes the list; the next mount applies it

The handle, not the mounted `Overlay` view, owns the entry list. So `insert` and `rearrange` on a handle whose overlay is not mounted change the list and rebuild nothing, and an `Overlay` that mounts with the same handle later builds the list as it is then. The behaviour test `insert_on_an_unmounted_overlay_waits_for_the_next_mount` pins this.

This is the contract rather than a silent no-op or an error because the handle deliberately outlives any one mounted `Overlay`:

- **No-op:** a subtree that unmounts and remounts its overlay (a route shown again, a conditional branch toggled back) would silently lose what was inserted meanwhile.
- **Error:** it would make every caller race the tree's lifecycle, which is exactly what the handle exists to avoid.
- **Before first mount:** mutating before the first mount was already the documented contract of `OverlayHandle` ("the first build reads whatever the list holds"). The overlay tests build their overlays that way (`overlay_with` inserts, then mounts), so after unmount the same rule simply continues.

Flutter's equivalent is a `GlobalKey<OverlayState>` whose state has gone: there, the caller has nothing to insert into. The FLUI handle keeps the list, and the rebuild simply waits.

The same reason makes `remove` diverge from Flutter. Flutter's `OverlayEntry.remove` returns before touching the list when the overlay is unmounted, which is harmless there because that list dies with the state. Here the next mount would build a detached entry that nothing can remove anymore. So `remove` always takes the entry out of the list, and only the rebuild waits for a mount.

### 2a. The public API refuses the misuses a single in-crate caller never made

Before this ADR the navigator was the only caller, and it never misused the surface. Published, the surface has to hold for any caller. Each refusal below is logged with `tracing::error!` and never panics (PANIC-POLICY: caller error; Flutter `assert`s the same preconditions):

- **An entry lives in one overlay, once.** `insert` refuses an entry another live overlay holds, or one this overlay already holds. `rearrange` refuses foreign entries, and a repeat within one call counts once. Without this, a second insert re-pointed the entry's back-reference and left a ghost copy that `remove` could not reach.
- **One handle serves one mounted `Overlay`.** A second concurrent mount with the same handle builds nothing, and disposing it leaves the first one mounted. The rebuild slot is released only by the element that holds it.
- **An entry moved between overlays in one frame keeps its rebuild capability.** The entry's slot, like the overlay's, is released only by the view that published it, so the old overlay disposing its view cannot revoke the new one's.
- **A replacement handle takes over.** An `Overlay` rebuilt with a different handle releases the old handle's slot and claims the new one (`did_update_view`), so the new list is built and the old handle reports unmounted.

### 3. A public constructor: closing ADR-0036's deferral as a benefit

ADR-0036 deferred "a public `Overlay::new`/`initialEntries` constructor", because nothing outside the crate needed one. The Navigator moving crates is that need. With `OverlayHandle::new()` and `Overlay::new(handle)` public, an app author can build a standalone overlay the same way the Navigator does:

1. create a handle;
2. insert entries, before or after mounting;
3. build `Overlay::new(handle.clone())`.

`initialEntries` has no separate analogue, because inserting into the handle before mounting is that. `Overlay.wrap` stays deferred.

### 4. Bookkeeping stays private

The compiler list did not include `OverlayEntry::id`, `element_id` or `builder`; `OverlayHandle::{len, is_same, insert_all}`; the builder-form constructors; or `OverlayScope`, `Theater`, `OverlayShared`, `OnstagePlan` and `OverlayEntryViewState`. They stay `pub(crate)`, like their oracle counterparts (`_RenderTheaterMarker`, `_Theater`, the state internals). `Draggable`'s feedback layer, which stays in `flui-widgets`, keeps using what it uses in-crate.

### 5. Tests inspect through `testing::overlay_probe`

Tests that read the stacking order back (`entry_ids`, in the overlay's own tests and the navigator's hero tests) use the `OverlayProbe` extension trait from `flui_widgets::testing::overlay_probe`, behind the `testing` feature. The overlay's public API never exposes the list; nothing outside a test needs it.

## Consequences

- The overlay is now semver surface. Changing `InsertPosition` or the entry lifecycle is a breaking change.
- `rearrange`'s `above:`/`below:` placement of the unmentioned group stays deferred (the note on `rearrange`), because nothing needs it.
- `navigator_tests::overlay_publishes_the_lookup_and_mutation_contract` replaces ADR-0036's guard. It pins the published names, keeps the machinery out of the crate root's `pub use` lines, and asserts the module stays private.
- Behaviour tests pin §2 and §2a: `insert_on_an_unmounted_overlay_waits_for_the_next_mount`, `overlay_entry_remove_on_an_unmounted_overlay_takes_it_out_of_the_list`, `remove_before_the_first_mount_keeps_the_entry_out_of_the_first_build`, `an_entry_already_in_an_overlay_is_refused_elsewhere_and_twice`, `one_handle_serves_one_mounted_overlay`, `an_entry_moved_between_overlays_in_one_frame_keeps_rebuilding` and `a_replacement_handle_takes_over_the_mounted_overlay`.
