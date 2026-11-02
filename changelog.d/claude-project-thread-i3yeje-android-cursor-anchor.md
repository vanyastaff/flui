### Fixed

- **Android view placement** (`flui-native`): `FluiView` covers the whole window FLUI draws in,
  not the content frame below the status bar, so the input method's caret, a drop's point and
  the accessibility bounds are no longer a status bar's height off on a window that is not edge
  to edge.

### Added

- **Android composing character boxes** (`flui-native`): `CursorAnchorInfo` carries the box of
  each composing character, so an input method can place its candidates and handwriting feedback
  at the composing text.
