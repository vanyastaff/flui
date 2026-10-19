### Changed

- **`flui-sdk` paths follow the facade** (`flui-sdk`): `HapticsHandle` and `HapticObserver` are at
  `interaction::`, as in `flui`, instead of `platform::haptics::` (still under `unstable-input`).
  The `rendering::InputRegion*` re-exports are removed until a package uses them.
- **`LifecycleContext::haptics` and `drag_and_drop` have no default** (`flui-view`): the trait is
  sealed, so this changes no implementor outside the crate.

### Fixed

- **A lazily mounted sliver item handed back unchanged is not built again** (`flui-view`): the
  item's element now remembers the shared view it came from, as one mounted with its parent does.
- **The gesture overlay's bookkeeping is bounded** (`flui-devtools`): contact positions are kept
  only for retained contests and only while the overlay is shown, contests are no longer copied
  on every build, and the controller is not borrowed while the outgoing inspector is dropped.
