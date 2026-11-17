### Added

- `TextInputPurpose` (free text, email, URL, phone, a number with its `NumberFormat`, search),
  part of the `TextInputOptions` a field passes to `EditableText::text_input`,
  `TextField::text_input`, `RawTextFormField::text_input` and the Material `TextField` and
  `TextFormField` (the types are in `flui_sdk::platform`). It shapes the layout and suggestions an
  input method offers and never filters what the field accepts.

### Fixed

- Windows: an obscured field tells input methods it is a password field (TSF input scope
  `IS_PASSWORD`), so a Japanese or Chinese IME no longer shows the plain reading in its
  candidate window and the touch keyboard gets the password layout. Email, URL, phone, number
  and search fields ask for the matching scope.
