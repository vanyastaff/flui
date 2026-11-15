### Fixed

- X11: a stream of events from the server no longer keeps a window's frames,
  the owner turn and the wake deadlines waiting. A turn of the loop takes at
  most 256 events, and a window reads its state from the server once a turn,
  after them, where every configure, map, focus, property or hover event cost
  it a blocking read.
- X11: a window follows a window manager that was killed or crashed. It read
  the mode and the frame the dead window manager had left on it, so a
  minimized window stayed `Minimized` for good, could not be restored, and
  `Activate` asked nobody; it reads normal, shown and unframed now, reports the
  modes only a window manager carries out as unavailable, and takes the focus
  from the server.
- X11: a minimized window the application hides reads hidden at once.
- X11: a master pointer the server removes is reported (`DeviceRemoved`), its
  press cancelled, and no longer counts against the devices the host knows.
- X11: a change of the keymap is read once, before the next key, however many
  changes the server reports.
- X11: reading the clipboard or the primary selection takes only text. An
  owner that answered with a property of another type or width had its bytes
  returned as text.
- X11: a change of the desktop or of the work area reads the work area only,
  not every output; the monitors, outputs and modes read from RandR are
  bounded.
- Linux: a `tracing` subscriber that samples the system preferences as it
  hears the warning about a refused text scale no longer deadlocks.

### Changed

- X11: the motions of a pointer that arrive in one turn of the loop are
  delivered as one `PointerEvent::Move` at the last position, with the earlier
  ones as its coalesced samples (the latest hundred), where each was a move of
  its own.
