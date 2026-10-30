### Added

- **`flui-native`**: the native Wayland host has a clipboard: `Platform::clipboard` reads and
  writes the seats' selection through `wl_data_device`, so text copied in another Wayland client
  pastes into a FLUI window and back. A read of another client's selection answers when its text
  has arrived; one of the application's own selection answers at once.
