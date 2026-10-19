### Changed

- **`GestureAlternatives::new`** (`flui-widgets`): refuses two commands bound to one key with
  `GestureAlternativesError::DuplicateKey`, instead of leaving which one the key performs to the
  order of the bindings.
- **`WidgetsLocalizations`** (`flui-widgets`): `search_results_found`, `no_results_found`,
  `context_menu_button_label` and `page_label` are required, so a locale states every string; the
  English ones are `DefaultWidgetsLocalizations`'.

### Removed

- **`SingleActivator::is_on_release`** (`flui-widgets`): nothing read it.

### Fixed

- **`Draggable`** (`flui-widgets`): the feedback sits under the pointer when the overlay is away
  from the window's origin, from its first frame.
- **`Focus` and `FocusScope`** (`flui-widgets`): a node replacement refused during a focus
  notification is made by the next rebuild, instead of never.
- **`Dismissible`** (`flui-widgets`): a row sliding in a lazy list keeps its slide and its contact
  when it scrolls out of the cache band and back.
- **`RawGestureDetector`** (`flui-widgets`): a rebuild whose retired recognizer panics on
  cancellation still reconfigures the recognizers it keeps.
- **`ContextMenuArea`** (`flui-widgets`): the menu's labels stay readable under any app text
  colour, and the menu control that keyboard focus reveals draws in the text colour around its
  row.
- **Gesture alternatives** (`flui-widgets`): a command's reserved key is reported once, not on
  every rebuild.
