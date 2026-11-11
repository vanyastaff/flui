### Added

- `flui-engine`: `GpuContext::bootstrap` makes the shared GPU context and its `SurfaceFactory` on
  the host's `DisplayConnection`. The factory stays on its owner thread and refuses a window on
  another display with `EngineError::SurfaceOnForeignDisplay` before wgpu sees it; its
  `UnconfiguredSurface`s attach to the raster executor as window slots, which take
  `Release`, `Restore` and `Configure` with a display colour or a frame latency.
- `flui-engine`: a window slot whose surface stays lost after a reconfigure drops it and answers
  `EngineError::SurfaceReplacementNeeded`; `SlotRecord::wants_surface` asks the owner for a new
  surface, which it makes with its factory and sends in a `Restore`.
- `flui-engine` (`testing`): `noop_surface_factory`, `FakeDisplay` and `FakeWindow`, window surfaces from wgpu's no-op backend, so
  a test drives a window slot's commands without a GPU or a window.

### Changed

- `flui-engine`: `SlotBackend` gains `release`, `restore`, `set_display_color` and
  `set_latency`; `SlotTarget` is no longer `Copy`, since a window target owns its surface. A
  resize the backend always refuses is no longer retried every turn: the slot keeps its previous
  configuration and the executor warns once for that size.

### Fixed

- `flui-engine`: slots of different surface formats on one context each get a blit pipeline of
  their own format instead of sharing the first slot's.
