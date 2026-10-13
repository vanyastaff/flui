### Fixed

- **Windows keystrokes could lose their translated text** (`flui-platform`): a keydown sometimes
  missed the character, dead-key or system-character message `TranslateMessage` had already
  queued for it, so a key typed twice (its fallback key, then the character as a second key-down)
  and a dead key arrived as its plain character. A keydown now takes the translated messages that
  carry its own scan code, read without the `PM_QS_POSTMESSAGE` wake bit that an earlier peek
  clears, and keeps them when a sent keydown or a nested message loop runs during its drain.
