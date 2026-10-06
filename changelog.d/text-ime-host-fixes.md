### Changed

- **`flui_platform_api::text_store::commit_composition_in_place`** returns the store's answer to
  its lock request (`Result<LockOutcome, TextStoreError>`): a `Deferred` commit is queued in the
  store and owed a later run.

### Fixed

- `TextInputOwner` records a queued in-place composition commit from the lock request's outcome,
  not from the frame transaction: a completion asked for from inside a grant on the same store
  that then panics is still run by the next anchor or the close. A `TextInputOwner` dropped
  without a close retires the stores and host operations it holds one at a time, so two panicking
  store destructors no longer abort the process.
- `TextInputOwner::close` ends a frame transaction still open, so a composition completion queued in
  that frame commits (through the host, or in place when the host abandons it) before the store is
  retired, instead of leaving the field with a stale composing range. On a push backend the close
  runs the queued grants of a store whose in-place commit waits behind the frame; every other
  store's queued grants are still cancelled.
- The Win32 text services contain a panic in a store's observer retirement: TSF is still moved back
  to the empty document, the panic is raised by the host operation afterwards and never unwinds
  out of a TSF call, even when the `tracing` subscriber logging it panics too. A composition
  committed in place after such a panic reports its own failures behind it.
- The Win32 text services report a protected field's text as hidden to TSF, and a change of the
  field's protection opens a new TSF document so TSF reads the static status again.
  `GetACPFromPoint` no longer overflows for extreme screen coordinates, and answers
  `TS_E_INVALIDPOINT` for a point with no finite logical position.
