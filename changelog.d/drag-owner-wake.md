### Changed

- **`DragOwner::new` takes an `OwnerWake`**, called from whichever thread the last request of a
  dropped offer resolves or is dropped on. The runtime passes the platform wake, so a drop whose
  fetch ends off the owner's thread is concluded with the OS on the next tick instead of waiting for
  unrelated input.
