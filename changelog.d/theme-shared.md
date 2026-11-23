### Changed

- `MaterialApp::theme`, `dark_theme`, `high_contrast_theme`, `high_contrast_dark_theme` and `AnimatedTheme::new` take `impl Into<Rc<ThemeData>>`; the selected theme reaches `Theme::of` as the `Rc` the app was given, instead of being copied on every build (a `MaterialApp` is now 200 bytes, not 53 KB).

### Added

- `Lerp` for `Rc<T: Lerp>`: the endpoints come back shared, and only the samples between them allocate.
