### Changed

- On Android the fingers of one `ACTION_MOVE` reach the app as one `PlatformInput::PointerFrame`, so a pinch is measured once per event instead of once per finger.
