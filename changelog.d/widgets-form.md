### Added

- **`Form`, `FormField` and `TextFormField`** (`flui-widgets`, `flui-material`): Flutter's form
  with validation, the five `AutovalidateMode`s, `save`, `reset`, `force_error_text` and Tab
  traversal between fields, driven through `FormHandle`/`FormFieldHandle` (or `Form::of`)
  instead of a `GlobalKey<FormState>`. `flui_material::TextFormField` puts the field's error on
  its `InputDecoration`; `flui_widgets::RawTextFormField` is the theme-free sibling. The form is
  a semantics node with the form role, and a text field's error line is a live region. New
  example: `cargo run --example form`.
- **Copy, cut and paste in text fields**: `CopySelectionTextIntent` and `PasteTextIntent`, bound
  to Ctrl/Cmd+C, X and V by `DefaultFocusTraversal` and answered by `EditableText` over the
  presentation's clipboard. An obscured field copies nothing; a paste into a single-line field
  drops line breaks. Widgets reach the clipboard through `LifecycleContext::clipboard_handle`
  (`flui_interaction::ClipboardHandle`), which every realm installs over its platform's
  clipboard; the ADR-0084 capability registry replaces that method. `flui-platform-api` gains
  `InMemoryClipboard`, the headless backend's clipboard.
- **`EditableText::on_changed`** (also on `RawTextField` and the Material `TextField`): called
  with the new text after each user edit, not after the caller's own controller edits.
  `EditableText` also publishes a text-field semantics node (obscured, enabled, focused, value).

### Changed

- **`SingleActivator`** matches an ASCII letter trigger in either case, so Ctrl+C still fires
  with Caps Lock on; the Shift comparison stays exact.
- **Ctrl/Cmd+C in a focused text field with a selection** is consumed there, as in Flutter, so
  an app binding for that chord above the focus root no longer sees it then.
