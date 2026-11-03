### Changed

- Android delivers every `onTrimMemory` as a memory-pressure host notice when it arrives, in the
  foreground and the background, forwarded by `FluiActivity` through a queue that buffers until the
  platform starts. The process trim level is no longer read at lifecycle callbacks, so a level is no
  longer noticed late or missed while the app is stopped.
