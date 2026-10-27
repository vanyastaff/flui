### Added

- `WindowCommand::SetMaterial(WindowMaterial)` with `WindowCapabilities::material`: the
  material the window system draws behind a window, `WindowMaterial::Opaque` or
  `WindowMaterial::System` (Mica on Windows 11 build 22621 and later, `NSVisualEffectView`'s
  window-background material on macOS). Other hosts report it `Unavailable`; one OS's own
  materials stay with `flui::native::windows` and `flui::native::macos`.
- `WindowCapabilities::frame_rate_hint`: whether the window system takes a refresh-rate request.
- `WindowCapabilities::unavailable_on_this_device` and `WindowCapabilities::refusal`: a capability
  this device lacks is refused at admission with `NotOnThisDevice`.
- `flui_platform::window_takes_no_frame_verbs!`, the per-frame verbs of a window test double
  whose window system takes neither.

### Changed

- `PlatformWindow::set_frame_rate_hint` and `PlatformWindow::set_input_regions` return
  `Result<(), Unsupported>` and have no default body: they answer `Unsupported` exactly when
  their capability (`WindowCapabilities::frame_rate_hint`,
  `InputCapabilities::input_regions`) is `Unavailable`.
- A window system that refuses a material answers `ApplyError::Platform` instead of logging.
- `Win32Window::set_backdrop` and `Win32Window::backdrop` answer `Unsupported` with
  `NotOnThisDevice` below Windows 11 build 22621.

### Removed

- `WindowBackgroundAppearance` (with `MicaBackdrop` and `MicaAltBackdrop`) and
  `PlatformWindow::set_background_appearance`: use `WindowCommand::SetMaterial`, or
  `Win32Window::set_backdrop` for Mica Alt and Acrylic.
