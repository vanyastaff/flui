### Added

- On Android from API 29, `flui_native::fonts::system_fallback` asks the system's font matcher (`AFontMatcher`) which face draws each script and emoji in the locale, instead of reading `fonts.xml`, which the platform does not support parsing there (ADR-0271).
