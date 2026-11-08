### Changed

- `flui_native::MockWindow` is now `flui_native::HeadlessWindow`, the production headless window. Its material accessor reports current state without allocating a command history.
- Production headless text input, accessibility and system chrome retain no request history. Tests opt into recording facets and window histories explicitly with `HeadlessPlatform::with_test_doubles`.

### Removed

- `FakeTextInput`, `FakeAccessibility` and `FakeSystemChrome` no longer ship in `flui-native`'s default API. They are available from `flui_native::testing` with the `testing` feature.
