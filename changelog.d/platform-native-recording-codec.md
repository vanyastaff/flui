### Added

- **`flui-platform`**: native pointer recordings
  ([ADR-0215](/docs/adr/ADR-0215-input-recording-and-backend-conformance.md) §4). The
  `native-capture` feature (debug builds) journals every pointer-registry call on a thread through
  `shared::pointer_assembly::capture`, so a session on a native host becomes a checked-in
  recording; the tests replay Win32, AppKit, UIKit, Android, winit and web recordings through the
  shared decoder on any host.
