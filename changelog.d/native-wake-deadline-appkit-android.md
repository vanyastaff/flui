### Fixed

- On macOS and Android a wake deadline whose frame the application declines no longer keeps the
  event loop awake: the AppKit wake pump asked for a frame every 8 ms, and the Android loop polled
  without a timeout, for as long as the deadline stayed past. Both now deliver a due deadline once
  and deliver the same instant again only after a frame ran, as the Windows, X11, Wayland and iOS
  loops do.
