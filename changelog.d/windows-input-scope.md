### Added

- `TextInputPurpose` (free text, email, URL, phone, number, search) and `TextStoreStatus::purpose`,
  set from `EditableText::input_purpose`, `TextField::input_purpose` and
  `RawTextFormField::input_purpose` and the Material `TextField` and `TextFormField` (the type is `flui_sdk::platform::TextInputPurpose`). It shapes the layout and suggestions an input method
  offers and never filters what the field accepts.

### Fixed

- Windows: an obscured field tells input methods it is a password field (TSF input scope
  `IS_PASSWORD`), so a Japanese or Chinese IME no longer shows the plain reading in its
  candidate window and the touch keyboard gets the password layout. Email, URL, phone, number
  and search fields ask for the matching scope.
