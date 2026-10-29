### Changed

- **`flui-native`**: a presentation's native text sizing is acquired with
  `flui_native::capture_text_sizing(&owner, &window)` instead of the method
  `OwnerPlatform::capture_text_sizing(&window)`; the owner is still the proof of the owner
  thread, and the answer is the same.
