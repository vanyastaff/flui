### Fixed

- Android reads and writes the system clipboard (`ClipboardManager`, through JNI) instead of an in-process stand-in, so copy and paste reach other apps.
