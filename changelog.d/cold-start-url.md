### Added

- A URL opened before a window's first frame (`UiRuntime::open_url`, `HeadlessHost::open_url`)
  becomes the location its first `Router` opens on: the stack `back_stack` derives from it replaces
  the Router's initial stack, whose pages are never built. A location no route matches keeps the
  initial stack and reaches `on_unknown` after the frame, where the hook may navigate. A location no
  Router took by the end of the first frame is dropped with a warning.
