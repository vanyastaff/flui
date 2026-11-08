### Added

- `NavigatorHandle::remove_observer` is public, allowing observers and applications to remove a registration through the same handle used to add it.

### Fixed

- Removing a Navigator observer re-entrantly before its first `did_attach` now skips both lifecycle callbacks instead of delivering an unmatched `did_detach` followed by a stale `did_attach`. Attachment state is tracked per registration, including repeated additions of the same observer.
