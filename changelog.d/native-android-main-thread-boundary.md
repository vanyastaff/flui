### Fixed

- Android: a native call from `FluiView` (a drag event, an input-method report, a node-provider
  query) whose JNI read fails or whose FLUI code panics no longer leaves a Java exception pending,
  which the framework callback above it threw on the main thread and ended the process with; the
  call answers its "nothing" (`false`, `0`, `null`) and the application goes on.
- Android: a panic while `FluiView` installs no longer leaves the window without the input
  capabilities the view gives it; the installation finishes and the window hears it either way.
