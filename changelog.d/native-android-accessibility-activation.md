### Fixed

- **Android stops building semantics when no accessibility service is enabled**
  (`flui-native`): after any service once read the tree, every frame built and posted a semantics
  update for the rest of the process. The window's bridge now ends its tree when Android reports
  that no accessibility service is enabled; nothing is built or posted until a service asks again,
  and then the whole tree is built once. The APK's `FluiView` classes and the library must come
  from the same release (the Java protocol moved to 8).
- **A deactivated accessibility bridge holds no tree** (`flui-native`): on Linux (AT-SPI) and iOS,
  the tree a bridge kept to answer a late screen reader is dropped when the last one leaves, and a
  repeated notice of the leave no longer tells the application again.
