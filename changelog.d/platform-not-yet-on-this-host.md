### Added

- **`flui-platform`**: `UnsupportedReason::NotYetOnThisHost` says the operating system has what
  was asked for and this FLUI host does not implement it yet, apart from `NotOnThisPlatform` (no
  device of the platform has it) and `NotOnThisDevice` (this device or OS build lacks it). It is
  not retryable
  ([ADR-0152](/docs/adr/ADR-0152-capability-seam-revised.md)).

### Changed

- **`flui-platform`**: `WindowCapabilities::unavailable_on_this_device(command)` becomes
  `WindowCapabilities::unavailable(command, reason)`: one per-capability reason that admission
  answers, `NotOnThisDevice` or `NotYetOnThisHost` alike
  ([ADR-0254](/docs/adr/ADR-0254-window-contract.md)).
- **`flui-native`**: the window material on iOS and Android, and the frame-rate hint on macOS,
  answer `NotYetOnThisHost` instead of `NotOnThisPlatform` or `NotOnThisDevice`: the OS has them
  (`UIVisualEffectView`, Android 12's window background blur, `NSView.displayLink`), the host
  does not drive them yet.
