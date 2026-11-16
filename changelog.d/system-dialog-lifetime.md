### Changed

- **`flui::services::system::dialog`**: `FileDialogs::open` and `save` return a `Dialog`, which
  is consumed like a completion (`on_complete`, `poll`, `into_future`); dropping it dismisses the
  system's dialog and frees the window. A dialog ends with its window: the caller hears
  `DialogError::WindowClosed`, the system's dialog is taken down, and a later dialog on the
  closed window answers `WindowClosed` without being shown. On Linux a dismissed dialog closes
  its portal request
  ([ADR-0344](/docs/adr/ADR-0344-a-dialog-lasts-while-its-caller-holds-it-and-ends-with-its-window.md)).
- **`flui::services::system::dialog`**: an open dialog answers `Option<Picks>`, a list that is
  never empty; `FileDialogBackend::open` and `save` return the `DialogHold` the dialog lasts by.
- **`flui::platform`**: `LaunchError::Failed` and `DialogError::Failed` carry a `SystemFailure`
  (an OS error code, a portal response, a bus failure, an answer outside the system's contract)
  instead of an integer; a portal interaction that ended some other way is
  `Failed(SystemFailure::PortalResponse(2))` rather than `LaunchError::NoHandler`.
- **`flui::platform::capability`**: `WindowRef::new` takes the window's `WindowLifetime`, which
  `WindowRef::lifetime` hands to providers.

### Added

- **`flui::platform::capability`**: `WindowLifetime`, `WindowCloseWatch` and `WindowCloser`, a
  provider's view of whether its window is still open and the notice when it closes;
  `scripted::RequestTicket` and `Script::receive_with_ticket`, a scripted request its caller can
  withdraw.

### Fixed

- **`flui::services::system::dialog`**: a portal success without picks, several picks for a
  single-pick request and several files for a save are `SystemFailure::MalformedAnswer`, not a
  cancel or a silent truncation; two widgets of one window share the one-dialog rule, a widget of
  another window does not.
