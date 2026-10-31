### Added

- `flui_foundation::panic::discard_payload` destroys a caught panic payload under containment,
  so a payload whose destructor panics cannot escape a boundary that already caught its failure.

### Changed

- A caught panic payload that FLUI does not propagate is destroyed instead of leaked, under
  containment (`flui_foundation::panic::discard_payload`), and a value
  retired after a caught failure in the same operation (`PanicRecovery::retire`,
  `RecoveryScope::retire`) has its destructor run under that operation's containment, so the
  first failure still propagates. `flui-animation` destroys its sources, captures and waiters
  the same way. A destructor that panics while the thread unwinds aborts, as in std
  (ADR-0281).

### Removed

- `flui_foundation::panic::retain_opaque_payload`; use `discard_payload` instead.
