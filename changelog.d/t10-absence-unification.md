### Changed

- `flui_platform::launch::LaunchError::Unavailable(UnsupportedReason)` and
  `flui_system::dialog::DialogError::Unavailable(UnsupportedReason)` are now
  `Unsupported(#[from] Unsupported)`, matching `CursorError`/`StorageError`: the
  launch/dialog services report their absence through the unified `Unsupported`
  value (ADR-0152) instead of a raw reason, and the "retry later may help"
  distinction stays inside `UnsupportedReason::Unavailable`.
- Both errors answer `unsupported() -> Option<Unsupported>` like the other
  carriers, and their message no longer repeats the absence wording.
