### Added

- **`flui-native`**: `Platform::primary_selection` hands out the host's primary selection,
  `NativePrimarySelection`, where the platform has one, and `None` elsewhere. The native Wayland
  host provides it through `zwp_primary_selection_device_manager_v1`. The application's
  selection is offered lazily: its text is asked only when another client reads it, at a turn of
  the loop and never inside the compositor's dispatch.
- **`flui-app`**: on Linux the built-in providers include `PrimarySelectionCapability` over the
  owner platform's primary selection. It is refused as unavailable where the host has none: winit,
  or a compositor without the protocol.
