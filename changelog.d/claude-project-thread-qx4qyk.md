### Changed

- **`InkWell` mounts one element for its behaviours** (ADR-0322): its tap, focus node and ring,
  activation bindings and interaction states sit on the catalogs' interactive node instead of a
  `GestureDetector`, `MouseRegion`, `Actions`, `Focus` and `FocusRing`, so every Material control
  built on it mounts 12 fewer elements and 7 fewer render nodes per control (an `InkWell` over a
  `Text` went from 15 elements, 9 render nodes, 200 allocations and 33 KB to 3, 2, 83 and 11.7 KB).
  Its constructor and methods are unchanged.
- **`InkWell` reports `Pressed` from the contact's down**, as Flutter's `handleTapDown` does, then
  holds it for 100 ms after the activation as before, now through a hold the states controller
  counts beside the contact, so the hold follows a controller handed over under it.
- **A disabled `InkWell` stops an activation intent** (Enter, Space) that reaches it from a focused
  descendant, instead of letting it activate an enclosing control (ADR-0322 §2).

### Fixed

- `FloatingActionButton` is a button to assistive technology, as Flutter's is: one node of the
  button role that offers the click while `on_pressed` is set and says it is disabled without one.
- A labelled `Icon` or `AnimatedIcon` names the button or other control it sits in, as Flutter's
  does: it declares no image role of its own, so an `IconButton`, a floating action button or an
  `ElevatedButton` over a labelled icon is one button node with that name instead of a nameless
  button over an image.
- A `ListTile` without `on_tap` is enabled or disabled as `ListTile::enabled` says, to assistive
  technology too; a disabled one is a node of its own, so its state no longer reaches the list it
  sits in. It still takes no focus and tracks no pointer. The tile describes itself on its well,
  one element and one render node fewer per row.
- `Checkbox`, `Radio` and `Switch` turned off or on resolve their colors from the new state in the
  frame that turns them, building once instead of twice (one under a pointer still builds twice,
  as the hover it loses is written during that build).
