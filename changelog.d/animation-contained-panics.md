### Added

- **Competing panics during contained cleanup are reported** (`flui-animation`): a panic caught
  while a controller's delivery, its disposal or a Vsync tick is already recovering from an
  earlier one is reported on `flui_animation::contained_panic` (`CONTAINED_PANIC_TARGET`) with its
  `phase`, `payload` text and the controller's `label`. The first failure is still the one resumed.
- **`PanicRecovery::finish_reporting`** (`flui-foundation`): finishes a recovery after handing a
  callback the text of every panic caught after the first. A panicking callback is contained.
