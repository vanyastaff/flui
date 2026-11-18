### Changed

- The native recording format is version 4: a recording reader refuses a device pixel ratio no
  screen has (zero, negative, or outside `DevicePixelRatio::MIN..=MAX`) on the line that holds
  it, wherever a row holds one (a pointer reading's scale, a wheel's, a touchpad viewport's, a
  display's scale factor, a window's or the system's pixel ratio), with the typed
  `CodecError::Ratio`. Version 3 recordings are refused as another version.
- A native pointer reading, wheel reading and touchpad frame carry their scale as a
  `DevicePixelRatio` instead of an `f64`; `DecodeFault::Scale` is gone, since no reading can
  hold a scale no screen has.
