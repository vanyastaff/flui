### Changed

- **`InkWell` mounts one element for its behaviours** (ADR-0322): its tap, focus node and ring,
  activation bindings and interaction states sit on the catalogs' interactive node instead of a
  `GestureDetector`, `MouseRegion`, `Actions`, `Focus` and `FocusRing`, so every Material control
  built on it mounts 12 fewer elements and 7 fewer render nodes per control (an `InkWell` over a
  `Text` went from 15 elements, 9 render nodes, 200 allocations and 33 KB to 3, 2, 87 and 11.6 KB).
  Its constructor and methods are unchanged.
- **`InkWell` reports `Pressed` from the contact's down**, as Flutter's `handleTapDown` does, then
  holds it for 100 ms after the activation as before.
- **A disabled `InkWell` stops an activation intent** (Enter, Space) that reaches it from a focused
  descendant, instead of letting it activate an enclosing control (ADR-0322 §2).
