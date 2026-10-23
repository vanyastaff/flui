### Added

- **`flui-platform`**: one vocabulary for "not here" (ADR-0151 §4): `Unsupported`, built with
  `Unsupported::new(reason)` and never written as a literal, and its `UnsupportedReason`
  (`NotOnThisPlatform`, `NotOnThisDevice`, `NotDeclared { what }`, `Unavailable`,
  `NotRegistered`, `NoWindow`). No reason names an operating-system version. `flui-sdk`
  re-exports both from `platform`.
- **`flui-platform`**: every error and outcome that can mean absence answers
  `unsupported() -> Option<Unsupported>` and converts from `Unsupported`.
- **`flui-native`**: `OpenWindowError::Unsupported` and `PlatformError::Unsupported`, which a
  host whose table leaves the window or the display position out answers with
  `NotOnThisPlatform`.

### Changed

- **`flui-platform`**: the unit variants that said "not here" carry an `Unsupported` with its
  reason: `WindowCommandError::Unsupported(_)`, `WindowShowError::Unsupported(_)`,
  `CursorError::Unsupported(_)`, `DragError::Unsupported(_)` (was `DragError::Unavailable`),
  `HapticOutcome::Unsupported(_)` (was `HapticOutcome::Unavailable`),
  `StorageError::Unsupported(_)` (was `StorageError::Unavailable` and
  `StorageError::LockUnsupported`, now `NotOnThisDevice`),
  `PreferenceQueryError::Unsupported(_)` (was `PreferenceQueryError::Unavailable`) and
  `TextStoreHostError::Unsupported(_)` (was `TextStoreHostError::Unavailable`). `CursorError`
  is `#[non_exhaustive]`.
- **`flui-platform`**: `Availability` is named at the crate root
  (`flui_platform::Availability`), not in `input_capabilities`; `flui-sdk` keeps it at
  `platform::input_capabilities::Availability` under `unstable-input`.
- **`flui-native`**: a window's haptics answer why nothing played: no pen in range of a Win32
  window is `Unavailable` and a pen without a controller `NotOnThisDevice`; a browser without
  vibration is `NotOnThisDevice` and a page before user activation `Unavailable`.
- **`flui-platform`**: `PlatformWindow::text_input`, `haptics` and `system_chrome` have no
  default body: every window says what it hands out, and one without the capability answers
  `None` itself.
- **`flui-native`**: every window hands out the text input, accessibility bridge, haptic
  actuator and system chrome its host's table made for it as it opened, and
  `HostWindow::accessibility` has no default body: a window on a host without an
  accessibility integration, or built without `a11y`, answers a bridge that is never active
  instead of `None`. An iOS window's haptic actuator is made with the window rather than on
  first use.
- **`flui-platform`**: `WindowAppearance` and `WindowEvent` are `#[non_exhaustive]`: a backend
  may report a new appearance or window event, so a `match` on them needs a wildcard arm.
- **`flui-testing`**: `window_conformance`'s `capabilities_are_honest` also fails a window that
  refuses a command its capabilities leave out as worth asking again (`Unavailable`): such a
  refusal is for good.
- **`flui-native`**: `MockWindow::simulate_move_to` refuses a display the platform does not hold
  with `UnknownDisplay`, which names the display, instead of
  `DisplayChangeError::UnknownDisplay`.
