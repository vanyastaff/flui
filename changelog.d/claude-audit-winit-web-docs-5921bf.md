### Fixed

- **A minimized X11 window kept rendering** (`flui-platform`, winit backend): a window manager
  minimizes by unmapping, and X sends no visibility event for that, so winit never raised
  `Occluded`. A window whose WM state becomes minimized now reports `is_visible() == false`,
  publishes `visible: false` and raises `on_visibility_status_change`, and reports visible again
  when it returns. A winit window's visibility reports now follow `is_visible()` exactly: the
  `Hide` and `Show` commands are reported too, and an `Occluded` that repeats or arrives late
  raises nothing.
- **A Wayland window kept its first output's display** (`flui-platform`, winit backend): moving a
  window to an output of the same scale raises no winit event, and the idle check looked only at
  windows that had no display yet. Every Wayland window's display is now checked when the loop
  goes idle, so `on_display_changed` and the refresh period follow it.
- **A Web canvas stayed at the old pixel ratio** (`flui-platform`, Web backend): dragging the
  browser window to a display of another scale changes `devicePixelRatio` but not the canvas's
  CSS box, so neither the `ResizeObserver` nor `resize` resized the backing store. Each window
  now watches the ratio and resizes its backing store, raising `on_resize`, when it changes.
