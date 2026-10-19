### Added

- **`flui-platform` (Android)**: text fields take input from the system keyboard. A
  `FluiView` laid over the `NativeActivity` serves the focused field's text store to the input
  method through its `InputConnection`: composition, commits, deletions, the editor action
  (as Enter or Tab) and pasted text, with the soft keyboard shown while a field is focused. A
  key the input method passes on while it composes arrives marked composing, so Escape cancels
  the composition first. The manifest stays `NativeActivity` with `hasCode="false"`: the view's
  dex is part of the native library
  ([ADR-0247](/docs/adr/ADR-0247-android-java-view.md)).
- **`flui-platform` (Android)**: drag and drop with other apps and windows. Drags over the
  window reach the runtime's drop targets, and a drag that leaves its window starts a system
  drag (`startDragAndDrop`, API 24); `InputCapabilities::outgoing_drag` says `Native` there.
- **`flui-platform` (Android)**: a stylus press inside the focused field starts the input
  method's handwriting (API 33).
- **`cargo xtask android-dex`**: checks the checked-in dex of `FluiView` against its Java
  sources; `--write` regenerates it with `javac` and `d8`.
