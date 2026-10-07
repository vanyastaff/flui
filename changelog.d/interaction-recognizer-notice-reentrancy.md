### Added

- `GestureArenaEntry::abandon` ends one arena generation without a winner.

### Fixed

- A force press, scale or tap-and-drag callback that ends its own gesture reentrantly no longer
  receives a later notice of that gesture (a peak, an update) after its end; a completed tap's up
  is still delivered when `on_tap_down` admits the next contact.
- A cancelled contact in a self-driven arena no longer awards the arena to a rival: eager, force
  press, scale and tap-and-drag abandon the generation instead of sweeping it.
