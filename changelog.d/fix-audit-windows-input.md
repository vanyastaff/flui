### Fixed

- **Windows window hover with the default mouse route** (`flui-platform`): under
  `EnableMouseInPointer` a mouse arrives as `WM_POINTER` messages, which never set the window's
  hover. `is_hovered`, `on_hover_status_change` and the immediate cursor apply of `set_cursor`
  now follow the mouse on that route too, and a hover change is raised once per enter and leave
  instead of on every legacy mouse move.
- **Windows pointers held across focus loss or minimise** (`flui-platform`): `WM_KILLFOCUS` and
  minimising end every open mouse, touch and pen sequence with a `FocusLost` cancel and cancel an
  open touchpad manipulation, so the moves and the release that still arrive for them deliver
  nothing and the next press starts clean.
- **Windows key timestamps** (`flui-platform`): key events carry the message's generation time on
  the clock the mouse uses, not the time they were processed, so a key and a click queued behind a
  long frame keep their order and spacing.
- **Windows candidate window placement** (`flui-platform`): moving the window, ending a
  size-and-move loop or a DPI change tells TSF the focused field's layout changed, so an input
  method's candidate window follows the text.
- **Windows pen precision** (`flui-platform`): pen positions come from the himetric location
  through the device's rectangles, keeping the fraction of a pixel a 120–240 Hz stroke moves.
