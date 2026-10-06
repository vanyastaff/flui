### Fixed

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
