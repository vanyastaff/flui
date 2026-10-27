### Added

- `PlatformWindow::set_material(WindowMaterial)` with `WindowCapabilities::material`: the
  material the window system draws behind a window, `WindowMaterial::Opaque` or
  `WindowMaterial::System` (Mica on Windows 11 build 22621 and later). Other hosts report it
  `Unavailable`; one OS's own materials stay with `flui::native::windows` and
  `flui::native::macos`.
- `WindowCapabilities::frame_rate_hint`: whether the window system takes a refresh-rate request.

### Changed

- `PlatformWindow::set_frame_rate_hint` and `PlatformWindow::set_input_regions` return
  `Result<(), Unsupported>` and have no default body: they answer `Unsupported` exactly when
  their capability (`WindowCapabilities::frame_rate_hint`,
  `InputCapabilities::input_regions`) is `Unavailable`.
- A window system that refuses a material answers `Unsupported` with `NotOnThisDevice` instead
  of logging.

### Removed

- `WindowBackgroundAppearance` (with `MicaBackdrop` and `MicaAltBackdrop`) and
  `PlatformWindow::set_background_appearance`: use `PlatformWindow::set_material`, or
  `Win32Window::set_backdrop` for Mica Alt and Acrylic.
