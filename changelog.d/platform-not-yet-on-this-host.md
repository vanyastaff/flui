### Added

- **`flui-platform`**: `UnsupportedReason::NotYetOnThisHost` says the operating system has what
  was asked for and this FLUI host does not implement it yet, apart from `NotOnThisPlatform` (no
  device of the platform has it) and `NotOnThisDevice` (this device or OS build lacks it). It is
  not retryable
  ([ADR-0152](/docs/adr/ADR-0152-capability-seam-revised.md)).

### Changed

- **`flui-platform`**: `WindowCapabilities::unavailable(command, &reason)` marks a capability
  `Unavailable` with the reason admission answers, `NotOnThisDevice` or `NotYetOnThisHost` alike,
  in place of `unavailable_on_this_device`
  ([ADR-0254](/docs/adr/ADR-0254-window-contract.md)).
- **`flui-native`**: what the OS has and the host does not drive yet answers `NotYetOnThisHost`
  instead of `NotOnThisPlatform` or `NotOnThisDevice`: the window material on iOS
  (`UIVisualEffectView`), on Android (window background blur) and on a macOS window owned off the
  main lane; the frame-rate hint on macOS (`NSView.displayLink`); the pointer icon on Android
  (`PointerIcon`); input regions under winit on Linux (Wayland and X11 input shapes); a
  `Win32Window` value newer than the host.
