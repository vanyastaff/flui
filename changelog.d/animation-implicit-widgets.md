### Added

- `AnimatedScale`, `AnimatedSlide`, `AnimatedPositioned`, `AnimatedCrossFade` (with `CrossFadeState`) and `AnimatedDefaultTextStyle` implicit widgets.
- `Animated<T>` animates any `TwoWayConverter` value, keeping its velocity, and builds its subtree from the value shown; `AnimatedLerp<T>` does the same for any `Lerp` value by re-anchoring from the value shown.
- `Lerp` for `TextStyle`, and in `flui-material` for `ColorScheme`, `TextTheme` and `ThemeData`; `TwoWayConverter` for `TranslationFraction`.
- `flui-material`: `AnimatedTheme`.

### Changed

- `MaterialApp` animates a theme change over 200 ms instead of switching it in one frame; under reduced motion it still switches at once.
