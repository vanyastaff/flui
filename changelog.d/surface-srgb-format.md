### Fixed
- A window surface that offers only `Bgra8UnormSrgb`, `Rgba8UnormSrgb` or `Rgb10a2Unorm` for sRGB now renders instead of failing renderer construction with `UnsupportedSurfaceColorConfiguration`: the frame is painted into an `Rgba8Unorm` working target and converted at presentation.
