### Added

- `PlatformTextInput::set_field_status`: a push-model platform is told the focused field's `TextStoreStatus` as the field attaches and each time it changes. The method has no default body, so every backend states what it does with a protected field.
- `flui_testing::widgets::harness::Harness::field_status_calls` and `HeadlessWindow::ime_field_statuses` record the statuses a window's text input was told.

### Security

- iOS: a protected field's keyboard is secure (`isSecureTextEntry`), and the keyboard type, autocorrection and spell checking follow the field's purpose.
- winit: a protected field is named `ImePurpose::Password`, which reaches `text-input-v3` on Wayland as the password purpose with the sensitive-data hint.
