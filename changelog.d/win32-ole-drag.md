### Added

- **`flui-native`**: Windows drag and drop through OLE, as target and as source
  ([ADR-0211](/docs/adr/ADR-0211-unified-drag-and-drop.md)). Every window registers an
  `IDropTarget`: a drag from this process or another arrives as `DragDropEvent`s whose offer
  carries text, HTML, files, media types and the FLUI session identity, the OS is answered from the
  target's cached feedback, and representations are fetched lazily through
  `Platform::data_transfer()`, now one OLE transport per platform. A Win32 window's
  `drag_source()` starts the OS session: `DoDragDrop` runs from the owner window's next turn, not
  inside the callback that decided the drag, and while its loop rests every window on the thread
  keeps getting its due frames and paints. Win32 windows report outgoing drags `Native`.

### Changed

- **`flui-native`**: the Win32 platform enters its thread's apartment with `OleInitialize`
  instead of `CoInitializeEx`, which drag and drop needs.
