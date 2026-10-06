### Fixed

- `TextInputOwner::close` ends a frame transaction still open, so a composition completion queued in
  that frame commits (through the host, or in place when the host abandons it) before the store is
  retired, instead of leaving the field with a stale composing range.
- The Win32 text services contain a panic in a store's observer retirement: TSF is still moved back
  to the empty document, the panic is raised by the host operation afterwards and never unwinds
  out of a TSF call.
- The Win32 text services report a protected field's text as hidden to TSF (`GetStatus` follows
  the store's status), and `GetACPFromPoint` no longer overflows for extreme screen coordinates.
