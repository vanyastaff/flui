### Changed

- `flui_engine::SlotRecord` publishes a slot's surface state as one `SlotSurface`: whether the slot
  holds a surface (`SurfaceHold::Held`, `Wanted`, `Refused`) and its newest size with the backend's
  answer (`SlotSize::Unsized`, `Pending`, `Applied`, `Refused`). It replaces the
  `wants_surface` and `surface_refused` flags.

### Fixed

- A desktop window whose size the device refused heard that refusal even when another failure of
  its slot was held in the same turn, and a window is no longer held to a size refusal of a device
  that was rebuilt since: the window reads its slot's surface state instead of the failure it took.
