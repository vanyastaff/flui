### Added

- **Navigation and press intents** (`flui-widgets`): `ActivationPressIntent` (Space's key-down),
  `BackIntent` and `DismissIntent`, with `SingleActivator::on_release` for bindings that fire on a
  key's release.
- **`FocusRing`, `FocusRingStyle` and `DefaultFocusRingStyle`** (`flui-widgets`): a two-band focus
  indicator drawn in front of and inside its child, shown only while the enclosing `Focus` has
  visible focus (keyboard or assistive technology, not a pointer); its style is explicit, else
  inherited from `DefaultFocusRingStyle`, else `FocusRingStyle::BUILT_IN`.

### Changed

- **Default key bindings** (`flui-widgets`): Space activates on its release (`ActivationPressIntent`
  on key-down); Enter, Select and Space activate once per press, not on auto-repeat; Escape sends
  `DismissIntent`; Alt+Left and `BrowserBack` (Cmd+[ and `BrowserBack` on macOS and iOS) send
  `BackIntent`.
- **`RawButton` takes keyboard focus** (`flui-widgets`): with `on_press` it is a Tab stop, answers
  `ActivateIntent` and `ButtonActivateIntent` with its press, consumes Space's key-down and draws a
  `FocusRing`; without `on_press` it takes no focus and an activation key bubbles past it.
- **`Focus` requests from assistive technology carry `FocusCause::Assistive`** (`flui-widgets`), so
  focus moved by a screen reader is shown like focus moved by Tab.

### Fixed

- **A focus request on a node `Focus` or `FocusScope` created and let go** (`flui-widgets`) answers
  `FocusRequestOutcome::Stale` instead of waiting forever as `Queued`.
- **`Focus` and `FocusScope` no longer panic when a node replacement is refused** during a focus
  notification (`FocusTreeError::NotificationInFlight`): they keep their current node and retry on
  the next update.
