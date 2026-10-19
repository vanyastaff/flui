### Changed

- **`flui-interaction`**: `ArenaMembership::with_motion_role` and `holding_contact` each replace
  the other: a member either moves or holds its contact, never both.
- **`flui-interaction`**: `GestureSettings` keeps its authored values as one `GestureValues`,
  whose fields are now documented; `values()` and `try_from_values` are unchanged.

### Removed

- **`flui-interaction`**: `PointerClass` is no longer public; `GestureSettings::preset` still
  falls back to a pointer kind's precision class.

### Fixed

- **`flui-interaction`**: a hold records its contact before the tap and long-press members it beats
  hear their rejections. A beaten member whose callback panics no longer leaves the hold without
  a contact, and one that ends the contest no longer gets `on_hold` fired for it; `on_release`
  answers only a hold `on_hold` announced.
- **`flui-interaction`**: a standalone arena operation whose member panics still delivers the
  decisions it committed to `GestureObserver`s before the panic propagates.
- **`flui-interaction`**: a monotonic clock that panics while the arena stamps a committed verdict
  no longer loses it: members and observers hear it, then the clock's panic propagates.
- **`flui-interaction`**: a `PanClaim::None` region no longer refuses an axial member inside it
  that declares a region of its own; the claims between a free and an axial member stop at the
  first region that encloses both (ADR-0208 rule 1).
