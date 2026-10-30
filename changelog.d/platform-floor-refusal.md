### Added

- `LaunchError::UnsupportedSystem` and `flui::UnsupportedSystem`: a system below FLUI's floor fails `run_app` and `run_app_android` with a typed refusal naming the environment and either the release found and the floor, or the missing requirement (ADR-0244 §3).
- `flui_platform::Environment` and `flui_platform::OsRelease`, a release that is shown and never compared.
- `PlatformError::UnsupportedOs` and `PlatformError::MissingRequirement` in `flui-native`: Windows, macOS, iOS and Android refuse a release below the floor when the platform is built; the Wayland host refuses a compositor without `xdg_wm_base`, and the X11 host a server without RandR 1.5.

### Changed

- `flui_native::android_platform` returns `Result<Box<dyn Platform>, PlatformError>`.
