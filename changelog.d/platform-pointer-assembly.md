### Changed

- Every native backend (Win32, winit, AppKit, UIKit, Android, Web) assembles
  pointers through one shared registry in `flui_platform::shared::pointer_assembly`:
  contact and device identities are minted and never reissued, a mouse keeps one
  identity across presses and visits to the window, a pen's barrel button changes
  within its sequence without starting or ending it, and a device first seen
  through an event is announced before that event.
- A reading that cannot be decoded cancels its open contact with
  `CancelReason::InvalidInput`; a release, leave or cancel for no open contact
  emits nothing. A coalesced batch with one undecodable reading cancels the
  contact instead of dropping that reading.

### Added

- Win32 mouse presses carry a click count from the OS double-click interval and
  rectangle, refreshed when those settings or the window's DPI change.
