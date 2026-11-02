### Added

- **`flui-native`**: the native X11 host serves the focused text field to IBus or Fcitx5 over
  D-Bus, one input context per window: the preedit composes in the field, a commit replaces it, a
  deletion around the caret removes whole characters, and the field's text, caret, purpose and
  caret rectangle reach the input method, again as the window moves on the screen. Each key of a composing field waits for the input
  method's answer and keys behind it wait in order; a key unanswered within 200 ms, the keys of a
  window losing the focus or closing, and every key once the input method is gone are delivered as
  typed in their original order, after the composition ends in the field; an input method that
  stops leaves the field even while the session bus runs; an input method that leaves a key
  unanswered has its context replaced, so a late edit for that key types nothing. `flui-app`
  still picks winit on Linux.
