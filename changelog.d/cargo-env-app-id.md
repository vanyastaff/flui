### Added

- `flui create` writes the application's identifier into the project's `.cargo/config.toml`
  (`[env] FLUI_APP_ID`), and `flui build` and `flui run` keep that one key in line with
  `flui.toml`, adding it to a project created earlier. `AppConfig::default()` carries it as
  `AppIdentity::app_id`, so an application built by any cargo invocation in the project, an
  editor's included, has storage under its own directory without further setup.
