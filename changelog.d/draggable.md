### Added

- **`flui-widgets`**: `DropTarget` receives drops from a `Draggable`, from another window of the
  app and from other applications through the operating system's drag
  ([ADR-0211](/docs/adr/ADR-0211-unified-drag-and-drop.md)). `accept` answers each enter and move;
  `on_drop` runs only on the target that accepted the last move; `spring_loaded` runs once per
  hover; the builder shows its `DropHover`. It is the `DragTarget` of other frameworks
  (`#[doc(alias)]`).
- **`flui-view`**: `LifecycleContext::drag_and_drop`, the presentation's `DragAndDrop` capability.
- **`flui-interaction`**: `DragAndDrop`, one presentation's way into its runtime's drag sessions:
  drags that leave through its window and show their state on its cursor, and the cut, copy and
  paste that reach a drop from the keyboard and assistive technology (`hold`, `paste`);
  `DragOwner::cancel_on_escape`; `DragView::identity` (`DragIdentity`) tells the drags over one
  receiver apart.
- **`flui`, `flui-sdk`**: `interaction::drag`, the drag-and-drop types a source or receiver names.
- **`flui-testing`**: `HeadlessWindow::os_drags` plays the operating system's drag sessions
  (`HeadlessOsDrags::end_with`, `started`); `LaidOut::dispatch_key` delivers a key through the
  UI runtime's input path.

### Changed

- **`flui-widgets`**: `Draggable` starts a drag-and-drop session: `Draggable::new(DragCommands,
  items, child)` carries `DragItem`s, allows `AllowedActions` within and outside the app, starts at
  once or after a long press (`DragTrigger`, a `Sequenced` hold), escalates to the operating
  system's drag when the pointer leaves the window and the drag may reach beyond it, plays the
  lift and drop haptics, and reports one `DragOutcome` to `on_drag_end`, the callback the drag
  started with. Its `DragCommands` and the platform's cut and copy keys
  (`CopySelectionTextIntent`) hold the items for a paste into a `DropTarget` (`PasteTextIntent`).
  The feedback follows the pointer's window position (`DragAnchor`).
- **`flui-runtime`**: Escape cancels a presentation's drags in its window, and a held cut or copy,
  before focus sees the key; with nothing to cancel the key reaches focus as before.
- **`flui-interaction`**: a mouse or pen drag owns the window's cursor while it runs in its
  window: grabbing, copy, alias or no-drop by what a drop would do.

### Removed

- **`flui-widgets`**: `DragTarget<T>` and its `DragTargetSlot`, `DragTargetDetails`,
  `DragTargetBuilder`, `DragTargetWillAccept`, `DragTargetAccept`, `DragTargetLeave`,
  `DragTargetMove`, `DragTargetState`, `DragPosition` and `ErasedDragData`; `Draggable<T>`'s
  `DraggableDetails`, `DraggableCanceledDetails`, `data`, `axis`, `feedback_offset`,
  `max_simultaneous_drags`, `on_drag_update`, `on_draggable_canceled` and `on_drag_completed`.
  Use `DropTarget`, `DragItem::with_value`, `DragView::value`, `DragAnchor` and
  `on_drag_end(DragOutcome)`.

### Fixed

- **`flui-interaction`**: a drag whose receiver panics on release still ends: the source hears it
  cancelled, its cursor and capture are released, and the panic propagates afterwards.
- **`flui-interaction`**: content an autoscroll moves under a resting pointer is found: after a
  frame that scrolled, an in-window drag hit-tests again where its pointer rests, so the target
  under it is entered without the pointer moving.
