### Added

- **`flui-native`**: the Windows host reads the user's regional formats from Region settings
  (first weekday, 12- or 24-hour clock, units, native digits and the formats region), and the
  Linux hosts read them from `LC_TIME`, `LC_MEASUREMENT` and GNOME's `clock-format`
  ([ADR-0278](/docs/adr/ADR-0278-regional-formats-are-the-unicode-locale-extension-keys.md)).
