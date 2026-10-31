### Added

- **Android keyboard and system gesture insets** (`flui-native`): an Activity's window reports
  the input method's occlusion (`WindowInsets.Type.ime()` from API 30, below it the whole
  system window bottom once it exceeds the stable one) and, from API 29, the system gesture
  regions, so `MediaQueryData::view_insets` and `system_gesture_insets` follow them and `Scaffold`
  resizes above the keyboard. The keyboard is reported at rest; its animation follows separately.
