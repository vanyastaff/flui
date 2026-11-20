### Added

- `flui-driver` has Linux lanes. `lane::x11::X11Lane` sends real input through XTEST (the mouse,
  the wheel, keys by the server's keymap, and text, a character on no key typed through a spare key
  mapped for it), refuses what is unsafe before every event (a window that is not the app's or lacks
  the focus, a point over another window, physical input held, a chord the desktop takes), stops on
  the first input its XInput 2 guard did not see it send (another program's XTEST input included),
  reads the app's AT-SPI tree, the one Orca reads, and captures its window through `GetImage`.
  `lane::wayland::WaylandLane` (the `wayland` feature) sends input through libei, from a
  compositor's EIS socket or the RemoteDesktop portal, only into a compositor of the run's own, and
  reads the app through AT-SPI too. `lane::atspi::roles` is the AT-SPI role table.
- A launched app on Linux leads a process group of its own, which the session ends, and dies with
  the driver (`PR_SET_PDEATHSIG`), however the driver goes, whatever thread launched it.
