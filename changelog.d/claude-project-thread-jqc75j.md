### Added

- **`flui-platform`**: `PreferenceParts`, `GestureParts` and `WheelParts`, the public fields of
  `SystemPreferences`, `GesturePreferences` and `WheelPreferences` (`into_parts`, and back through
  `PreferenceParts::try_into_preferences` with the same validation as the `with_*` methods). Every
  serialization of a preferences snapshot goes through them, so a new preference fails to compile
  in each mirror until it carries it.
