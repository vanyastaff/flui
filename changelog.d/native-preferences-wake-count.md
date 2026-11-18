### Added

- **`flui-native`**: `Platform::preferences_delivered` and `OwnerPlatform::preferences_delivered`
  tell the host that the preferences `preferences()` returned reached the application. On macOS
  and Android a change counts as delivered only then, and one the application drops is read
  again in the next owner turn
  ([ADR-0377](/docs/adr/ADR-0377-preferences-arrive-by-notice-and-a-failed-read-backs-off.md)).

### Fixed

- **`flui-native`**: on macOS and Android a system preference change wakes the application once
  instead of twice. On Android the loop no longer reads the preferences every 500 ms while
  `FluiView` is not in the window, and a view that never gets there is logged once. A preference
  read that fails or panics is retried 0.5 to 16 s apart six times (on macOS only while the
  application is active) and then waits for the next change notice, instead of every 500 ms for
  as long as it fails. A panicking read on Android no longer ends the Activity.
