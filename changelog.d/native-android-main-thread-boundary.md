### Fixed

- Android: a drag event or an input-method report from `FluiView` whose JNI read fails no longer
  leaves a Java exception pending, which the framework callback above it threw on the main thread
  and ended the process with; the call answers its "nothing" (`false`, no value) and the
  application goes on.
- Android: a panic while `FluiView` installs no longer leaves the view in the window without the
  window hearing of it; a view the failed installation left is taken out, and its handle is freed
  only once no view names it.
