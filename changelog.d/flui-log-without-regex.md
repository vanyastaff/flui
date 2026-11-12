### Changed

- `flui-log` filters with `tracing_subscriber`'s `Targets` instead of `EnvFilter`, and the
  workspace no longer enables tracing-subscriber's `env-filter`, so an application links no
  regex engine. The facade's default graph has three packages fewer on every platform, and a
  counter's release binary is 0.37 MB smaller (wasm32 0.31 MB).
  `FilterConfig::env_filter`/`env_filter_from` and `LogConfig::env_filter` are now `targets`/
  `targets_from` and return `Targets`. A directive string is `target=level` pairs and one bare
  level; a span or field directive is refused with `DirectiveError::SpanOrField`, the new source
  of `FilterError::Environment` and `FilterError::Configured`.
