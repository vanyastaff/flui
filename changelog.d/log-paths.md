### Changed

- **`flui-log`**: each item has one path, through its module (`flui_log::config::LogConfig`,
  `flui_log::filter::FilterConfig`, `flui_log::ownership::SubscriberPolicy`,
  `flui_log::identity::AppIdentity`, `flui_log::backend::PlatformLayer`); the root keeps
  `setup`, `setup_with_env_fallback` and `ResilientSetup`. The field-privacy contract moved from
  `flui_log::backend::privacy` to `flui_log::privacy`.
- **`flui-log`**: the capture in `flui_log::testing` is behind the `testing` feature, which
  `flui-testing` turns on for `flui_testing::log_capture`. `CapturedRecord` is
  `#[non_exhaustive]`.
- **`flui-log`**: `FilterConfig`'s fields are private; it is built with `new`, `env_var` and
  `without_env_var`.

### Removed

- **`flui-log`**: `LogConfigBuilder::directives` (use `.filter(FilterConfig::new(..))`; it also
  re-enabled `RUST_LOG` after `without_env_var`), `LogConfig::targets` (use
  `config.filter().targets()`), `LogConfig::platform_layer`, the `PlatformLayer` constructors
  `desktop`, `desktop_compact_stderr`, `logcat` and `web_console` (use `platform_default`;
  `apple_unified_logging` stays as the macOS opt-in), `PlatformLayer::backend_name`, the
  Android-only `report_to_logcat`, and the public `LogcatPriority`, `FieldPrivacy`, `FieldKind`,
  `EventOrigin` and `REDACTED_VALUE` re-export (the placeholder is
  `flui_foundation::diagnostics::REDACTED_VALUE`).

### Fixed

- **`flui-log`**: on Android and iOS a field's `Debug` implementation runs once per event, not
  twice: the redaction stage classifies an event by its field names before rendering any value,
  and an event with nothing to redact reaches the sink untouched.
