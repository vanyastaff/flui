### Added

- The facade's `http` feature turns on the built-in `HttpCapability` provider (each host's own
  HTTP stack, `ureq` over rustls on Linux). It is off by default, so an application that sends no
  request links no HTTP client or TLS stack.

### Changed

- `flui create` writes `[profile.dev.package."*"] opt-level = 3` into every application template,
  so a new project's debug build runs its dependencies (wgpu, naga, Parley, lyon) optimized, as
  the FLUI workspace does.
- `flui create` templates strip symbols from release builds (`strip = "symbols"`): a panic still
  prints its message and `file:line`, only backtrace frame names are lost. A `profiling` profile
  inherits release and keeps symbols and line tables for profilers and symbolized backtraces.

### Removed

- `flui-app` no longer turns `http` on by default; an application that uses `HttpCapability`
  names `flui/http`.
- Features that enabled nothing: the facade's `localizations` and `gpu-readback-tests`, `signals`
  in `flui-view`, `flui-widgets`, `flui-app` and `flui-testing`, `flui-app`'s `desktop`,
  `android`, `ios`, `web`, `debug-overlay` and `performance-overlay`, and `flui-native`'s `web`.
  A manifest that names one of them drops it.
