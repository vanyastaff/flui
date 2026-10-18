### Added

- **Display snapshot** (`flui-platform-api`, `flui-platform`): `Platform::display_snapshot` returns an
  immutable `DisplaySnapshot` of every display: id, name, bounds, usable bounds, scale factor and the
  refresh period of the display's current mode. The period is a validated `RefreshPeriod` (1 ms to
  1 s) and `None` when the OS reports nothing that describes a cadence; it is never a guessed 60 Hz.
  A window names its display with `PlatformWindow::display_id` and hints at a change with
  `PlatformWindow::on_display_changed`.
- **Framework-issued display ids** (`flui-platform-api`): `DisplayId` is opaque and non-zero, issued by
  a `DisplayRegistry` from the OS's own output identity (`DisplayKey`). The same output keeps its id
  within a session, two identical monitors differ, and an exhausted id space refuses new displays
  instead of wrapping. It cannot be built from a number.
- **Injectable displays in the headless platform** (`flui-platform`): `HeadlessPlatform::with_displays`
  takes `DisplaySpec`s, `HeadlessDisplayControls::simulate_display_change` switches a display's mode,
  and `MockWindow::simulate_move_to` moves a window between displays.

### Changed

- **Breaking: `PlatformDisplay`, `Platform::displays`, `Platform::primary_display`,
  `PlatformWindow::display` and `PlatformWindow::refresh_period` are removed** (`flui-platform-api`,
  `flui-platform`). Read the snapshot and `PlatformWindow::display_id` instead; `DisplayId` no longer
  has a public field and `MockDisplay` is gone. `PlatformError` gains a `Displays` variant.
- **The macOS and winit backends feed the snapshot** with the refresh period they already read, so
  120 and 144 Hz panels keep their real period. Windows, Android, iOS and the Web report an unknown
  period until their native sources land; Windows no longer answers the primary display for every
  window.
