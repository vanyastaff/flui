### Added

- `flui_runtime::capability::HostCapabilities`: the scope a composition root
  acquires capabilities through for itself, outside every UI runtime, from the
  run's registry, with no window (ADR-0338).
- `flui_native::AppHandler::turn_deadline`, a required method: an instant at
  which every host gives the owner a turn with or without a window open and
  asks no window for a frame. That turn serves every instant not later than
  its own time, so a handler that keeps answering a past instant gets one turn,
  not a spinning loop (ADR-0343). An existing handler answers `None`.
- `flui::platform::WindowState::interaction` (`WindowInteraction`): whether
  the user is placing the window through the window system, stated on the
  report that carries the change. Win32 states `UserPlacing` during its
  size-move loop and the maximize, minimize and restore the user works, and
  `None` when they end; the other hosts state `Unknown` for now.
- Agent protocol 0.13: a recorded window state carries `user_placing`, an
  optional field absent where the host cannot tell, so a 0.12 reader still
  reads it.
- `flui_native::HeadlessOwnerTurns::drive_at` and
  `HeadlessHostWindow::simulate_interaction`, and the app test kit's
  `HeadlessRunHandle::turn_deadline` and `run_owner_turn_at`: a test serves the
  turn deadline on its manual clock.
- `flui::testing::MemoryStorage::hold_reads` (and `ReadHold`): a read asked
  while the hold lives answers only after it drops, as a slow store would.

### Fixed

- A window that remembers its placement (`AppConfig::restore_placement`)
  keeps it in the application's storage: the `StorageCapability` provider the
  application registers with `Application::capability` (an encrypted or
  sandboxed store), else the host's store. An application with its own
  provider and no data directory now has its window placement remembered.
- Reading a remembered placement no longer blocks the application's thread
  before the first window opens: the window waits a fraction of a second at
  most and then opens from its configuration. A storage provider that panics,
  refuses or holds bytes that are no placement leaves the window opening from
  its configuration instead of failing the launch, and a placement read too
  late is never applied to the window.
- A window that could not read its placement in time leaves the stored one
  alone until the report of a change says the user is moving, resizing or
  maximizing the window: a display or scale change, a window manager's
  adjustment, a command of the application or a change after the user let go
  does not overwrite it. On the hosts other than Windows, which cannot tell
  yet, it is not overwritten that run.
- A window reopened while its predecessor's last write is still out takes that
  write over: one writer per name, so storage never ends with the closed
  window's placement.
- A store that keeps refusing a window's placement is tried three times, each
  after twice the wait of the attempt before, and then not until the placement
  changes: an idle application no longer wakes every two seconds for it.
- Closing a window no longer blocks the application's thread while its last
  placement is written; the application's exit waits for those writes within
  its deadline, takes answers a storage provider posts back meanwhile, and
  reports by name each one storage did not take.
- At the end of the session, the window placements are written after the
  application has heard the session end and its documents were written, within
  a short bound of their own: a store that never answers no longer takes the
  application's time.
- Two windows asking to remember their placement under one name no longer
  overwrite each other: the second opens from its configuration and is not
  remembered.
- A window's placement is written again when the displays change under it (a
  moved taskbar, a rearranged desktop), since it is kept relative to its
  display's usable area.
