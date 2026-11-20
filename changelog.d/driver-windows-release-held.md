### Changed

- `flui_driver::lane::release_outcome` gathers the releases of every part of a lane, replacing
  `flui_driver::lane::macos::release_outcome`; it takes the parts' results in the order they ran.

### Fixed

- The Windows lane's release of everything answers `Error::ReleaseFailed` naming every mouse
  button and touch or pen contact still held, not only the keys: a failed key release no longer
  hides them, and a failed button or contact release names what stayed down.
