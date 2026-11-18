### Changed

- A native recording reader refuses a device pixel ratio no screen has (zero, negative, or
  outside `DevicePixelRatio::MIN..=MAX`) on the line that holds it, wherever a row holds one (a
  pointer reading's scale, a wheel's, a touchpad viewport's, a display's scale factor, a window's
  metrics, the system's mouse and touch geometry), with the typed `CodecError::Ratio`. Writers
  write format version 4; readers read versions 3 and 4, whose fields are the same.
- An input recording refuses such a ratio the same way: `RecordingError::Ratio` (and
  `WireError::Ratio` from `flui-runtime`) for a window's scale, a metrics observation's and the
  system's mouse or touch geometry's, where it was a malformed-row error before.
- `WindowMetrics::try_new` and `WindowMetrics::from_logical` take a `DevicePixelRatio` instead
  of an `f64`; `InvalidMetrics::InvalidScale` is gone, since the ratio is checked where a host
  reads it.
- `HeadlessHost::try_set_scale_factor`, the widget tester's `try_set_scale_factor` and
  `TestWindow::with_scale_factor` take a `DevicePixelRatio`.
- A native pointer reading, wheel reading and touchpad frame carry their scale as a
  `DevicePixelRatio` instead of an `f64`; `DecodeFault::Scale` is gone, since no reading can
  hold a scale no screen has.
