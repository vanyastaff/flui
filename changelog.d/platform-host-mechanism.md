### Added

- `flui_native::host`: the table a host fills, `Host`, with one type for each position of the
  closed list, and `Absent<F>` for a position a platform does not have. A table that leaves a
  position out does not compile.
- `flui_native::HostPlatform<H>`, the one implementation of `Platform`.

### Changed

- `WindowsPlatform`, `MacOSPlatform`, `IOSPlatform`, `AndroidPlatform`, `WebPlatform`,
  `WinitPlatform` and `HeadlessPlatform` are `HostPlatform` over each host's table. Their
  constructors and methods keep their signatures and behaviour.
- `Platform`'s methods have no default bodies: each host's table answers every one.

### Removed

- The paths `flui_native::platforms::{WindowsPlatform, MacOSPlatform, IOSPlatform,
  AndroidPlatform, WebPlatform, WinitPlatform, HeadlessPlatform}` and their per-OS modules'
  re-exports; the crate-root names remain. `flui_native::platforms::android` is no longer public.
