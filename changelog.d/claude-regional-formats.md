### Added

- **`flui-platform`**: `RegionalFormats` on `SystemPreferences`: the user's first weekday, hour
  cycle, measurement system, numbering system and region override, as the Unicode locale keys
  (`Weekday`, `HourCycle`, `MeasurementSystem`, `NumberingSystem`, `RegionOverride`).
  `RegionalFormats::apply_to` gives the locale a formatter takes. No host reads them yet
  ([ADR-0278](/docs/adr/ADR-0278-regional-formats-are-the-unicode-locale-extension-keys.md)).
- **`flui-widgets`**: `MediaQueryData::regional_formats` and `MediaQuery::regional_formats_of`.
