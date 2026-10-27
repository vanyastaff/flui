### Added

- **`flui-platform`**: `UnsupportedReason::NotYetOnThisHost` says the operating system has what
  was asked for and this FLUI host does not implement it yet, apart from `NotOnThisPlatform` (no
  device of the platform has it) and `NotOnThisDevice` (this device or OS build lacks it). It is
  not retryable
  ([ADR-0152](/docs/adr/ADR-0152-capability-seam-revised.md)).
- **`flui-platform`**: `CapabilityAbsence` (`NotOnThisPlatform`, `NotOnThisDevice`,
  `NotYetOnThisHost`), where a window capability is missing, converting to the reason a refusal
  answers ([ADR-0254](/docs/adr/ADR-0254-window-contract.md)).

### Changed

- **`flui-platform`**: `WindowCapabilities::unavailable(command, absence)` marks a command's
  capability `Unavailable` with the `CapabilityAbsence` admission answers, in place of
  `unavailable_on_this_device`; `WindowCapabilities::refusal` returns the `CapabilityAbsence`.
  An absence counts for equality only while its capability is unavailable.
- **`flui-native`**: what the OS has and the host does not drive yet answers `NotYetOnThisHost`
  from the version that has it, and `NotOnThisDevice` below it: the window material on iOS, on
  Android (API 31, where the device blurs across windows) and on an AppKit window off the main
  lane; the macOS frame-rate hint (macOS 14); the pointer's shape on Android (API 24) and on an
  iPad (iOS 13.4); a `Win32Window` value newer than the host.
