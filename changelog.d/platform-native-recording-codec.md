### Added

- **`flui-native`**: native pointer recordings
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md) §4). The
  `native-capture` feature (debug builds) journals every pointer-registry call on a thread through
  `shared::pointer_assembly::capture`, so a session on a native host becomes a checked-in
  recording; the tests replay Win32, AppKit, UIKit, Android, winit and web recordings through the
  shared decoder on any host.

### Changed

- **`flui-native`**: a lifted pen keeps hovering on its pointer identity until the platform's
  leave, so its `Enter` and `Leave` pair up; a touch still ends at `Up`, and one that entered
  delivers its `Leave` there (or at a cancel)
  ([ADR-0217](/docs/adr/ADR-0217-interaction-module-layers-and-pointer-assembly.md) §3).
