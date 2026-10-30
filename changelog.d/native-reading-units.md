### Changed

- `flui_native::input::assembly::NativeReading::at` is split into `at_device(x, y, scale)` and
  `at_logical(x, y)`, so a reading names the unit its OS reports. AppKit, UIKit, the DOM and Wayland
  report points and place readings with `at_logical`.
