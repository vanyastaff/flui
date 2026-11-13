### Added

- `TextureUpdates::merge` folds a later update into an earlier one, so the pair applies as both
  would in turn.
- `SceneSnapshot::textures` carries a presentation's external-texture frames and retirements to
  the raster executor, which applies them to the slot's own textures before the frame renders,
  shown or not; a frame that supersedes an unshown one carries that one's updates too.
- `SlotBackend::apply_textures`. `GpuContext` keeps each slot's textures apart, since every
  presentation mints its own `TextureId`s, lends them to the shared painter for the slot's frame,
  uploads them again on a rebuilt device and drops them when the slot detaches.
  `testing::ScriptedSlots` records each call (`textures`, `script_textures`).
