### Added

- `flui_runtime::capability::HostCapabilities`: the scope a composition root
  acquires capabilities through for itself, outside every UI runtime, from the
  run's registry, with no window (ADR-0336).
- `flui_native::AppHandler::turn_deadline`: an instant at which the loop gives
  the owner a turn with or without a window open and asks no window for a
  frame. Served by Win32 (its modal loops included), X11, Wayland and AppKit.
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
  alone until the user moves, resizes or changes the mode of the window: a
  display change, a scale change or a command of the application does not
  overwrite it.
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
  remembered. A window reopened while its predecessor's last write is still out
  opens from that placement and still writes it if that write fails.
- A window's placement is written again when the displays change under it (a
  moved taskbar, a rearranged desktop), since it is kept relative to its
  display's usable area.
