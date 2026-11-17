### Added

- **`flui-platform`**: a field says what it asks of the input method and the on-screen keyboard
  in one value, `TextStoreStatus::text_input` (`TextInputOptions`): the label of the Enter key
  (`EnterAction`: Done, Go, Next, Previous, Search, Send), capitalization (`Capitalization`),
  suggestions and corrections (`Correction::{Off, Suggestions, Autocorrect}`), what the system may
  fill it with (`Autofill`: a user name, an existing or a new password, a one-time code), and a
  number's minus sign and decimal separator (`TextInputPurpose::Number(NumberFormat)`).
  `TextInputOptions::new(purpose)` picks the defaults from the purpose. iOS, Android, the Web,
  Wayland, IBus, Fcitx5 and TSF pass on what their platform has a word for
  ([ADR-0352](/docs/adr/ADR-0352-a-text-field-asks-the-input-method-in-one-value-and-acts-on-its-enter-key-itself.md)).
- **`flui-widgets`**, **`flui-material`**: `EditableText::text_input`, forwarded by `TextField`,
  `RawTextFormField` and the Material fields. Enter on a field whose key says Next moves the focus
  to the next field after `on_submitted`, and Previous to the one before, on every host and from a
  hardware keyboard.
- **`flui-protocol`**: a native recording's store focus keeps every field of the status (protocol
  0.18; the new fields are optional and an older recording reads as it was taken).

### Changed

- **`flui-platform`**: `TextStoreStatus::purpose` and `with_purpose` are replaced by
  `text_input` and `with_text_input`; `TextStoreStatus::with_single_line` is new.
  `TextInputPurpose::Number` carries a `NumberFormat` (`NumberFormat::WHOLE` for what it meant
  before).
- **`flui-widgets`**, **`flui-material`**: `input_purpose(purpose)` on `EditableText`, `TextField`,
  `RawTextField`, `RawTextFormField`, `TextFormField` and the Material `TextField` is replaced by
  `text_input(TextInputOptions::new(purpose))`.
- **Text input**: free text is autocorrected by default on Android (the keyboard is now told
  `TYPE_TEXT_FLAG_AUTO_CORRECT`) and in the Web's hidden editor (`autocorrect`, `spellcheck`); an
  address or a number shows no suggestion strip on Android; iOS shifts no letter on its own unless
  the field asks for a `Capitalization`.
- **Android**: every editor action arrives as Enter; Next and Previous used to arrive as Tab and
  Shift+Tab, which moved the focus without `on_submitted`.
- **`flui-interaction`**: `InMemoryTextStore::set_purpose` is replaced by `set_text_input`.

### Fixed

- **`flui-platform`**: a push preedit keeps the input method's target clause: its cursor range is
  selected inside the composition, so a field can mark the part being converted, where only the
  caret at its end was kept.
