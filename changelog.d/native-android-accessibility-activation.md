### Fixed

- **Android builds semantics only while an accessibility service is enabled** (`flui-native`):
  after any service once read the tree, every frame built and posted a semantics update for the
  rest of the process. The window's bridge now builds the tree only while Android reports an
  accessibility service enabled: when none is, nothing is built or posted, a question from
  anything else (Assist structure, the autofill framework's compatibility mode) is answered
  without starting the tree, and a service enabled again gets the whole tree built once. The
  APK's `FluiView` classes and the library must come from the same release (the Java protocol
  moved to 8).
- **A deactivated accessibility bridge holds no tree and always reaches the application**
  (`flui-native`): on Linux (AT-SPI) and iOS, the tree a bridge kept to answer a late screen reader
  is dropped when the last one leaves, a tree published after that is not kept, and the
  application hears the departure even when its listener failed part way through the arrival.
