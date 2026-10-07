### Added

- Down callbacks can retain a weak, owner-affine pointer capture token to route later contact packets exclusively to their target. Releasing the token preserves accepted movement and delivers one capture-loss cancellation on the owning presentation; stale tokens cannot cancel replacement contacts.
