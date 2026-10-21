### Added

- **`TextStore::screen_moved`** (`flui-platform-api`): the platform tells a field's store that
  its text moved on the screen without a relayout (its window moved or changed scale); the store
  reports `layout_changed` under its own notification rule. Provided as a no-op;
  `InMemoryTextStore` and `EditableTextStore` report it.

### Fixed

- **Windows window hover with the default mouse route** (`flui-native`): under
  `EnableMouseInPointer` a mouse arrives as `WM_POINTER` messages, which never set the window's
  hover. `is_hovered`, `on_hover_status_change` and the immediate cursor apply of `set_cursor`
  now follow the mouse on that route too, and a hover change is raised once per enter and leave
  instead of on every legacy mouse move. A pointer capture lost while a press was dragged outside
  the window reads the hover from the cursor, since no leave follows it.
- **A contact removed without a leave** (`flui-native`): a hovering pen or the mouse that
  entered, ended by focus loss, minimise, device removal or a platform cancel, now delivers its
  `Leave`, as a touch already did.
- **Windows pointers held across focus loss or minimise** (`flui-native`): `WM_KILLFOCUS` and
  minimising end every open mouse, touch and pen sequence with a `FocusLost` cancel and cancel an
  open touchpad manipulation, so the moves and the release that still arrive for them deliver
  nothing and the next press starts clean.
- **Windows key timestamps** (`flui-native`): key events carry the message's generation time on
  the clock the mouse uses, not the time they were processed, so a key and a click queued behind a
  long frame keep their order and spacing; a key is never stamped before a `WM_POINTER` reading
  delivered ahead of it.
- **Windows candidate window placement** (`flui-native`): moving the window, ending a
  size-and-move loop or a DPI change tells the focused field's store, which reports the layout
  change to TSF when no lock is held, so an input method's candidate window follows the text.
- **Windows pen precision** (`flui-native`): pen positions come from the himetric location
  through the device's rectangles, keeping the fraction of a pixel a 120–240 Hz stroke moves.
