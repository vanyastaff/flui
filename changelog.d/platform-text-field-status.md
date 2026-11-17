### Added

- **`flui-platform`**: a field says what it asks of the input method and the on-screen keyboard
  in one value, `TextStoreStatus::text_input` (`TextInputOptions`): the label of the Enter key
  (`EnterAction`: Done, Go, Next, Previous, Search, Send), capitalization (`Capitalization`),
  suggestions and corrections (`Correction::{Off, Suggestions, Autocorrect}`), what the system may
  fill it with (`Autofill`: a user name, an existing or a new password, a one-time code), and a
  number's minus sign and decimal separator (`TextInputPurpose::Number(NumberFormat)`).
  `TextInputOptions::new(purpose)` picks the defaults from the purpose, and
  `TextStoreStatus::holds_secret` says whether the field holds a password or a one-time code,
  which no input method suggests, corrects or learns and no recording keeps. iOS, Android, the
  Web, Wayland, IBus, Fcitx5 and TSF pass on what their platform has a word for
  ([ADR-0352](/docs/adr/ADR-0352-a-text-field-asks-the-input-method-in-one-value-and-acts-on-its-enter-key-itself.md)).
- **`flui-widgets`**, **`flui-material`**: `EditableText::text_input`, forwarded by `TextField`,
  `RawTextFormField` and the Material fields. Enter on a field whose key says Next moves the focus
  to the next field after `on_submitted`, and Previous to the one before, on every host and from a
  hardware keyboard.
- **`flui-protocol`**: a native recording's store focus keeps every field of the status (protocol
  0.18; an older recording reads as it was taken).

### Changed

- **Text input**: free text is autocorrected by default on Android (the keyboard is now told
  `TYPE_TEXT_FLAG_AUTO_CORRECT`) and in the Web's hidden editor (`autocorrect`, `spellcheck`); an
  address or a number shows no suggestion strip on Android; iOS shifts no letter on its own unless
  the field asks for a `Capitalization`; the Web always states `enterkeyhint` (`enter` for the
  keyboard's own label), so Chrome for Android never turns the key into a focus move of its own.
- **Android**: every editor action arrives as Enter; Next and Previous used to arrive as Tab and
  Shift+Tab, which moved the focus without `on_submitted`. A change of the focused field's status
  restarts the keyboard only when what the keyboard reads changes, never during a composition, and
  never shows a keyboard the user put away; a protected number keeps the number pad, hidden.

### Fixed

- **`flui-platform`**: a push preedit keeps the input method's target clause: its cursor range is
  selected inside the composition, so a field can mark the part being converted, where only the
  caret at its end was kept.
- **Journal**: a field whose text is a password or a one-time code shown in clear is recorded as
  a protected field is, without its content.
