### Added

- **Input methods in Windows windows** (`flui-platform`): every Win32 window now activates the
  text services (TSF) as it is created and offers them as its text-store host, so an input
  method such as Microsoft IME composes directly into the focused field's text store instead of
  the window offering no input-method support. A window whose activation fails falls back to
  `WM_CHAR` text input. The window deactivates its text services when it is destroyed.

### Changed

- **A field commits its composition before it loses its input** (`flui-widgets`):
  `EditableText` commits an active IME composition, keeping its text, before it blurs, before a
  pointer-down on it moves the caret, and before a paste lands. A blur previously left the
  preedit in the field as uncommitted text. Paste is no longer disabled while a composition is
  active.
