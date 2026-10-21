### Added

- `HeadlessPlatform::with_preferences` and `HeadlessPlatform::preference_source` (`flui-native`):
  the headless platform reports system preferences like a native source does, before any window,
  changes them through an owner wake, and can fail its next read
  ([ADR-0172](/docs/adr/ADR-0172-host-owned-system-preferences.md)).
