### Added

- **`flui-platform`**: `UnsupportedReason::NotYetOnThisHost` says the operating system has what
  was asked for and this FLUI host does not implement it yet, apart from `NotOnThisPlatform` (no
  device of the platform has it) and `NotOnThisDevice` (this device or OS build lacks it). It is
  not retryable
  ([ADR-0152](/docs/adr/ADR-0152-capability-seam-revised.md)).

### Changed

- **`flui-platform`**: `WindowCapabilities::unavailable`,
  `unavailable_on_this_device` and `unavailable_not_yet_on_this_host` mark a command's capability
  `Unavailable` and record its lasting `UnsupportedReason`;
  `WindowCapabilities::command_capability` answers that reason as `Unsupported`. An absence counts
  for equality only while its capability is unavailable
  ([ADR-0254](/docs/adr/ADR-0254-window-contract.md)).
- **`flui-native`**: what the OS has and the host does not drive yet answers `NotYetOnThisHost`
  from the version that has it, and `NotOnThisDevice` below it: the window material on iOS, on
  Android (API 31, where the device blurs across windows) and on an AppKit window off the main
  lane; the macOS frame-rate hint (macOS 14); the pointer's shape on Android (API 24) and on an
  iPad (iOS 13.4); a `Win32Window` value newer than the host.
