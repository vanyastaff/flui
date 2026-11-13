### Changed

- `flui_engine::SlotRecord` publishes a slot's surface state as one `SlotSurface`: whether the slot
  holds a surface (`SurfaceHold::Held`, `Wanted`, `Restoring`, `Refused`) and its newest size with the backend's
  answer (`SlotSize::Unsized`, `Pending`, `Applied`, `Refused`). It replaces the
  `wants_surface` and `surface_refused` flags.

### Fixed

- A desktop window hears that the device refused its size even when another failure of
  its slot was held in the same turn, and a window is no longer held to a size refusal of a device
  that was rebuilt since: the window reads its slot's surface state instead of the failure it took.
- A window over a rebuilt device's size limit, or restored over it, no longer draws frames on its
  unconfigured surface; a resize error other than a size past the limit is retried instead of
  holding the window at that size; and a window waiting for its new surface no longer makes
  another one on each frame before the first is answered.
