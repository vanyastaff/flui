### Fixed

- Wayland: an application whose window the compositor does not draw (minimized, on a hidden
  workspace) no longer freezes once that window asks for its next frame: a Wayland surface now
  presents in `Mailbox` (else `Immediate`), paced by the compositor's frame callback, instead of a
  `Fifo` present that waited for the compositor to show the window again. Other surfaces keep
  `Fifo`.

### Added

- `flui-native`'s `testing` kit has a Wayland compositor a test scripts
  (`testing::wayland::compositor::ScriptedCompositor`): it offers the globals and versions a test
  chooses and plays typed steps over a socket of its own (configure bounds, scale changes, an old
  seat, held-back frame callbacks, a removed output, a configure storm, a closed connection), and
  `WaylandPlatform::try_new_at` connects a test's host to it.
