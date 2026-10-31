### Added

- **Android keyboard motion** (`flui-native`): from API 30, `FluiView` observes the input method's
  insets animation, so the keyboard's insets carry its motion (start, length and the
  interpolator's curve) and each frame lays out with the keyboard where it is, instead of jumping
  to where it ends up.
