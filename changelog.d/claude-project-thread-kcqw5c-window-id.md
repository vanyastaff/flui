### Added

- `flui_platform::WindowIdMint`, which issues a host's `WindowId`s in increasing order from 1,
  never one twice, and refuses with `IdExhausted` instead of wrapping.
- `OpenWindowError::IdsExhausted`: a host that issued every window id refuses to open another.
- `WindowId::try_from_raw` under `flui-platform`'s `testing` feature, for recordings and tests;
  it answers `None` for 0.
- `HeadlessHost::window_of` in `flui-testing`, and the window conformance check
  `a_window_opened_after_a_close_has_an_id_of_its_own` with `WindowFixture::open_another`.

### Changed

- `WindowId` is opaque: its field is a private `NonZeroU64`, read with `get()`. `WindowId::new`,
  `as_u64` and the public tuple field are gone, so no crate builds an id from a number.
- `flui-runtime`'s `TestWindow::with_id` takes a `WindowId`.

### Fixed

- A Win32 window's id is no longer its HWND and a macOS window's no longer its `NSWindow` address:
  each host issues ids from its own mint and keeps a handle → id map, so an event for a closed
  window can no longer reach a new window that reused the handle. `active_window` answers `None`
  for a foreground or key window the platform did not open.
- Android and the Web gave every window the same fixed id (and Android's `active_window` named
  another id than the window's own); each window now has an id of its own.
- The headless platform's first window had id 0; headless ids now start at 1, and exhaustion
  refuses the open with `OpenWindowError::IdsExhausted` instead of panicking.
- A recording that names window 0 is refused on its line instead of replaying to no window.
- `HeadlessHost::open_window` reissued an open window's id after a secondary window closed.
