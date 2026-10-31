### Added

- **Android keyboard and system gesture insets** (`flui-native`): an Activity's window reports
  the input method's occlusion (`WindowInsets.Type.ime()` from API 30, the system window insets
  over the stable ones below) and, from API 29, the system gesture regions, so
  `MediaQueryData::view_insets` and `system_gesture_insets` follow them and `Scaffold` resizes
  above the keyboard. The keyboard is reported at rest; its animation follows separately.
