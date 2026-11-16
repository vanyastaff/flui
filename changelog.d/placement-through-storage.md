### Added

- `flui_runtime::capability::HostCapabilities`: the scope a composition root
  acquires capabilities through for itself, outside every UI runtime, from the
  run's registry, with no window.
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
- Closing a window no longer blocks the application's thread while its last
  placement is written; the application's exit waits for those writes within
  its deadline and reports by name each one storage did not take.
- A window that could not read its placement in time does not overwrite the
  stored one until the user moves it.
- Two windows asking to remember their placement under one name no longer
  overwrite each other: the second opens from its configuration and is not
  remembered while the first is open.
- A window's placement is written again when the displays change under it (a
  moved taskbar, a rearranged desktop), since it is kept relative to its
  display's usable area.
