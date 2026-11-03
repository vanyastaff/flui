### Fixed

- A clipboard read on Android, macOS, iOS and the web refuses text past the 64 MiB limit from
  its UTF-16 length, before copying it into Rust (and, on Android, before Java's `toString`
  copy). Before, those hosts copied the whole text and then refused it. Text within that bound is
  still measured exactly, so a read gives the same answer on every host.
