### Added

- **`flui-platform`**: `PreferenceParts`, `GestureParts` and `WheelParts`, the public fields of
  `SystemPreferences`, `GesturePreferences` and `WheelPreferences` (`into_parts`, and back through
  `PreferenceParts::try_into_preferences` with the same validation as the `with_*` methods). Every
  serialization of a preferences snapshot goes through them, so a new preference fails to compile
  in each mirror until it carries it.
- **`flui-platform`**: `DisplayParts` and `ColorParts`, the public fields of `DisplayInfo` (apart
  from its id) and of `DisplayColor` with its colour space, for the same reason
  (`DisplayInfo::into_parts`, `DisplayParts::into_info`).
- **`flui-native`**: `HeadlessDisplayControls::replace` reports a whole new set of displays, and
  `HeadlessDisplayControls::fail_next_read` fails the next display read, as a native display
  source does.
