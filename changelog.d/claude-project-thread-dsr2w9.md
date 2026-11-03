### Added

- `flui_foundation::panic::discard_payload` destroys a caught panic payload under containment,
  so a payload whose destructor panics cannot escape a boundary that already caught its failure.
- `flui_foundation::panic::drop_contained` destroys a value after a caught failure that stays the
  first one: its destructor runs under containment and a panic it raises is discarded.

### Changed

- A caught panic payload that FLUI does not propagate is destroyed instead of leaked, under
  containment (`flui_foundation::panic::discard_payload`), and a value
  retired after a caught failure in the same operation (`PanicRecovery::retire`,
  `RecoveryScope::retire`) has its destructor run under that operation's containment, so the
  first failure still propagates. `flui-animation` destroys its sources, captures and waiters
  the same way. A destructor that panics while the thread unwinds aborts, as in std
  (ADR-0281).
- Every other crate does the same: interaction, scheduler, runtime, view, widgets, platform,
  semantics, rendering, app and material no longer leak a callback capture, a retired value or a
  panic payload on a failed path or during an unwind; each is destroyed under containment and the
  first failure still propagates (ADR-0281).
- `FirstPanic::resume` called while the thread is already unwinding (from a `Drop` an earlier
  failure runs) discards the panic it holds instead of raising it, which would abort: the
  unwinding failure is the first one.

### Removed

- `flui_foundation::panic::retain_opaque_payload`; use `discard_payload` instead.
- `flui_platform::text_store::RetainOnFailure` and `flui_scheduler::PostFrameRejection::Retained`:
  nothing is retained after a failure any more.
