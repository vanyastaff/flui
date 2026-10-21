### Added

- **`ContextMenuArea`** (`flui-widgets`, ADR-0216): a region whose commands open in a theme-free
  context menu. The menu opens on every trigger of an `Alternative::ContextMenu` (a secondary
  click, a touch or pen long press, the menu keys, assistive technology's `ShowContextMenu` and
  the control keyboard focus reveals) at the pointer, or below the area for a key, flipping at
  the window's edges and mirrored in right-to-left locales. Inside it, Down and Up move between
  the items and wrap, Home and End go to the ends, and Tab stays inside; Escape or a tap outside
  closes it and returns the focus. Choosing an item closes the menu, then performs its command's
  intent through the `Actions` chain. Assistive technology reads a menu of menu items. The menu
  is a popup route, so the area needs a `Navigator` above it.
