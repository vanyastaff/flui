### Added

- `ClipboardWrite::expires_after(ClipboardExpiry)` writes a concealed entry that clears itself at
  its time, when the user's session locks or the machine sleeps, and when the application ends,
  if the clipboard still holds that entry: a later copy, by the application or another one, is
  never cleared (ADR-0362). `ClipboardExpiry::try_new` takes a second to a day.
- `ClipboardHandle::expiry_clearing` says what the host does with an expiring write
  (`ExpiryClearing`): the web cannot clear it, and iOS and Android hear no lock.

### Changed

- `ClipboardBackend` and `NativeClipboard` implementors answer `expiry_clearing`;
  `NativeClipboard` implementors also answer `clear_expired` and `clear_expired_before_exit`.
