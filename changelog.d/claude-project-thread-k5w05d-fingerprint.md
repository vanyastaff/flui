### Changed

- `DisplayList::fingerprint` hashes what a list's commands draw from their values, a payload by
  content, instead of hashing each command's `Debug` text; inspection's picture fingerprint uses
  it, so a release build no longer links display-list `Debug` formatting for it.
