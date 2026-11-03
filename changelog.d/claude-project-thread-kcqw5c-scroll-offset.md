### Fixed

- A `ScrollableViewportOffset` listener that panics no longer silences the offset: the listeners
  after it still run, the next scroll reaches every listener, and the first panic resumes once
  the round has finished. The offset's listeners and `ScrollPosition`'s render listeners now
  share one `ChangeNotifier`-backed set, and a carousel's pause listeners use a `ChangeNotifier`.
