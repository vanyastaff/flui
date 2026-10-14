### Added

- **Focus causes and visible focus** (`flui-interaction`): `FocusNode::request_focus_with` takes a
  `FocusCause` (`Programmatic`, `Traversal`, `Assistive`); `FocusNode::has_visible_focus` is true
  while the node has primary focus and the presentation is in keyboard modality — after traversal,
  an assistive focus move or a key press other than a modifier, until `FocusManager::note_pointer_input`.
  A change of modality notifies the focused node's listeners.
- **Traversal units** (`flui-interaction`): `FocusNode::register_traversal_entry` makes a node one
  Tab stop whose descendants are not Tab candidates; entering it asks its `TraversalEntry`, which
  answers an `EntryTarget` (`Node`, `Pending`, `Unit`), and scope focus history records the unit.
- **`FocusManager::settle_orphaned_focus`** and **`FocusManager::forget_pressed_keys`**
  (`flui-interaction`).

### Changed

- **Removing the focused node publishes nothing** (`flui-interaction`): the primary focus stays on
  the removed node until `FocusManager::settle_orphaned_focus` (or the next key, traversal or focus
  request) moves it in one edge to its successor — the node itself if it was put back, the
  enclosing traversal unit's entry, the next Tab candidate of its scope, else the first — and only
  to `None` when no candidate remains.
- **Key repeats and releases follow their key-down** (`flui-interaction`): the focus walk receives a
  key's repeats and release only while the traversal unit that received its key-down still has
  primary focus, and none of a key an input method was composing with. Global key handlers still
  see every event.

### Fixed

- **Replacing a focus node from a focus listener** (`flui-interaction`): `FocusAttachment::replace_node`
  returns `FocusTreeError::NotificationInFlight` before changing anything, in release builds too,
  instead of publishing an edge ahead of the notification in flight.
- **Requests on a widget's discarded node** (`flui-interaction`): once a widget detaches or replaces
  the node it created (`__runtime::mark_widget_owned`), `request_focus` answers
  `FocusRequestOutcome::Stale` instead of queueing a request no attachment will fulfill.
