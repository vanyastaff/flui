### Added

- `AppLaunch`, `AppCapabilities` and `LaunchError` (`flui`, `flui_app`): a run starts with an `AppLaunch` carrying its `AppConfig` and the capability overrides and plugins it registers (ADR-0152), on every platform, Android, iOS and the Web included.

### Changed

- `run_app(root, launch)` and `run_app_android(app, root, launch)` are the one entry point per platform: they take an `AppLaunch` and return `Result<(), LaunchError>`, with a capability conflict found before the platform starts.

### Removed

- `run_app_with_config` and `run_app_android_with_config`: pass `AppLaunch::new().config(config)` to `run_app` or `run_app_android`.
