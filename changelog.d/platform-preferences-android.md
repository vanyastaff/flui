### Added
- On Android the host reports high contrast, from the system contrast level (API 34+) and the high-contrast-text setting, and the preferred languages, from the configuration's `LocaleList`.

### Changed
- The Android host no longer samples preferences, scroll factors and insets every 500 ms while resumed. `FluiView` reports setting, contrast and inset changes through content observers, the API 34 contrast listener and `onApplyWindowInsets`; an idle app makes no loop turn. The cadence remains only while the view is not in the window.
