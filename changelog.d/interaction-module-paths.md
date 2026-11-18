### Removed

- **`flui-interaction`**: each public type has one path, in its module (`events`, `ids`, `focus`,
  `pointer`, `text_input`, `gesture`, `drag`, `binding`, `haptics`, `testing`). The crate root
  no longer re-exports them, and `flui_interaction::prelude` is gone. Types that other crates own
  are named from those crates: geometry and `RenderId` from `flui_foundation`, pointer samples,
  buttons and kinds from `flui_platform::pointer`. The `flui::interaction` facade is unchanged.

### Changed

- **`flui-interaction`**: the crate's id-keyed tables (focus, key ledger, pointer sequences,
  router, mouse tracker, interaction lane) hash with `rustc_hash::FxHashMap` instead of SipHash.
- **`flui-interaction`**: `MouseTracker::device_active_regions` returns the hovered regions as a
  `Vec` in hit-test order, the innermost first, instead of a `HashSet`, so no hasher reaches the
  public API.
