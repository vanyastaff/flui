### Fixed

- `flui devices` and `flui emulators` on macOS retry a `xcrun simctl list` that does not answer
  within 10 s once, with 30 s, so a simulator whose CoreSimulator service is slow after its boot
  is listed instead of left out.
