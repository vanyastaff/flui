### Fixed

- `flui run --device <android serial>` no longer hangs when `adb` stops answering: the stop of the app before each start and at the end of the session (`q`, Ctrl-C) gives up after ten seconds instead of waiting for `adb` forever.
