# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Type-State Builder Pattern** - `BuilderContextBuilder` with compile-time validation
  - Type states: `NoPlatform`/`HasPlatform`, `NoProfile`/`HasProfile`
  - Ensures all required fields are set before `build()`
  - Order-independent builder (can set platform or profile in any order)
  - Optional fields available at any state

- **Custom Error Types** - `BuildError` enum with 9 error variants
  - Rich error context with actionable messages
  - `BuildResult<T>` type alias for convenience
  - Helper constructors: `tool_not_found()`, `command_failed()`, etc.
  - `#[non_exhaustive]` for future compatibility
  - Implements `std::error::Error` with proper `source()` chaining

- **Extension Traits** - `BuilderContextExt` with 14 utility methods
  - Profile checks: `is_release()`, `is_debug()`
  - Platform checks: `is_android()`, `is_web()`, `is_desktop()`
  - Feature utilities: `has_feature()`, `has_any_feature()`, `has_all_features()`
  - Cargo argument generation: `cargo_args()`
  - Path utilities: `platform_output_dir()`

- **Comprehensive Documentation** - 1,352 lines of rustdoc
  - Crate-level documentation with examples
  - 38 documented code examples
  - Module-level documentation
  - All public items documented

- **Test Coverage** - 64 tests total
  - 26 unit tests
  - 38 documentation tests
  - All tests passing

- **API Guidelines Compliance** - 96% compliance (49/51 items)
  - Complete audit document: `API_GUIDELINES_AUDIT.md`
  - Follows Rust best practices

### Added (API Guidelines Phase 1)

- **Trait Implementations** - Enhanced interoperability
  - `PartialEq`, `Eq`, `Hash` for `Platform`
  - `Hash` for `Profile`
  - `PartialEq` for `BuilderContext`
  - `Default` for `Profile` (defaults to `Debug`)

- **From Conversions** - Convenient string conversions
  - `From<&str>` for `Profile` - Parse "release" or "debug"
  - `From<&str>` for `Platform` - Parse "android", "web", "desktop"

### Changed

- N/A (initial feature release)

### Deprecated

- N/A

### Removed

- N/A

### Fixed

- N/A

### Security

- N/A

## [0.1.0] - 2025-11-28

### Added

- Initial release
- Android build support via `AndroidBuilder`
- Web/WASM build support via `WebBuilder`
- Desktop build support via `DesktopBuilder`
- `PlatformBuilder` trait for extensibility
- `BuilderContext` for build configuration
- Environment validation
- Gradle integration for Android APK builds
- wasm-pack integration for Web builds

[Unreleased]: https://github.com/your-repo/flui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/your-repo/flui/releases/tag/v0.1.0
