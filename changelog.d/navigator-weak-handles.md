### Fixed

- An application unmounted with a dialog or menu open is freed. A popup's builder that captured
  its page's `PageHandle` or a `RouterHandle` kept the navigator, its routes and the builder alive
  in a cycle (about 26 KB a time in a Material application). Both handles now hold the navigator
  weakly; once it is gone, a `PageHandle` operation answers `PageError::Gone` and a
  `RouterHandle` edit answers `RouterError::Disposed`.
