### Added

- `MemoryStorage::hold_copies() -> CopyHold`: no copy aside is answered while the hold is alive, and
  a copy asked for meanwhile is made from what is stored when the last hold drops, as a slow disk
  makes it.
