### Changed

- **`flui-native`**: an Android Activity that is paused but still on screen (split screen, picture
  in picture, a translucent or dialog Activity on top) keeps drawing, taking input and running its
  owner jobs, and reports its lifecycle as `Inactive`; only `onStop` suspends the window and
  releases its surface (ADR-0366). While stopped, the loop answers every input event, so a touch
  held through Home no longer makes the system report the application not responding.
- **`flui-native`**: an Android process runs one FLUI platform, in a task of its own application.
  A `FluiActivity` that a share or a link from another application starts in that application's
  task, or that starts beside the running one, starts the Activity again in the application's own
  task with what it was opened with, granting it the shared documents, and finishes: the running
  Activity hears it as a new intent, and the launcher icon always brings it forward. The new
  `flui_native::android_hands_over` tells such an instance before anything starts, and
  `#[flui::main]`'s entry starts nothing for it; `android_platform` refuses it with the new
  `PlatformError::SecondActivity`. `FluiHost.PROTOCOL` is 7, so an APK needs the dex of this
  release.

### Fixed

- **`flui-cli`**: the generated Android manifest says what the host does with back: FLUI registers
  no back callback, so from Android 13 every back is the system's, which moves the task to the
  background (below Android 12 it finishes the Activity).
