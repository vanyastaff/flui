### Fixed

- Wayland: on a compositor without `cursor-shape`, a theme cursor whose image size is no multiple
  of the window's scale (Adwaita's 64 pixels at scale 3) no longer risks the compositor closing
  the connection: the image is shown through a viewport, or at a scale that divides it.
- Wayland: a window asked for larger than the screen opens within the bounds the compositor
  recommends (`configure_bounds`); a size the application asks for later is not held to them.
- Wayland: where the compositor prefers no scale for a window, a window across outputs of
  different scales is drawn for the densest one, not for the first it entered.
- Wayland: a key held on a seat too old to be told its repeat (below version 4) repeats, at 25 a
  second after 600 ms, instead of never.
