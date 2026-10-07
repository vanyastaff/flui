### Added

- Owner-local binary arena branches for exclusive and require-first-failure
  recognizer competition. Fallback requests survive preferred rejection, owner
  release and deadline expiry without resolving a reused pointer generation.
- `GestureDetector::exclusive_drags()` explicitly permits pan and horizontal
  callbacks to compete for one winner. Double-tap and tap use the arena's failure
  relationship while preserving their existing hold timing.
