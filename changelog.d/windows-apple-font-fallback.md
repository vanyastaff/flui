### Added

- `flui_native::fonts::system_fallback` answers on Windows, from DirectWrite's system font fallback, and on macOS and iOS, from CoreText's choice per script and its cascade list for the locale, so text in a script FLUI's tables miss draws with the face the system would use (ADR-0271).
