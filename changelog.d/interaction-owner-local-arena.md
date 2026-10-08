### Fixed

- Keep gesture arena state on its input owner and retire resolution candidates
  after releasing storage borrows, allowing their destructors to reenter safely.
- Poll gesture deadlines in stable pointer and generation order; preserve the
  first retirement failure while delivering remaining notifications and keeping
  later contests usable.
- Refuse exhausted pointer signal handler identities permanently without
  wrapping or discarding accepted registrations.
