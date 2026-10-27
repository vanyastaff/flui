### Added

- `AdmittedCommand`: the only thing `PlatformWindow::apply` takes, built only by
  `WindowCommand::admit(&WindowCapabilities)`. It carries the revision of the capabilities it was
  admitted against; `AdmittedCommand::against` opens it under a window's current capabilities.
- `WindowCapabilities::revision`: the snapshot revision at which what the window honours last
  changed. `WindowState::successor` keeps it; it does not move with other facts.
- `AdmissionError { Unsupported, InvalidGeometry }`, `ApplyError { Closed, Superseded, Platform }`
  and `SubmitError { Admission, Closed }`. `ApplyError::Superseded` says the capabilities changed
  between admission and application: admit the command again.
- `MockWindow::simulate_capabilities`: the headless window system revises what a window honours.
- `flui_testing::window_commands::AdmitAndApply`: admit a command against a window's snapshot and
  apply it at once, for tests and fixtures with no queue in front of the window.
- `CloseRequestError::Failed`: the window took the close and its window system failed it, or it had
  already closed.

### Changed

- `PlatformWindow::apply` takes an `AdmittedCommand` and answers `ApplyError`;
  `WindowCommand::admit` takes the command by value and returns the token.
- `WindowControl::submit` and every `WindowHandle` command answer `SubmitError`.
- `CloseRequestError::Refused` carries the `AdmissionError` of a window that does not take a close.
- The application's window queue admits a command again when its capabilities changed between
  submission and the owner turn that applies it; a command the window no longer honours is refused
  then.
- A Win32 position or size beyond the device range is `ApplyError::Platform`, no longer
  `InvalidGeometry`: admission held the numbers finite and positive, and the range is Win32's.

### Removed

- `WindowCommandError`: use `AdmissionError`, `ApplyError` or `SubmitError`.
