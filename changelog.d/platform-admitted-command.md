### Added

- `AdmittedCommand`: the only thing `PlatformWindow::apply` takes, built only by
  `WindowCommand::admit(&WindowCapabilities)`. It is bound to the window and the capabilities
  revision it was admitted against; `AdmittedCommand::against` opens it under a window's current
  capabilities.
- `WindowCapabilities::revision` and `CapabilitiesOwner` (`WindowCapabilities::owned_by`): the
  snapshot revision at which what the window honours last changed, and the window, unique in the
  process, whose snapshots publish the capabilities. `WindowState::successor` keeps both.
- `AdmissionError { Unsupported, InvalidGeometry }`,
  `ApplyError { Closed, Superseded { command }, Platform(PlatformFailure) }`,
  `PlatformFailure { OutOfRange, Failed { message } }` and `SubmitError { Admission, Closed }`.
  `ApplyError::Superseded` says the command was admitted against capabilities the window does not
  have now, and gives the command back to admit again.
- `WindowCommand::admit_and_apply` and `CommandRefusal`: admit against a window's snapshot and
  apply at once, admitting once more when superseded.
- `MockWindow::simulate_capabilities`: the headless window system revises what a window honours.
- `flui_testing::window_commands::AdmitAndApply` (`admit_and_apply`, `close_now`,
  `close_for_cleanup`) and the conformance check `a_command_admitted_before_a_revision_is_superseded`
  with `WindowFixture::revise_capabilities`.
- `CloseRequestError::Failed`: the window took the close and its window system failed it, or it had
  already closed.

### Changed

- `PlatformWindow::apply` takes an `AdmittedCommand` and answers `ApplyError`;
  `WindowCommand::admit` takes the command by value and returns the token.
- `WindowControl::submit` and every `WindowHandle` command answer `SubmitError`.
- `CloseRequestError::Refused` carries the `AdmissionError` of a window that does not take a close.
- The application's window queue admits a command again when its capabilities changed between
  submission and the owner turn that applies it; a command the window no longer honours is refused
  then. A queued close the window refuses or fails leaves the rest of the batch to apply.
- A Win32 position or size beyond the device range is `PlatformFailure::OutOfRange`, no longer
  `InvalidGeometry`: admission held the numbers finite and positive, and the range is Win32's.

### Removed

- `WindowCommandError`: use `AdmissionError`, `ApplyError`, `SubmitError` or `CommandRefusal`.
